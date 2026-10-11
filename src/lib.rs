//! The parts of [Dockling](https://github.com/alexandreravelli/dockling) that the dock and its
//! settings window share: the configuration file and the `.desktop` application database.
//!
//! - [`config`] reads, checks and writes `~/.config/dockling/config.toml`.
//! - [`desktop`] finds the installed applications and matches windows to them.
//!
//! The dock itself (X11 window, drawing, input) is the `dockling` program, built from the same
//! package.

pub mod config;
pub mod desktop;
