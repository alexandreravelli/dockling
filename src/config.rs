//! The configuration file, `~/.config/dockling/config.toml`: reading it, checking it and
//! writing it back without losing its comments.
//!
//! ```
//! use dockling::config::{self, Position};
//!
//! let loaded = config::parse("position = \"bottom\"\n[colors]\nindicator = \"#3584e4\"\n");
//! assert!(loaded.warnings.is_empty());
//! assert_eq!(loaded.config.position, Position::Bottom);
//! assert_eq!(loaded.config.colors.indicator.to_hex(), "#3584e4");
//! // Everything else keeps its default value.
//! assert_eq!(loaded.config.icon_size, 48);
//! ```

use serde::Deserialize;
use serde::de::DeserializeOwned;
use std::cmp::Ordering;
use std::fmt::{self, Write as _};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The documented default configuration, written on first start.
pub const DEFAULT_CONFIG: &str = include_str!("../config/default.toml");

/// The screen edge the dock sits on.
#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Position {
    /// Along the left edge, as on Ubuntu.
    Left,
    /// Along the right edge.
    Right,
    /// Along the bottom edge.
    Bottom,
    /// Along the top edge.
    Top,
}

impl Position {
    /// Every position, in the order the settings window lists them.
    pub const ALL: [Position; 4] = [Position::Left, Position::Right, Position::Bottom, Position::Top];

    /// True for the bottom and top edges, where icons are in a row.
    ///
    /// ```
    /// use dockling::config::Position;
    ///
    /// assert!(Position::Bottom.horizontal());
    /// assert!(!Position::Left.horizontal());
    /// ```
    #[must_use]
    pub fn horizontal(self) -> bool {
        matches!(self, Position::Bottom | Position::Top)
    }

    /// The value used in the config file.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Position::Left => "left",
            Position::Right => "right",
            Position::Bottom => "bottom",
            Position::Top => "top",
        }
    }
}

/// What a left click does on a running application.
#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "kebab-case")]
pub enum ClickAction {
    /// Focus it; when it is focused already, minimize it (one window) or switch to its next
    /// window (several windows).
    MinimizeOrCycle,
    /// Focus it; when it is focused already, switch to its next window.
    Cycle,
}

impl ClickAction {
    /// The value used in the config file.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ClickAction::MinimizeOrCycle => "minimize-or-cycle",
            ClickAction::Cycle => "cycle",
        }
    }
}

/// A straight (not premultiplied) RGBA color, written `"#RRGGBB"` or `"#RRGGBBAA"` in the file.
///
/// ```
/// use dockling::config::Color;
///
/// let color = Color::try_from("#ff800080".to_string())?;
/// assert_eq!((color.r, color.g, color.b, color.a), (255, 128, 0, 128));
/// assert_eq!(color.to_hex(), "#ff800080");
/// assert_eq!(color.opaque().to_hex(), "#ff8000");
/// assert!(Color::try_from("orange".to_string()).is_err());
/// # Ok::<(), String>(())
/// ```
#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(try_from = "String")]
pub struct Color {
    /// Red.
    pub r: u8,
    /// Green.
    pub g: u8,
    /// Blue.
    pub b: u8,
    /// Opacity: 0 is transparent, 255 opaque.
    pub a: u8,
}

impl TryFrom<String> for Color {
    type Error = String;

    fn try_from(s: String) -> Result<Self, String> {
        let hex = s.strip_prefix('#').unwrap_or(&s);
        // Byte slicing below needs ASCII: "#e95\u{a0}0" is 6 bytes long but cut inside a char.
        if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(format!("invalid color {s:?}, expected \"#RRGGBB\" or \"#RRGGBBAA\""));
        }
        let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16);
        let parsed = match hex.len() {
            6 => (byte(0), byte(2), byte(4), Ok(255)),
            8 => (byte(0), byte(2), byte(4), byte(6)),
            _ => {
                return Err(format!("invalid color {s:?}, expected \"#RRGGBB\" or \"#RRGGBBAA\""));
            }
        };
        match parsed {
            (Ok(r), Ok(g), Ok(b), Ok(a)) => Ok(Color { r, g, b, a }),
            _ => Err(format!("invalid color {s:?}, expected \"#RRGGBB\" or \"#RRGGBBAA\"")),
        }
    }
}

