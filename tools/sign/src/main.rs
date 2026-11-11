//! Signs Phantom Switch's releases: the app installs an update only with its ECDSA P-256
//! signature, checked with Windows' own cryptography against the public key it carries
//! (`assets/release-key.pub`, its x and y). Only whoever holds the private key, a secret of the
//! repository, can publish one.
//!
//! - `cargo run -p sign -- keygen > key.txt`, once: makes a key pair, writes the public key to
//!   `assets/release-key.pub` and the private key to standard output, never to the terminal.
//!   It is set as the repository's secret `PHANTOM_SIGNING_KEY` and kept somewhere safe: without
//!   it, the copies already installed can no longer update themselves.
//! - `cargo run -p sign -- <file>…`, with `PHANTOM_SIGNING_KEY` set: writes `<file>.sig`, the
//!   base64 signature (r and s) of the file's SHA-256, after checking that the key is the one the
//!   app carries.
//!
//! From Linux, add `--target x86_64-unknown-linux-gnu`.

use std::error::Error;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::{env, fs};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use p256::ecdsa::signature::{Signer, Verifier};
use p256::ecdsa::{Signature, SigningKey, VerifyingKey};

/// Where the app's public key lives, from the repository's root.
const PUBLIC_KEY: &str = "assets/release-key.pub";
/// The private key, base64, as the release workflow gets it.
const SECRET: &str = "PHANTOM_SIGNING_KEY";
/// A P-256 private key, and each of a public key's coordinates.
const SIDE: usize = 32;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn main() -> Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        [] => Err("usage: sign keygen [--replace] > key.txt | sign <file>…".into()),
        ["keygen"] => keygen(false),
        ["keygen", "--replace"] => keygen(true),
        files => files.iter().try_for_each(|file| sign(Path::new(file))),
    }
}

fn keygen(replace: bool) -> Result<()> {
    if std::io::stdout().is_terminal() {
        return Err("the private key would show on screen: send it to a file, `keygen > key.txt`".into());
    }
    if embedded_key()?.is_some() && !replace {
        return Err(format!(
            "{PUBLIC_KEY} already has a key; a new one leaves the installed copies unable to update \
             themselves. Add --replace if that is what you want."
        )
        .into());
    }
    // A random number below the curve's order: nearly every one is.
    let (secret, key) = loop {
        let mut secret = [0u8; SIDE];
        getrandom::fill(&mut secret).map_err(|err| format!("no randomness from the system: {err}"))?;
        if let Ok(key) = SigningKey::from_slice(&secret) {
            break (secret, key);
        }
    };
    fs::write(root().join(PUBLIC_KEY), format!("{}\n", STANDARD.encode(coordinates(key.verifying_key()))))?;
    writeln!(std::io::stdout(), "{}", STANDARD.encode(secret))?;
    eprintln!(
        "Public key → {PUBLIC_KEY}: commit it.\n\
         Private key → standard output: set it as the repository's secret {SECRET}\n\
         (`gh secret set {SECRET} < key.txt`), keep a copy somewhere safe and delete the file."
    );
    Ok(())
}

fn sign(file: &Path) -> Result<()> {
    let secret = env::var(SECRET).map_err(|_| format!("{SECRET} is not set"))?;
    let key = STANDARD
        .decode(secret.trim())
        .ok()
        .and_then(|bytes| SigningKey::from_slice(&bytes).ok())
        .ok_or(format!("{SECRET} is not a base64 P-256 private key"))?;
    let embedded = embedded_key()?.ok_or(format!("{PUBLIC_KEY} has no key yet: run `sign keygen` first"))?;
    if key.verifying_key() != &embedded {
        return Err(format!("{SECRET} is not the key of {PUBLIC_KEY}: the app would refuse this release").into());
    }
    let data = fs::read(file).map_err(|err| format!("{}: {err}", file.display()))?;
    let signature: Signature = key.sign(&data);
    embedded.verify(&data, &signature)?;
    let mut sig = file.as_os_str().to_owned();
    sig.push(".sig");
    fs::write(&sig, STANDARD.encode(signature.to_bytes()))?;
    eprintln!("Signed {} → {}", file.display(), PathBuf::from(sig).display());
    Ok(())
}

/// The key the app carries; `None` while it is the placeholder of zeros.
fn embedded_key() -> Result<Option<VerifyingKey>> {
    let text = fs::read_to_string(root().join(PUBLIC_KEY))?;
    let bytes = STANDARD
        .decode(text.trim())
        .ok()
        .filter(|bytes| bytes.len() == 2 * SIDE)
        .ok_or(format!("{PUBLIC_KEY} is not a P-256 public key: base64 of its x and y"))?;
    if bytes.iter().all(|byte| *byte == 0) {
        return Ok(None);
    }
    // SEC1, uncompressed: 4, then x and y.
    let sec1: Vec<u8> = [4].into_iter().chain(bytes).collect();
    Ok(Some(VerifyingKey::from_sec1_bytes(&sec1)?))
}

/// What the app carries of a public key: x and y.
fn coordinates(key: &VerifyingKey) -> Vec<u8> {
    key.to_sec1_point(false).as_bytes()[1..].to_vec()
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
