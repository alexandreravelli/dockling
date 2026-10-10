<div align="center">

<img src="docs/logo.svg" alt="Dockling logo" width="112">

# Dockling

**A tiny, Ubuntu-style dock for XFCE on Debian, written in Rust.**

~2 MB binary · ~5 MB of RAM · 0% CPU when idle · no GTK, no Qt, no D-Bus

[![CI](https://github.com/alexandreravelli/dockling/actions/workflows/ci.yml/badge.svg)](https://github.com/alexandreravelli/dockling/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/alexandreravelli/dockling?color=e95420)](https://github.com/alexandreravelli/dockling/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue)](LICENSE)
[![Rust 1.88+](https://img.shields.io/badge/rust-1.88%2B-orange?logo=rust)](https://www.rust-lang.org)
[![Debian 13 · XFCE](https://img.shields.io/badge/Debian%2013-XFCE-a80030?logo=debian)](https://www.debian.org)

[Install](#-install) · [Usage](#%EF%B8%8F-usage) · [Configuration](#%EF%B8%8F-configuration) · [Changelog](CHANGELOG.md)

</div>

---

## ✨ Features

|  |  |
| --- | --- |
| 🪶 **Featherweight** | One small binary that sleeps until the X server has something to say. Perfect for VMs, thin clients and remote desktops. |
| 🐧 **Feels like Ubuntu** | Pinned apps, one dot per open window, focused-app highlight, "Show Applications" grid button, tooltips. |
| 🖱️ **Smart clicks** | Click to launch or focus, click again to minimize or cycle windows, middle-click for a new window, scroll to switch windows. |
| 📌 **Pin and arrange** | Pin or unpin from the right-click menu, drag icons to reorder them. |
| 📐 **Plays nice with XFCE** | Reserves its screen edge and fits between your existing panels. Left, right, bottom or top, full-length or centered, optional autohide. |
| 🎨 **Looks good anywhere** | Transparency and rounded corners with a compositor, crisp solid colors without one (ideal over VNC / noVNC). |
| 🖼️ **Your icon theme** | PNG and SVG icons from your theme, with the window's own icon as a fallback. |
| ⚙️ **One config file** | A commented TOML file, applied live with `dockling --reload`. Admins can preset it for every user. |

## 📦 Install

### Debian 13 package (recommended)

No Rust needed. Grab the `.deb` from the [latest release](https://github.com/alexandreravelli/dockling/releases/latest):

```sh
wget https://github.com/alexandreravelli/dockling/releases/download/v0.1.0/dockling_0.1.0-1_amd64.deb
sudo apt install ./dockling_0.1.0-1_amd64.deb
```

Log out and back in: the dock starts automatically (`/etc/xdg/autostart/dockling.desktop`).
To try it right away, run `dockling &`.

### From source

Dockling needs Rust 1.88 or newer (Debian 13 ships 1.85), so install the current toolchain with rustup:

```sh
sudo apt install -y build-essential curl git
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
. "$HOME/.cargo/env"

git clone https://github.com/alexandreravelli/dockling.git
cd dockling
cargo build --release
sudo make install   # /usr/local/bin/dockling + autostart for every user
```

Want an even smaller binary? `cargo build --release --no-default-features` drops SVG support.

### 💡 XFCE tips

- **Remove the duplicate bottom panel** that Debian's XFCE ships with:
  *right-click it → Panel → Panel Preferences → pick Panel 2 → "−"*.
- **Over VNC / noVNC**, turning the compositor off saves CPU and bandwidth:
  `xfconf-query -c xfwm4 -p /general/use_compositing -s false`

## 🖱️ Usage

| Action | What happens |
| --- | --- |
| **Left click** | Launch the app, focus it, or minimize it / switch to its next window when it is already focused |
| **Middle click** | Open a new window |
| **Scroll** | Switch between the app's windows |
| **Drag a pinned icon** | Reorder your pinned apps (the new order is saved to the config) |
| **Right click** | New Window · Pin to Dock / Unpin from Dock · Quit |
| **Right click on ⠿** | Dock Settings (opens the config file) · Reload Dock |

```text
dockling                         start the dock
dockling --replace               start, replacing a running instance
dockling --reload                apply configuration changes
dockling --quit                  stop the dock
dockling --print-default-config  show every option with its default
```

## ⚙️ Configuration

The first start creates `~/.config/dockling/config.toml` with every option documented
([see the defaults](config/default.toml)). A taste:

```toml
position = "left"                 # left, right, bottom, top
icon_size = 48
extend = true                     # span the whole edge, or false for a centered dock
autohide = false
click_action = "minimize-or-cycle"
apps_command = "xfce4-appfinder"
pinned = ["firefox-esr", "xfce4-terminal", "thunar", "mousepad"]

[colors]
background = "#1d1d1de6"          # #RRGGBB or #RRGGBBAA
indicator = "#e95420"
```

Pinned entries can be `.desktop` file names (with or without `.desktop`), short names like
`mousepad` for `org.xfce.mousepad.desktop`, or absolute paths to `.desktop` files.

### 🏢 For administrators

Place a template in `/etc/xdg/dockling/config.toml` (`sudo make install-config` installs the default one).
Every new user's configuration starts from it, so you can preset pinned apps and colors for everyone.

## 🔧 How it works

Dockling speaks X11 directly through [x11rb](https://github.com/psychon/x11rb) (pure Rust, no libxcb),
follows windows through EWMH properties such as `_NET_CLIENT_LIST` and `_NET_ACTIVE_WINDOW`,
draws with [tiny-skia](https://github.com/linebender/tiny-skia), renders SVG icons with
[resvg](https://github.com/linebender/resvg) and text with [ttf-parser](https://github.com/harfbuzz/ttf-parser).
The finished image becomes the window's background, so the X server repaints it on its own.
It works with any EWMH window manager and is tested with xfwm4.

## 🗺️ Roadmap

- [x] Drag and drop to reorder pinned apps
- [ ] Window list and previews in the menu
- [ ] Multi-monitor placement option

Ideas and bug reports are welcome in [Issues](https://github.com/alexandreravelli/dockling/issues).

## 📄 License

[MIT](LICENSE) © alexandreravelli