impl Color {
    const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Color { r, g, b, a }
    }

    /// The same color for tiny-skia.
    #[must_use]
    pub fn skia(self) -> tiny_skia::Color {
        tiny_skia::Color::from_rgba8(self.r, self.g, self.b, self.a)
    }

    /// The same color, fully opaque (for drawing without a compositor).
    #[must_use]
    pub fn opaque(self) -> Self {
        Color { a: 255, ..self }
    }

    /// "#rrggbb", or "#rrggbbaa" when not fully opaque.
    #[must_use]
    pub fn to_hex(self) -> String {
        let rgb = format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b);
        if self.a == 255 { rgb } else { format!("{rgb}{:02x}", self.a) }
    }
}

/// The `[colors]` table.
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Colors {
    /// The dock's background.
    pub background: Color,
    /// The highlight behind the icon under the pointer.
    pub hover: Color,
    /// The highlight behind the focused application.
    pub active: Color,
    /// The dots that count an application's windows.
    pub indicator: Color,
    /// The dots of an application that asks for attention.
    pub attention: Color,
    /// Tooltip and menu text.
    pub text: Color,
    /// The tooltips' background.
    pub tooltip_background: Color,
    /// The menus' background.
    pub menu_background: Color,
    /// The highlight of the menu entry under the pointer.
    pub menu_hover: Color,
    /// The dots of the "Show Applications" button.
    pub apps_button: Color,
}

impl Default for Colors {
    fn default() -> Self {
        Colors {
            background: Color::rgba(0x1d, 0x1d, 0x1d, 0xe6),
            hover: Color::rgba(0xff, 0xff, 0xff, 0x26),
            active: Color::rgba(0xff, 0xff, 0xff, 0x40),
            indicator: Color::rgba(0xe9, 0x54, 0x20, 0xff),
            attention: Color::rgba(0xf6, 0xd3, 0x2d, 0xff),
            text: Color::rgba(0xff, 0xff, 0xff, 0xff),
            tooltip_background: Color::rgba(0x2b, 0x2b, 0x2b, 0xf2),
            menu_background: Color::rgba(0x30, 0x30, 0x30, 0xf5),
            menu_hover: Color::rgba(0xe9, 0x54, 0x20, 0xff),
            apps_button: Color::rgba(0xff, 0xff, 0xff, 0xff),
        }
    }
}

/// Every setting of the dock. [`Config::default`] gives the values of the default file.
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(default, deny_unknown_fields)]
#[expect(clippy::struct_excessive_bools, reason = "each bool is an independent on/off setting of the file")]
pub struct Config {
    /// The screen edge the dock sits on.
    pub position: Position,
    /// Icon size in pixels (16 to 256); icons shrink when the dock is full.
    pub icon_size: u32,
    /// Space around each icon inside its cell, in pixels (up to 64).
    pub icon_padding: u32,
    /// Space between cells, in pixels (up to 64).
    pub spacing: u32,
    /// Space along the dock's ends, in pixels (up to 64).
    pub margin: u32,
    /// Span the whole screen edge; else the dock is as long as its content, centered.
    pub extend: bool,
    /// Corner radius of the highlights, and of the dock when it does not span the edge (0 to 128).
    pub corner_radius: f32,
    /// Reserve screen space so maximized windows stop at the dock.
    pub reserve_space: bool,
    /// Hide the dock when the pointer leaves it (no space is reserved then).
    pub autohide: bool,
    /// How long the dock waits before hiding, in milliseconds.
    pub autohide_delay_ms: u64,
    /// Show the running applications that are not pinned.
    pub show_running: bool,
    /// Show the "Show Applications" button.
    pub show_apps_button: bool,
    /// Put that button first instead of last.
    pub apps_button_at_start: bool,
    /// The shell command the "Show Applications" button runs.
    pub apps_command: String,
    /// What a left click does on a running application.
    pub click_action: ClickAction,
    /// Icon theme name; empty for the desktop's theme.
    pub icon_theme: String,
    /// Path of the tooltip and menu font; empty to let fontconfig choose.
    pub font: String,
    /// Font size in pixels (6 to 72).
    pub font_size: f32,
    /// Show the application's name when the pointer rests on its icon.
    pub tooltips: bool,
    /// Terminal for applications that run in one (`Terminal=true`).
    pub terminal: String,
    /// Pinned applications, in order: desktop file ids, short names or `.desktop` paths.
    pub pinned: Vec<String>,
    /// The colors.
    pub colors: Colors,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            position: Position::Left,
            icon_size: 48,
            icon_padding: 6,
            spacing: 4,
            margin: 4,
            extend: true,
            corner_radius: 10.0,
            reserve_space: true,
            autohide: false,
            autohide_delay_ms: 500,
            show_running: true,
            show_apps_button: true,
            apps_button_at_start: false,
            apps_command: "xfce4-appfinder".into(),
            click_action: ClickAction::MinimizeOrCycle,
            icon_theme: String::new(),
            font: String::new(),
            font_size: 13.0,
            tooltips: true,
            terminal: "x-terminal-emulator".into(),
            pinned: ["firefox-esr", "xfce4-terminal", "thunar", "mousepad"].map(String::from).to_vec(),
            colors: Colors::default(),
        }
    }
}

