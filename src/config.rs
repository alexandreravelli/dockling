//! Configuration file handling (`~/.config/dockling/config.toml`).

use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

/// The documented default configuration, written on first start.
pub const DEFAULT_CONFIG: &str = include_str!("../config/default.toml");

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Position {
    Left,
    Right,
    Bottom,
    Top,
}

impl Position {
    pub const ALL: [Position; 4] = [Position::Left, Position::Right, Position::Bottom, Position::Top];

    pub fn horizontal(self) -> bool {
        matches!(self, Position::Bottom | Position::Top)
    }

    /// The value used in the config file.
    pub fn as_str(self) -> &'static str {
        match self {
            Position::Left => "left",
            Position::Right => "right",
            Position::Bottom => "bottom",
            Position::Top => "top",
        }
    }
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "kebab-case")]
pub enum ClickAction {
    MinimizeOrCycle,
    Cycle,
}

impl ClickAction {
    /// The value used in the config file.
    pub fn as_str(self) -> &'static str {
        match self {
            ClickAction::MinimizeOrCycle => "minimize-or-cycle",
            ClickAction::Cycle => "cycle",
        }
    }
}

/// Premultiplication happens at draw time; this is a straight RGBA color.
#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(try_from = "String")]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl TryFrom<String> for Color {
    type Error = String;

    fn try_from(s: String) -> Result<Self, String> {
        let hex = s.strip_prefix('#').unwrap_or(&s);
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

    pub fn skia(self) -> tiny_skia::Color {
        tiny_skia::Color::from_rgba8(self.r, self.g, self.b, self.a)
    }

    pub fn opaque(self) -> Self {
        Color { a: 255, ..self }
    }

    /// "#rrggbb", or "#rrggbbaa" when not fully opaque.
    pub fn to_hex(self) -> String {
        let rgb = format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b);
        if self.a == 255 { rgb } else { format!("{rgb}{:02x}", self.a) }
    }
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Colors {
    pub background: Color,
    pub hover: Color,
    pub active: Color,
    pub indicator: Color,
    pub attention: Color,
    pub text: Color,
    pub tooltip_background: Color,
    pub menu_background: Color,
    pub menu_hover: Color,
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

#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub position: Position,
    pub icon_size: u32,
    pub icon_padding: u32,
    pub spacing: u32,
    pub margin: u32,
    pub extend: bool,
    pub corner_radius: f32,
    pub reserve_space: bool,
    pub autohide: bool,
    pub autohide_delay_ms: u64,
    pub show_running: bool,
    pub show_apps_button: bool,
    pub apps_button_at_start: bool,
    pub apps_command: String,
    pub click_action: ClickAction,
    pub icon_theme: String,
    pub font: String,
    pub font_size: f32,
    pub tooltips: bool,
    pub terminal: String,
    pub pinned: Vec<String>,
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
    /// Clamp values that would make the dock unusable.
    fn sanitize(mut self) -> Self {
        self.icon_size = self.icon_size.clamp(16, 256);
        self.icon_padding = self.icon_padding.min(64);
        self.spacing = self.spacing.min(64);
        self.margin = self.margin.min(64);
        self.corner_radius = self.corner_radius.clamp(0.0, 128.0);
        self.font_size = self.font_size.clamp(6.0, 72.0);
        self
    }
}

pub fn config_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".config"));
    base.join("dockling").join("config.toml")
}

/// The first `dockling/config.toml` found in $XDG_CONFIG_DIRS (default /etc/xdg).
fn system_config() -> Option<String> {
    let dirs = std::env::var("XDG_CONFIG_DIRS").ok().filter(|v| !v.is_empty()).unwrap_or_else(|| "/etc/xdg".into());
    dirs.split(':')
        .filter(|d| !d.is_empty())
        .find_map(|d| fs::read_to_string(Path::new(d).join("dockling/config.toml")).ok())
}

pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

/// Loads the configuration, creating the default file if it does not exist.
/// A broken file is reported on stderr and the defaults are used instead.
pub fn load(path: &Path) -> Config {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if let Some(dir) = path.parent() {
                let _ = fs::create_dir_all(dir);
            }
            // New users start from the system-wide template when an admin provides one.
            let text = system_config().unwrap_or_else(|| DEFAULT_CONFIG.to_string());
            if let Err(e) = fs::write(path, &text) {
                eprintln!("dockling: cannot write {}: {e}", path.display());
            }
            text
        }
        Err(e) => {
            eprintln!("dockling: cannot read {}: {e}", path.display());
            return Config::default();
        }
    };
    match toml::from_str::<Config>(&text) {
        Ok(cfg) => cfg.sanitize(),
        Err(e) => {
            eprintln!("dockling: error in {}:\n{e}\nUsing default settings.", path.display());
            Config::default()
        }
    }
}

/// Writes every setting into the file, keeping its comments and layout
/// (missing keys are added). Used by the settings window.
pub fn save(path: &Path, cfg: &Config) -> std::io::Result<()> {
    let mut text = fs::read_to_string(path).unwrap_or_else(|_| DEFAULT_CONFIG.to_string());
    let str_value = |s: &str| {
        let mut out = String::from("\"");
        for c in s.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\t' => out.push_str("\\t"),
                c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
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
    // Write then rename, so the dock never reads a half-written file.
    let tmp = path.with_extension("toml.tmp");
    fs::write(&tmp, text)?;
    fs::rename(tmp, path)
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
            out.push_str(&format!("{indent}{key} = {value}\n"));
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
            out.push_str(&format!("\n[{name}]\n{new_line}"));
            out
        }
    }
}

/// Rewrites only the `pinned = [...]` entry of the file, keeping the user's comments.
pub fn save_pinned(path: &Path, pinned: &[String]) -> std::io::Result<()> {
    let text = fs::read_to_string(path).unwrap_or_else(|_| DEFAULT_CONFIG.to_string());
    fs::write(path, replace_pinned(&text, pinned))
}

fn replace_pinned(text: &str, pinned: &[String]) -> String {
    let mut block = String::from("pinned = [\n");
    for p in pinned {
        let escaped = p.replace('\\', "\\\\").replace('"', "\\\"");
        block.push_str(&format!("    \"{escaped}\",\n"));
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
        .map(|(pos, _)| pos)
        .unwrap_or(text.len());
    format!("{}{}\n\n{}", &text[..insert_at], block, &text[insert_at..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_file_matches_defaults() {
        let parsed: Config = toml::from_str(DEFAULT_CONFIG).unwrap();
        assert_eq!(parsed, Config::default());
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
}
