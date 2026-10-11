# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.1](https://github.com/alexandreravelli/dockling/compare/v0.3.0...v0.3.1) - 2026-10-11

### Documentation

- *(readme)* install with APT from Agent Center's repository

## [0.3.0](https://github.com/alexandreravelli/dockling/compare/v0.2.1...v0.3.0) - 2026-10-11

### Added

- `dockling --check-config [FILE]` lists the problems of the configuration file as `file:line: message`, and exits with 1 when there is one.
- Dock Settings shows the problems of the configuration file in a warning bar when it opens.
- The crate is packaged for crates.io (`cargo install dockling`), with its configuration library documented for docs.rs.

### Changed

- A typo in `config.toml` no longer resets every setting: an unknown key (with a suggestion), a wrong value or a value out of range is skipped or brought back into range, with its line number, and every other setting still applies.
- Text is drawn with skrifa instead of the unmaintained ttf-parser (RUSTSEC-2026-0192). The dock grows from about 2.0 to 2.2 MB.
- Clearer error messages, for example when there is no X server to connect to.
- Each release runs every check (tests on Xvfb, Clippy, MSRV, cargo-deny) before anything is published, and also carries `dockling_amd64.deb` under a name that does not change.
- Library: `config::load` returns the configuration with its warnings (`Result<Loaded, ConfigError>`) instead of printing them; `config::parse` is new; `save` and `save_pinned` return `ConfigError`.

### Fixed

- The dock no longer crashes at startup on a color with a non-ASCII character, such as a non-breaking space.
- Pinned apps are saved atomically, so the settings window never reads a half-written file.

## [0.2.1](https://github.com/alexandreravelli/dockling/compare/v0.2.0...v0.2.1) - 2026-10-10

### Changed

- New macOS-style app icon, also used by the dock's autostart entry.

## [0.2.0](https://github.com/alexandreravelli/dockling/compare/v0.1.0...v0.2.0) - 2026-10-10

### Added

- New **Dock Settings** window (`dockling-settings`) to change the appearance, behavior, pinned apps and colors without editing the config file. Changes apply to the dock right away. It opens from the dock's right-click menu, the applications menu and the XFCE Settings Manager.
- Drag a pinned icon along the dock to reorder pinned apps; the new order is saved to the config file.
- Right-clicking an empty part of the dock opens the dock menu (Dock Settings, Reload Dock), and app menus gain a Dock Settings entry.

### Changed

- When no font file is configured, the font is found through fontconfig (`fc-match`).
- Building from source needs Rust 1.92 and `libgtk-3-dev` (only for the settings window; `--no-default-features --features svg` builds the dock alone).

## [0.1.0](https://github.com/alexandreravelli/dockling/releases/tag/v0.1.0) - 2026-10-10

### Added

- Ubuntu-style dock for X11 desktops, written in Rust. Built for XFCE (xfwm4) on Debian 13, works with EWMH window managers.
- Pinned launchers, running applications with one indicator dot per window, focused-app highlight.
- "Show Applications" button (runs `xfce4-appfinder` by default).
- Left click to launch, focus, minimize or cycle windows; middle click for a new window; scroll to switch windows.
- Right-click menu: New Window, Pin to Dock / Unpin from Dock, Quit.
- Tooltips with the application name.
- Left, right, bottom or top position; full-length or centered; optional autohide.
- Reserves screen space and fits between existing XFCE panels.
- Transparency and rounded corners with a compositor, solid colors without one.
- Icons from the icon theme (PNG and SVG), with the window's own icon as a fallback.
- Commented TOML configuration, live reload with `dockling --reload`, system-wide template in `/etc/xdg/dockling/config.toml`.
- About 2 MB binary and about 5 MB of RAM.