impl Config {
    /// Brings values that would make the dock unusable back into range, with a warning.
    fn sanitize(mut self, text: &str, warnings: &mut Vec<Warning>) -> Self {
        let fixes = [
            ("icon_size", bound(&mut self.icon_size, 16, 256)),
            ("icon_padding", bound(&mut self.icon_padding, 0, 64)),
            ("spacing", bound(&mut self.spacing, 0, 64)),
            ("margin", bound(&mut self.margin, 0, 64)),
            ("corner_radius", bound(&mut self.corner_radius, 0.0, 128.0)),
            ("font_size", bound(&mut self.font_size, 6.0, 72.0)),
        ];
        for (key, message) in fixes {
            if let Some(message) = message {
                warnings.push(Warning { line: key_line(text, None, key), message: format!("{key} {message}") });
            }
        }
        self
    }
}

/// Brings `value` into `min..=max`, and says what was wrong; None when it was in range.
fn bound<T: PartialOrd + Copy + fmt::Display>(value: &mut T, min: T, max: T) -> Option<String> {
    let fixed = match (*value).partial_cmp(&min) {
        // Below the range, or not a number.
        Some(Ordering::Less) | None => min,
        _ if *value > max => max,
        _ => return None,
    };
    let message = format!("= {value} is out of range ({min} to {max}); using {fixed}");
    *value = fixed;
    Some(message)
}

/// The top-level settings, in the order of the default file.
pub const KEYS: [&str; 22] = [
    "position",
    "icon_size",
    "icon_padding",
    "spacing",
    "margin",
    "extend",
    "corner_radius",
    "reserve_space",
    "autohide",
    "autohide_delay_ms",
    "show_running",
    "show_apps_button",
    "apps_button_at_start",
    "apps_command",
    "click_action",
    "icon_theme",
    "font",
    "font_size",
    "tooltips",
    "terminal",
    "pinned",
    "colors",
];

/// The settings of the `[colors]` table.
pub const COLOR_KEYS: [&str; 10] = [
    "background",
    "hover",
    "active",
    "indicator",
    "attention",
    "text",
    "tooltip_background",
    "menu_background",
    "menu_hover",
    "apps_button",
];

/// A problem in the configuration file. The setting is skipped (or brought back into range);
/// the other settings still apply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    /// The line of the problem in the file, starting at 1, when it is known.
    pub line: Option<usize>,
    /// What is wrong, and what Dockling does about it.
    pub message: String,
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(line) => write!(f, "line {line}: {}", self.message),
            None => f.write_str(&self.message),
        }
    }
}

/// Why the configuration file could not be read or written.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ConfigError {
    /// The file exists but cannot be read.
    #[error("cannot read {}: {error}", path.display())]
    Read {
        /// The configuration file.
        path: PathBuf,
        /// Why it cannot be read.
        error: io::Error,
    },
    /// The file cannot be written.
    #[error("cannot write {}: {error}", path.display())]
    Write {
        /// The configuration file.
        path: PathBuf,
        /// Why it cannot be written.
        error: io::Error,
    },
}

