# Changelog

All notable changes to Phantom Switch. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/). Each release's notes on GitHub are its section here.

## [1.1.0] - 2026-11-10

### Added

- Phantom Switch updates itself: when a new version is out, **Update to version X.Y.Z** leads the menu. One click downloads it, installs it only if it is signed with Phantom Switch's key, puts it in place of the old `.exe` (so **Start with Windows** keeps starting it) and restarts into it, with a notice saying which version you now have. Nothing is downloaded until you choose to. If it cannot update itself there, in a folder it cannot write to, it opens the release's page to download it by hand.
- Each release is published with its signature, `PhantomSwitch.exe.sig` (ECDSA P-256), which the app checks with Windows' own cryptography: no new dependency.

### Note

From 1.0.0, this update is still by hand: download `PhantomSwitch.exe` and put it in place of the old one. From 1.1.0 on, **Update to version X.Y.Z** does it.

## [1.0.0] - 2026-11-10

The first stable release: switching from the app is now verified on a real ATK Phantom Ultimate
(BITMOUSE firmware), wired and through its 8K receiver, like every read before it.

### Added

- Phantom Switch tells you when a new version is out: **Download version X.Y.Z** leads the menu and opens its release. Once a day at most, it asks GitHub for the latest release's number, with a single request on a thread that ends with it; nothing else is sent, and nothing is downloaded or installed on its own. Only stable releases are offered.

## [0.3.1] - 2026-11-10

### Changed

- A new icon for the app, in the ATK Phantom's own look: a fierce ghost shaped like the mouse, its bone-white shell in two panels with a gold seam and wheel, angular eyes and a hem of spikes, on a warm dark tile with a thin gold edge. The tray icon still shows the active profile's number.

### Fixed

- **Start with Windows** is checked only when Windows starts this very copy of the app: after moving `PhantomSwitch.exe`, it no longer shows as on while Windows looks for the old one.
- The low-battery warnings come below 15% and below 5%, as described, rather than at 15% and 5%.

### Note

This is a pre-release: every read is verified on a real ATK Phantom Ultimate (BITMOUSE firmware), wired and through its 8K receiver, but switching from the app is not yet.

## [0.3.0] - 2026-11-10

### Added

- An on-screen notice when you switch profiles: the profile's badge, its name, and its polling rate, DPI and sensor mode, at the bottom center of the screen you are on, like Windows' volume flyout. It fades in and out (instantly when Windows' animations are off), follows the light or dark theme, never takes the focus, and shows over games in borderless windows. Turn it off from the menu with **Show a notice when switching**.
- Each profile in the menu carries its colored badge, and the active one is shown in bold.
- An English interface: the app speaks Spanish when Windows does, and English otherwise.

### Changed

- Text is drawn with ClearType's natural spacing, lighter and evenly spaced at every scale.
- The repository, its documentation and its release notes are now in English; each release's notes come from this changelog.

### Note

This is a pre-release: every read is verified on a real ATK Phantom Ultimate (BITMOUSE firmware), wired and through its 8K receiver, but switching from the app is not yet.

## [0.2.0] - 2026-11-10

### Added

- A notification when a switch could not be made.

### Changed

- The app is now **Phantom Switch** (`PhantomSwitch.exe`), with its own icon and version information.
- Tray icons are sharp at any display scaling, and menus turn dark when Windows is dark.
- The tray icon works from the keyboard: <kbd>Win</kbd> + <kbd>B</kbd>, <kbd>Enter</kbd> to switch and the menu key for the menu.

### Note

This is a pre-release: every read is verified on a real ATK Phantom Ultimate (BITMOUSE firmware), wired and through its 8K receiver, but switching from the app is not yet.

## [0.1.0] - 2026-11-10

### Added

- A tray icon with the number of the active profile (1 to 4); click it to go back to the previous one.
- A menu with the connection (wireless or wired), the battery, and each profile's polling rate, DPI and sensor mode.
- Low-battery warnings below 15% and 5% (through the receiver, checked every 15 minutes).
- **Open ATK HUB** and **Start with Windows**.

### Note

This is a pre-release: every read is verified on a real ATK Phantom Ultimate (BITMOUSE firmware), wired and through its 8K receiver, but switching from the app is not yet.

[1.1.0]: https://github.com/sbtcontreras/atk-phantom-profile-switcher/releases/tag/v1.1.0
[1.0.0]: https://github.com/sbtcontreras/atk-phantom-profile-switcher/releases/tag/v1.0.0
[0.3.1]: https://github.com/sbtcontreras/atk-phantom-profile-switcher/releases/tag/v0.3.1
[0.3.0]: https://github.com/sbtcontreras/atk-phantom-profile-switcher/releases/tag/v0.3.0
[0.2.0]: https://github.com/sbtcontreras/atk-phantom-profile-switcher/releases/tag/v0.2.0
[0.1.0]: https://github.com/sbtcontreras/atk-phantom-profile-switcher/releases/tag/v0.1.0
