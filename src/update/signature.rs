//! Whether a download is Phantom Switch's: its ECDSA P-256 signature (`<file>.sig`, the base64 of
//! the 64-byte signature of the file's SHA-256, made by `tools/sign`) checked against the public
//! key this build carries, with Windows' own cryptography (CNG): nothing of it in the `.exe`. Only
//! whoever holds the private key can sign.

use std::ptr::{null, null_mut};

use windows_sys::Win32::Security::Cryptography::{
    BCRYPT_ECCKEY_BLOB, BCRYPT_ECCPUBLIC_BLOB, BCRYPT_ECDSA_P256_ALG_HANDLE, BCRYPT_ECDSA_PUBLIC_P256_MAGIC,
    BCRYPT_SHA256_ALG_HANDLE, BCryptDestroyKey, BCryptHash, BCryptImportKeyPair, BCryptVerifySignature,
};

/// The public key, base64; all zeros in a build without one, which then installs nothing.
const PUBLIC_KEY: &str = include_str!("../../assets/release-key.pub");
/// A coordinate of P-256, a SHA-256 and each half of a signature.
const SIDE: usize = 32;
/// A public key: its two coordinates, x and y.
const KEY_LENGTH: usize = 2 * SIDE;
const SIGNATURE_LENGTH: usize = 2 * SIDE;

/// Whether this build can check a signature: without a key, updates are downloaded by hand.
pub fn has_key() -> bool {
    key(PUBLIC_KEY).is_some()
}

/// Whether `signature` (base64) is the one of `data` with the key this build carries.
pub fn is_authentic(data: &[u8], signature: &[u8]) -> bool {
    key(PUBLIC_KEY).is_some_and(|key| verify(&key, data, signature))
}

fn key(text: &str) -> Option<[u8; KEY_LENGTH]> {
    let key: [u8; KEY_LENGTH] = base64(text)?.try_into().ok()?;
    (key != [0; KEY_LENGTH]).then_some(key)
}

fn verify(key: &[u8; KEY_LENGTH], data: &[u8], signature: &[u8]) -> bool {
    let Some(signature) = std::str::from_utf8(signature).ok().and_then(base64) else { return false };
    let Ok(length) = u32::try_from(data.len()) else { return false };
    if signature.len() != SIGNATURE_LENGTH {
        return false;
    }
    // How CNG takes a public key: what it is and its side, then x and y.
    let header = BCRYPT_ECCKEY_BLOB { dwMagic: BCRYPT_ECDSA_PUBLIC_P256_MAGIC, cbKey: SIDE as u32 };
    let mut blob = Vec::with_capacity(size_of::<BCRYPT_ECCKEY_BLOB>() + KEY_LENGTH);
    blob.extend(header.dwMagic.to_le_bytes());
    blob.extend(header.cbKey.to_le_bytes());
    blob.extend(key);
    let mut hash = [0u8; SIDE];
    // SAFETY: buffers of the lengths given, which live through each call; the key handle is
    // destroyed once checked. The algorithm handles are Windows' own, never closed.
    unsafe {
        let hashed =
            BCryptHash(BCRYPT_SHA256_ALG_HANDLE, null(), 0, data.as_ptr(), length, hash.as_mut_ptr(), SIDE as u32);
        let mut handle = null_mut();
        let imported = BCryptImportKeyPair(
            BCRYPT_ECDSA_P256_ALG_HANDLE,
            null_mut(),
            BCRYPT_ECCPUBLIC_BLOB,
            &mut handle,
            blob.as_ptr(),
            blob.len() as u32,
            0,
        );
        if hashed != 0 || imported != 0 {
            return false;
        }
        let status = BCryptVerifySignature(
            handle,
            null(),
            hash.as_ptr(),
            SIDE as u32,
            signature.as_ptr(),
            SIGNATURE_LENGTH as u32,
            0,
        );
        BCryptDestroyKey(handle);
        status == 0
    }
}

/// Standard base64, with or without its padding and the line break of its file.
fn base64(text: &str) -> Option<Vec<u8>> {
    let text = text.trim().trim_end_matches('=');
    let mut bytes = Vec::with_capacity(text.len() * 3 / 4);
    let (mut bits, mut pending) = (0u32, 0);
    for c in text.bytes() {
        let value = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        };
        bits = (bits << 6 | u32::from(value)) & 0xFFFF;
        pending += 6;
        if pending >= 8 {
            pending -= 8;
            bytes.push((bits >> pending) as u8);
        }
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A key and a signature of `MESSAGE` made by `tools/sign` (RustCrypto's p256), so CNG is
    /// checked against what releases are signed with.
    const TEST_KEY: &str = "Ilm6EvxQ2gANWmlb23p1mawxvwwxftwNf3hOSsgX3TbBiWlDwMTOM80WdbT208AyL2O5txmiGBZHkRmm8HDgzQ==";
    const TEST_SIGNATURE: &[u8] =
        b"TZSWH6Q4iil8ZnoQc64dHfKMO8l03pzvJK5e8lG1Aq/GCbX1FbxLxbFGYbxUEOqbj/BvDKdO5Un2X2qmrq9XJA==\n";
    const MESSAGE: &[u8] = b"MZ pretend this is PhantomSwitch.exe";

    #[test]
    fn reads_base64() {
        assert_eq!(base64("TWFu").as_deref(), Some(&b"Man"[..]));
        assert_eq!(base64("TWE=\n").as_deref(), Some(&b"Ma"[..]));
        assert_eq!(base64("TQ==").as_deref(), Some(&b"M"[..]));
        assert_eq!(base64("").as_deref(), Some(&b""[..]));
        assert_eq!(base64("TW#u"), None);
    }

    #[test]
    fn checks_a_signature() {
        let Some(key) = key(TEST_KEY) else { panic!("the test key") };
        assert!(verify(&key, MESSAGE, TEST_SIGNATURE));
        assert!(!verify(&key, b"MZ something else", TEST_SIGNATURE));
        assert!(!verify(&key, MESSAGE, b"not a signature"));
        let mut other = TEST_SIGNATURE.to_vec();
        other[10] = if other[10] == b'A' { b'B' } else { b'A' };
        assert!(!verify(&key, MESSAGE, &other));
        let mut other_key = key;
        other_key[0] ^= 1;
        assert!(!verify(&other_key, MESSAGE, TEST_SIGNATURE));
    }

    #[test]
    fn the_key_of_zeros_is_no_key() {
        assert!(key(&format!("{}==", "A".repeat(86))).is_none());
        assert!(key("TWFu").is_none());
    }

    #[test]
    fn the_carried_key_is_two_coordinates() {
        assert_eq!(base64(PUBLIC_KEY).map(|bytes| bytes.len()), Some(KEY_LENGTH));
    }
}