/// A configuration, and the problems found in its file.
#[derive(Debug, Clone, PartialEq)]
pub struct Loaded {
    /// The settings: the file's valid ones, the defaults for the others.
    pub config: Config,
    /// Every key or value of the file that was skipped or changed, in reading order.
    pub warnings: Vec<Warning>,
}

/// The user's configuration file: `$XDG_CONFIG_HOME/dockling/config.toml`, by default
/// `~/.config/dockling/config.toml`.
pub fn config_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map_or_else(|| home_dir().join(".config"), PathBuf::from);
    base.join("dockling").join("config.toml")
}

/// The first `dockling/config.toml` found in $`XDG_CONFIG_DIRS` (default /etc/xdg).
fn system_config() -> Option<String> {
    let dirs = std::env::var("XDG_CONFIG_DIRS").ok().filter(|v| !v.is_empty()).unwrap_or_else(|| "/etc/xdg".into());
    dirs.split(':')
        .filter(|d| !d.is_empty())
        .find_map(|d| fs::read_to_string(Path::new(d).join("dockling/config.toml")).ok())
}

/// The user's home directory, from `HOME` (`/` when it is not set).
pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from)
}

/// Loads the configuration at `path`. When the file does not exist it is created from the
/// system-wide template (`/etc/xdg/dockling/config.toml`) or the defaults, so new users get a
/// commented file to edit. Problems in the file are returned as [`Warning`]s, see [`parse`].
///
/// # Errors
///
/// Returns [`ConfigError::Read`] when the file exists but cannot be read.
pub fn load(path: &Path) -> Result<Loaded, ConfigError> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(parse(&text)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            // New users start from the system-wide template when an admin provides one.
            let text = system_config().unwrap_or_else(|| DEFAULT_CONFIG.to_string());
            let mut loaded = parse(&text);
            let created = path.parent().map_or(Ok(()), fs::create_dir_all).and_then(|()| fs::write(path, &text));
            if let Err(error) = created {
                let error = ConfigError::Write { path: path.to_path_buf(), error };
                loaded.warnings.push(Warning { line: None, message: error.to_string() });
            }
            Ok(loaded)
        }
        Err(error) => Err(ConfigError::Read { path: path.to_path_buf(), error }),
    }
}

/// Reads a configuration file's text. Every valid setting applies. An unknown key, a value of
/// the wrong type or out of range is skipped (or brought back into range) and reported, so one
/// typo never resets the whole file. A file that is not valid TOML gives the defaults.
///
/// ```
/// use dockling::config::{self, Position};
///
/// let loaded = config::parse("position = \"bottom\"\ntooltip = false\nicon_size = \"big\"\n");
/// assert_eq!(loaded.config.position, Position::Bottom);
/// assert_eq!(loaded.config.icon_size, 48, "the default");
/// let warnings: Vec<String> = loaded.warnings.iter().map(ToString::to_string).collect();
/// assert_eq!(warnings[0], "line 2: unknown setting \"tooltip\". Did you mean \"tooltips\"?");
/// assert!(warnings[1].starts_with("line 3: invalid value for \"icon_size\""));
/// ```
#[must_use]
pub fn parse(text: &str) -> Loaded {
    let mut warnings = Vec::new();
    let table: toml::Table = match text.parse() {
        Ok(table) => table,
        Err(e) => {
            let line = e.span().map(|span| line_of(text, span.start));
            warnings.push(Warning { line, message: format!("{}; using the default settings", e.message().trim()) });
            return Loaded { config: Config::default(), warnings };
        }
    };
    let mut accepted = toml::Table::new();
    for (key, value) in table {
        let line = key_line(text, None, &key);
        match (key.as_str(), value) {
            ("colors", toml::Value::Table(colors)) => {
                let mut ok = toml::Table::new();
                for (name, value) in colors {
                    let line = key_line(text, Some("colors"), &name);
                    accept::<Colors>(&mut ok, &name, value, &COLOR_KEYS, "colors.", line, &mut warnings);
                }
                accepted.insert(key, toml::Value::Table(ok));
            }
            ("colors", _) => {
                warnings.push(Warning { line, message: "colors must be a table: [colors]".into() });
            }
            (_, value) => accept::<Config>(&mut accepted, &key, value, &KEYS, "", line, &mut warnings),
        }
    }
    // Each accepted value deserialized on its own, so together they do too.
    let config: Config = toml::Value::Table(accepted).try_into().unwrap_or_default();
    let config = config.sanitize(text, &mut warnings);
    // Tables are read in key order: report in file order.
    warnings.sort_by_key(|w| w.line.unwrap_or(usize::MAX));
    Loaded { config, warnings }
}

