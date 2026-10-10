<div align="center">

<img src="docs/logo.svg" alt="Dockling logo" width="112">

# Dockling

**A tiny, Ubuntu-style dock for XFCE on Debian, written in Rust.**

~2 MB binary · ~5 MB of RAM · 0% CPU when idle · no GTK, Qt or D-Bus in the dock

[![CI](https://github.com/alexandreravelli/dockling/actions/workflows/ci.yml/badge.svg)](https://github.com/alexandreravelli/dockling/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/alexandreravelli/dockling?color=e95420)](https://github.com/alexandreravelli/dockling/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue)](LICENSE)
[![Rust 1.92+](https://img.shields.io/badge/rust-1.92%2B-orange?logo=rust)](https://www.rust-lang.org)
[![Debian 13 · XFCE](https://img.shields.io/badge/Debian%2013-XFCE-a80030?logo=debian)](https://www.debian.org)

[Install](#-install) · [Usage](#%EF%B8%8F-usage) · [Settings](#%EF%B8%8F-settings) · [Changelog](CHANGELOG.md)

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
| 🎛️ **Settings window** | Change the position, size, behavior, pinned apps and colors from *Dock Settings*: every change shows up in the dock at once. |
| ⚙️ **One config file** | Prefer text? Everything is also in one commented TOML file, and admins can preset it for every user. |

## 📦 Install

### Debian 13 package (recommended)

No Rust needed. Grab the `.deb` from the [latest release](https://github.com/alexandreravelli/dockling/releases/latest):

```sh
wget https://github.com/alexandreravelli/dockling/releases/download/v0.2.1/dockling_0.2.1-1_amd64.deb
sudo apt install ./dockling_0.2.1-1_amd64.deb
```

Log out and back in: the dock starts automatically (`/etc/xdg/autostart/dockling.desktop`).
To try it right away, run `dockling &`.
Then right-click the dock and pick **Dock Settings** to make it yours.

### From source

Dockling needs Rust 1.92 or newer (Debian 13 ships 1.85), so install the current toolchain with rustup.
`libgtk-3-dev` is only needed for the settings window:

```sh
sudo apt install -y build-essential curl git libgtk-3-dev
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
. "$HOME/.cargo/env"

git clone https://github.com/alexandreravelli/dockling.git
cd dockling
cargo build --release
sudo make install   # dockling + dockling-settings in /usr/local, autostart for every user
```

Just the dock, without GTK? `cargo build --release --no-default-features --features svg`.
Leave out `--features svg` too for an even smaller binary without SVG icons.

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
| **Right click on an app** | Open / New Window · Pin to Dock / Unpin from Dock · Quit · Dock Settings |
| **Right click on empty space or ⠿** | Dock Settings · Reload Dock |

```text
dockling                         start the dock
dockling --replace               start, replacing a running instance
dockling --reload                apply configuration changes
dockling --quit                  stop the dock
dockling --print-default-config  show every option with its default
dockling-settings                open the settings window
```

## ⚙️ Settings

Open **Dock Settings** from the dock's right-click menu, the applications menu or the XFCE Settings Manager.

| Page | What you can change |
| --- | --- |
| **Appearance** | Screen edge, full length or centered, keeping windows clear of the dock, autohide, icon size and theme, spacing, rounded corners |
| **Behavior** | What a click does, running apps, the "Show Applications" button and its command, tooltips, font, terminal |
| **Pinned Apps** | Add apps from a searchable list, move them up or down, remove them |
| **Colors** | Background, hover and focus highlights, indicators, text, menus and tooltips, with transparency |

Every change is saved and applied to the dock right away. **Reset** brings back the defaults and keeps your pinned apps.

### 📝 The config file

Dock Settings writes to a plain text file that you can also edit by hand.
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
Dock Settings keeps the comments in the file when it saves.

## 🔧 How it works

Dockling speaks X11 directly through [x11rb](https://github.com/psychon/x11rb) (pure Rust, no libxcb),
follows windows through EWMH properties such as `_NET_CLIENT_LIST` and `_NET_ACTIVE_WINDOW`,
draws with [tiny-skia](https://github.com/linebender/tiny-skia), renders SVG icons with
[resvg](https://github.com/linebender/resvg) and text with [ttf-parser](https://github.com/harfbuzz/ttf-parser).
The finished image becomes the window's background, so the X server repaints it on its own.
Dock Settings is a separate GTK 3 program ([gtk-rs](https://gtk-rs.org)) that edits the same file and asks the
dock to reload, so the dock itself never loads a toolkit.
It works with any EWMH window manager and is tested with xfwm4.

## 🗺️ Roadmap

- [x] Drag and drop to reorder pinned apps
- [x] Settings window
- [ ] Window list and previews in the menu
- [ ] Multi-monitor placement option

Ideas and bug reports are welcome in [Issues](https://github.com/alexandreravelli/dockling/issues).

## 📄 License

[MIT](LICENSE) © alexandreravelli
