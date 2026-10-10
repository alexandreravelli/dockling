# Changelog

## Unreleased

- Drag a pinned icon along the dock to reorder pinned apps; the new order is saved to the config file.

## 0.1.0 - 2026-10-10

First release.

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