/// Adds `key = value` to `accepted` when `T` takes it, else reports why it is skipped.
fn accept<T: DeserializeOwned>(
    accepted: &mut toml::Table,
    key: &str,
    value: toml::Value,
    keys: &[&str],
    prefix: &str,
    line: Option<usize>,
    warnings: &mut Vec<Warning>,
) {
    if !keys.contains(&key) {
        let hint = match similar(key, keys) {
            Some(close) => format!(". Did you mean \"{prefix}{close}\"?"),
            None => String::new(),
        };
        warnings.push(Warning { line, message: format!("unknown setting \"{prefix}{key}\"{hint}") });
        return;
    }
    let mut trial = accepted.clone();
    trial.insert(key.to_string(), value);
    match toml::Value::Table(trial.clone()).try_into::<T>() {
        Ok(_) => *accepted = trial,
        Err(e) => warnings.push(Warning {
            line,
            message: format!("invalid value for \"{prefix}{key}\": {}; using the default", e.message().trim()),
        }),
    }
}

/// The closest of `keys` to `key`, when it is one or two typos away.
fn similar<'a>(key: &str, keys: &[&'a str]) -> Option<&'a str> {
    keys.iter().map(|k| (distance(key, k), *k)).filter(|(d, _)| *d <= 2).min().map(|(_, k)| k)
}

/// Edits (insertion, deletion, substitution, swap of two neighbours) from `a` to `b`.
fn distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut rows = vec![(0..=b.len()).collect::<Vec<usize>>()];
    for i in 1..=a.len() {
        let mut row = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            row[j] = (rows[i - 1][j] + 1).min(row[j - 1] + 1).min(rows[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                row[j] = row[j].min(rows[i - 2][j - 2] + 1);
            }
        }
        rows.push(row);
    }
    rows[a.len()][b.len()]
}

/// The line (from 1) of byte `offset` in `text`.
fn line_of(text: &str, offset: usize) -> usize {
    text.as_bytes().iter().take(offset).filter(|&&b| b == b'\n').count() + 1
}

/// The line (from 1) where `key` is set, at the top level (`table` = None) or in `[table]`.
fn key_line(text: &str, table: Option<&str>, key: &str) -> Option<usize> {
    let mut current = None;
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            current = Some(name.trim());
        } else if current == table && line.strip_prefix(key).is_some_and(|rest| rest.trim_start().starts_with('=')) {
            return Some(i + 1);
        }
    }
    None
}

/// Writes every setting into the file, keeping its comments and layout
/// (missing keys are added). Used by the settings window.
///
/// # Errors
///
/// Returns [`ConfigError::Write`] when the file cannot be written.
pub fn save(path: &Path, cfg: &Config) -> Result<(), ConfigError> {
    let mut text = fs::read_to_string(path).unwrap_or_else(|_| DEFAULT_CONFIG.to_string());
    let str_value = |s: &str| {
        let mut out = String::from("\"");
        for c in s.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\t' => out.push_str("\\t"),
                // Writing to a String cannot fail.
                c if c.is_control() => _ = write!(out, "\\u{:04X}", c as u32),
                c => out.push(c),
            }
        }
        out.push('"');
        out
    };
    let float = |f: f32| if f.fract() == 0.0 { format!("{f:.1}") } else { format!("{f}") };
    let top: Vec<(&str, String)> = vec![
        ("position", str_value(cfg.position.as_str())),
        ("icon_size", cfg.icon_size.to_string()),
        ("icon_padding", cfg.icon_padding.to_string()),
        ("spacing", cfg.spacing.to_string()),
        ("margin", cfg.margin.to_string()),
        ("extend", cfg.extend.to_string()),
        ("corner_radius", float(cfg.corner_radius)),
        ("reserve_space", cfg.reserve_space.to_string()),
        ("autohide", cfg.autohide.to_string()),
        ("autohide_delay_ms", cfg.autohide_delay_ms.to_string()),
        ("show_running", cfg.show_running.to_string()),
        ("show_apps_button", cfg.show_apps_button.to_string()),
        ("apps_button_at_start", cfg.apps_button_at_start.to_string()),
        ("apps_command", str_value(&cfg.apps_command)),
        ("click_action", str_value(cfg.click_action.as_str())),
        ("icon_theme", str_value(&cfg.icon_theme)),
        ("font", str_value(&cfg.font)),
        ("font_size", float(cfg.font_size)),
        ("tooltips", cfg.tooltips.to_string()),
        ("terminal", str_value(&cfg.terminal)),
    ];
    for (key, value) in top {
        text = set_value(&text, None, key, &value);
    }
    let c = &cfg.colors;
    let colors = [
        ("background", c.background),
        ("hover", c.hover),
        ("active", c.active),
        ("indicator", c.indicator),
        ("attention", c.attention),
        ("text", c.text),
        ("tooltip_background", c.tooltip_background),
        ("menu_background", c.menu_background),
        ("menu_hover", c.menu_hover),
        ("apps_button", c.apps_button),
    ];
    for (key, color) in colors {
        text = set_value(&text, Some("colors"), key, &str_value(&color.to_hex()));
    }
    text = replace_pinned(&text, &cfg.pinned);
    write_atomically(path, &text)
}

/// Writes then renames, so the dock and the settings window never read a half-written file.
fn write_atomically(path: &Path, text: &str) -> Result<(), ConfigError> {
    let tmp = path.with_extension("toml.tmp");
    fs::write(&tmp, text)
        .and_then(|()| fs::rename(tmp, path))
        .map_err(|error| ConfigError::Write { path: path.to_path_buf(), error })
}

/// Sets a single-line `key = value` in the top level (`table` = None) or in `[table]`.
fn set_value(text: &str, table: Option<&str>, key: &str, value: &str) -> String {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut current: Option<&str> = None;
    let mut table_header = None;
    let mut first_table = None;
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if let Some(name) = trimmed.strip_prefix('[').and_then(|t| t.strip_suffix(']')) {
            current = Some(name.trim());
            first_table.get_or_insert(i);
            if current == table {
                table_header = Some(i);
            }
            continue;
        }
        let is_key = trimmed.strip_prefix(key).is_some_and(|rest| rest.trim_start().starts_with('='));
        if current == table && is_key {
            let indent = &line[..line.len() - line.trim_start().len()];
            let mut out: String = lines[..i].concat();
            _ = writeln!(out, "{indent}{key} = {value}");
            out.push_str(&lines[i + 1..].concat());
            return out;
        }
    }
    let new_line = format!("{key} = {value}\n");
    let insert = |at: usize| format!("{}{new_line}{}", lines[..at].concat(), lines[at..].concat());
    match table {
        None => insert(first_table.unwrap_or(lines.len())),
        Some(_) if table_header.is_some() => insert(table_header.unwrap_or(0) + 1),
        Some(name) => {
            let mut out = text.to_string();
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            _ = write!(out, "\n[{name}]\n{new_line}");
            out
        }
    }
}

/// Rewrites only the `pinned = [...]` entry of the file, keeping the user's comments.
///
/// # Errors
///
/// Returns [`ConfigError::Write`] when the file cannot be written.
pub fn save_pinned(path: &Path, pinned: &[String]) -> Result<(), ConfigError> {
    let text = fs::read_to_string(path).unwrap_or_else(|_| DEFAULT_CONFIG.to_string());
    write_atomically(path, &replace_pinned(&text, pinned))
}

fn replace_pinned(text: &str, pinned: &[String]) -> String {
    let mut block = String::from("pinned = [\n");
    for p in pinned {
        let escaped = p.replace('\\', "\\\\").replace('"', "\\\"");
        _ = writeln!(block, "    \"{escaped}\",");
    }
    block.push(']');

    // Find a top-level `pinned = [ ... ]` (before the first [table]).
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') {
            break;
        }
        if let Some(rest) = trimmed.strip_prefix("pinned")
            && rest.trim_start().starts_with('=')
        {
            let start = offset;
            if let Some(close) = text[start..].find(']') {
                let end = start + close + 1;
                return format!("{}{}{}", &text[..start], block, &text[end..]);
            }
        }
        offset += line.len();
    }
    // Not found: insert before the first table so it stays a top-level key.
    let insert_at = text
        .split_inclusive('\n')
        .scan(0, |pos, line| {
            let here = *pos;
            *pos += line.len();
            Some((here, line))
        })
        .find(|(_, line)| line.trim_start().starts_with('['))
        .map_or(text.len(), |(pos, _)| pos);
    format!("{}{}\n\n{}", &text[..insert_at], block, &text[insert_at..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_file_matches_defaults() {
        let parsed: Config = toml::from_str(DEFAULT_CONFIG).unwrap();
        assert_eq!(parsed, Config::default());
        assert_eq!(parse(DEFAULT_CONFIG), Loaded { config: Config::default(), warnings: Vec::new() });
    }

    #[test]
    fn every_setting_has_a_key() {
        let table: toml::Table = DEFAULT_CONFIG.parse().unwrap();
        let keys: Vec<&str> = table.keys().map(String::as_str).collect();
        let mut expected = KEYS.to_vec();
        expected.sort_unstable();
        assert_eq!(keys, expected);
        let colors: Vec<&str> = table["colors"].as_table().unwrap().keys().map(String::as_str).collect();
        let mut expected = COLOR_KEYS.to_vec();
        expected.sort_unstable();
        assert_eq!(colors, expected);
    }

    fn messages(text: &str) -> (Config, Vec<String>) {
        let loaded = parse(text);
        (loaded.config, loaded.warnings.iter().map(ToString::to_string).collect())
    }

    #[test]
    fn one_bad_setting_keeps_the_others() {
        let text = "position = \"bottom\"\nicon_sise = 64\nautohide = \"yes\"\nfont_size = 15.0\n\
                    click_action = \"open\"\n\n[colors]\ntext = \"#000000\"\nhover = \"red\"\nshadow = \"#000\"\n";
        let (cfg, warnings) = messages(text);
        assert_eq!((cfg.position, cfg.font_size, cfg.colors.text), (Position::Bottom, 15.0, Color::rgba(0, 0, 0, 255)));
        assert_eq!((cfg.autohide, cfg.click_action), (false, ClickAction::MinimizeOrCycle), "defaults");
        assert_eq!(cfg.colors.hover, Colors::default().hover);
        assert_eq!(warnings.len(), 5, "{warnings:#?}");
        assert_eq!(warnings[0], "line 2: unknown setting \"icon_sise\". Did you mean \"icon_size\"?");
        assert!(warnings[1].starts_with("line 3: invalid value for \"autohide\""), "{}", warnings[1]);
        assert!(warnings[2].starts_with("line 5: invalid value for \"click_action\""), "{}", warnings[2]);
        assert!(warnings[3].starts_with("line 9: invalid value for \"colors.hover\""), "{}", warnings[3]);
        assert_eq!(warnings[4], "line 10: unknown setting \"colors.shadow\"");
    }

    #[test]
    fn values_out_of_range_are_brought_back() {
        let (cfg, warnings) = messages("icon_size = 1000\ncorner_radius = nan\nspacing = 3\n");
        assert_eq!((cfg.icon_size, cfg.corner_radius, cfg.spacing), (256, 0.0, 3));
        assert_eq!(
            warnings,
            [
                "line 1: icon_size = 1000 is out of range (16 to 256); using 256",
                "line 2: corner_radius = NaN is out of range (0 to 128); using 0",
            ]
        );
    }

    #[test]
    fn a_file_that_is_not_toml_gives_the_defaults() {
        let (cfg, warnings) = messages("icon_size = 32\nposition = \"left\n");
        assert_eq!(cfg, Config::default());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].starts_with("line 2: "), "{warnings:?}");
        assert!(warnings[0].ends_with("; using the default settings"), "{warnings:?}");
        let (_, warnings) = messages("colors = 3\n");
        assert_eq!(warnings, ["line 1: colors must be a table: [colors]"]);
    }

    #[test]
    fn keys_are_found_in_their_table() {
        let text = "# text = 1\ntext=2\n[colors]\n  text = \"#fff\"\n";
        assert_eq!(key_line(text, None, "text"), Some(2));
        assert_eq!(key_line(text, Some("colors"), "text"), Some(4));
        assert_eq!(key_line(text, None, "missing"), None);
        assert_eq!(line_of("a\nb\nc", 4), 3);
    }

    #[test]
    fn a_missing_file_is_created_from_the_defaults() {
        let dir = std::env::temp_dir().join(format!("dockling-load-{}", std::process::id()));
        let path = dir.join("sub/config.toml");
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.config, Config::default());
        assert!(path.is_file());
        fs::remove_dir_all(&dir).unwrap();
        // A directory where the file should be cannot be read.
        fs::create_dir_all(&path).unwrap();
        assert!(matches!(load(&path), Err(ConfigError::Read { .. })));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn pinned_is_replaced_in_place() {
        let new = replace_pinned(DEFAULT_CONFIG, &["a".into(), "b\"c".into()]);
        let parsed: Config = toml::from_str(&new).unwrap();
        assert_eq!(parsed.pinned, vec!["a".to_string(), "b\"c".to_string()]);
        assert!(new.contains("# Dockling configuration"));
        assert_eq!(parsed.colors, Colors::default());
    }

    #[test]
    fn save_round_trips_and_keeps_comments() {
        let dir = std::env::temp_dir().join(format!("dockling-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        fs::write(&path, "# my comment\nicon_size = 32\n[colors]\ntext = \"#000000\"\n").unwrap();
        let mut cfg = Config {
            position: Position::Bottom,
            icon_size: 64,
            apps_command: "say \"hi\" \\ there".into(),
            corner_radius: 7.5,
            pinned: vec!["a".into(), "b".into()],
            ..Config::default()
        };
        cfg.colors.background = Color::rgba(1, 2, 3, 4);
        save(&path, &cfg).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("# my comment\n"));
        let parsed: Config = toml::from_str(&text).unwrap();
        assert_eq!(parsed, cfg);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn saving_the_default_file_changes_nothing() {
        let mut text = DEFAULT_CONFIG.to_string();
        for (key, value) in [("position", "\"left\""), ("icon_size", "48"), ("corner_radius", "10.0")] {
            text = set_value(&text, None, key, value);
        }
        text = set_value(&text, Some("colors"), "indicator", "\"#e95420\"");
        assert_eq!(text, DEFAULT_CONFIG);
    }

    #[test]
    fn pinned_apps_are_saved_in_place() {
        let dir = std::env::temp_dir().join(format!("dockling-pinned-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        fs::write(&path, "# mine\npinned = [\"a\"]\nicon_size = 40\n").unwrap();
        save_pinned(&path, &["b".into(), "c".into()]).unwrap();
        let parsed: Config = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!((parsed.pinned, parsed.icon_size), (vec!["b".to_string(), "c".to_string()], 40));
        assert!(!path.with_extension("toml.tmp").exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn pinned_is_inserted_when_missing() {
        let new = replace_pinned("icon_size = 32\n[colors]\ntext = \"#000000\"\n", &["x".into()]);
        let parsed: Config = toml::from_str(&new).unwrap();
        assert_eq!(parsed.pinned, vec!["x".to_string()]);
        assert_eq!(parsed.icon_size, 32);
    }

    #[test]
    fn colors_parse() {
        assert_eq!(Color::try_from("#102030".to_string()).unwrap(), Color::rgba(16, 32, 48, 255));
        assert_eq!(Color::try_from("#10203040".to_string()).unwrap().a, 64);
        assert!(Color::try_from("#12".to_string()).is_err());
    }

    #[test]
    fn non_ascii_colors_are_rejected_without_panicking() {
        assert!(Color::try_from("#e95\u{a0}0".to_string()).is_err());
        assert!(Color::try_from("#éé0000".to_string()).is_err());
        assert!(Color::try_from("#+12345".to_string()).is_err(), "from_str_radix would take the sign");
        let text = "[colors]\ntext = \"#e95\u{a0}0\"\n";
        assert!(toml::from_str::<Config>(text).is_err());
    }
}
