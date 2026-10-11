//! Minimal `.desktop` file reader (freedesktop Desktop Entry specification): finds the
//! installed applications and matches windows to them.

use crate::config::home_dir;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// An application, from its desktop entry.
#[derive(Debug, Clone)]
pub struct App {
    /// Desktop file id without the ".desktop" suffix, e.g. "org.xfce.mousepad".
    pub id: String,
    /// Name shown in tooltips and menus.
    pub name: String,
    /// The command line, split into arguments, field codes expanded or dropped.
    pub exec: Vec<String>,
    /// Icon name from the icon theme, or a path.
    pub icon: String,
    /// Lowercased `StartupWMClass`, empty when absent.
    pub wm_class: String,
    /// Runs in a terminal (`Terminal=true`).
    pub terminal: bool,
    /// Hidden from menus (NoDisplay=true); still used to recognize windows.
    pub no_display: bool,
}

impl App {
    fn id_lower(&self) -> String {
        self.id.to_lowercase()
    }

    fn exec_name(&self) -> String {
        self.exec
            .first()
            .and_then(|p| Path::new(p).file_name())
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default()
    }
}

/// XDG data directories, most important first.
pub fn data_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    match std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
        Some(v) => dirs.push(PathBuf::from(v)),
        None => dirs.push(home_dir().join(".local/share")),
    }
    let system = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    dirs.extend(system.split(':').filter(|s| !s.is_empty()).map(PathBuf::from));
    dirs
}

/// Every application of the XDG data directories, the user's first. An id found again in a
/// later directory is ignored, even when the first entry is hidden.
#[must_use]
pub fn load_apps() -> Vec<App> {
    load_apps_in(&data_dirs())
}

fn load_apps_in(dirs: &[PathBuf]) -> Vec<App> {
    let mut apps = Vec::new();
    let mut seen = HashSet::new();
    for dir in dirs {
        scan(&dir.join("applications"), "", &mut apps, &mut seen);
    }
    apps
}

fn scan(dir: &Path, prefix: &str, apps: &mut Vec<App>, seen: &mut HashSet<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        if path.is_dir() {
            scan(&path, &format!("{prefix}{name}-"), apps, seen);
        } else if let Some(stem) = name.strip_suffix(".desktop") {
            let id = format!("{prefix}{stem}");
            // The first directory wins, even when that entry is hidden.
            if seen.insert(id.clone())
                && let Some(app) = parse(&path, id)
            {
                apps.push(app);
            }
        }
    }
}

/// Reads the desktop entry at `path`, whose desktop file id is `id`. None for unreadable files,
/// entries that are not applications, hidden ones and those without a command.
#[must_use]
pub fn parse(path: &Path, id: String) -> Option<App> {
    let text = fs::read_to_string(path).ok()?;
    let mut in_entry = false;
    let (mut name, mut exec, mut icon, mut wm_class) = (None, None, String::new(), String::new());
    let (mut terminal, mut no_display) = (false, false);
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = unescape(value.trim());
        match key.trim() {
            "Type" if value != "Application" => return None,
            "Hidden" if value == "true" => return None,
            "Name" => name = Some(value),
            "Exec" => exec = Some(value),
            "Icon" => icon = value,
            "StartupWMClass" => wm_class = value.to_lowercase(),
            "Terminal" => terminal = value == "true",
            "NoDisplay" => no_display = value == "true",
            _ => {}
        }
    }
    let exec = parse_exec(&exec?, &icon, name.as_deref().unwrap_or(&id), path);
    if exec.is_empty() {
        return None;
    }
    Some(App { name: name.unwrap_or_else(|| id.clone()), id, exec, icon, wm_class, terminal, no_display })
}

/// Unescapes a desktop-entry string value (\s \n \t \r \\).
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => out.push(' '),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') | None => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
        }
    }
    out
}

/// Splits an Exec value into arguments and expands field codes.
/// File/URL codes (%f %F %u %U ...) are dropped since the dock never passes files.
fn parse_exec(exec: &str, icon: &str, name: &str, path: &Path) -> Vec<String> {
    let mut words: Vec<(String, bool)> = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut in_word = false;
    let mut was_quoted = false;
    let mut chars = exec.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                in_word = true;
                was_quoted = true;
            }
            '\\' if quoted => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            ' ' | '\t' if !quoted => {
                if in_word {
                    words.push((std::mem::take(&mut cur), was_quoted));
                    in_word = false;
                    was_quoted = false;
                }
            }
            _ => {
                cur.push(c);
                in_word = true;
            }
        }
    }
    if in_word {
        words.push((cur, was_quoted));
    }

    let mut args = Vec::new();
    for (word, was_quoted) in words {
        if !was_quoted {
            match word.as_str() {
                "%f" | "%F" | "%u" | "%U" | "%d" | "%D" | "%n" | "%N" | "%v" | "%m" => continue,
                "%i" => {
                    if !icon.is_empty() {
                        args.push("--icon".into());
                        args.push(icon.into());
                    }
                    continue;
                }
                _ => {}
            }
        }
        let mut out = String::new();
        let mut it = word.chars().peekable();
        while let Some(c) = it.next() {
            if c != '%' {
                out.push(c);
                continue;
            }
            match it.next() {
                Some('%') => out.push('%'),
                Some('c') => out.push_str(name),
                Some('k') => out.push_str(&path.to_string_lossy()),
                _ => {}
            }
        }
        args.push(out);
    }
    args
}

/// Resolves a pinned entry: a desktop id (with or without ".desktop"), a short
/// name like "mousepad", or an absolute path to a .desktop file.
pub fn resolve(apps: &mut Vec<App>, entry: &str) -> Option<usize> {
    let entry = entry.trim();
    if entry.starts_with('/') {
        let path = Path::new(entry);
        let id = path.file_stem()?.to_string_lossy().into_owned();
        if let Some(i) = apps.iter().position(|a| a.id == id) {
            return Some(i);
        }
        apps.push(parse(path, id)?);
        return Some(apps.len() - 1);
    }
    let want = entry.strip_suffix(".desktop").unwrap_or(entry);
    let lower = want.to_lowercase();
    let dotted = format!(".{lower}");
    apps.iter()
        .position(|a| a.id == want)
        .or_else(|| apps.iter().position(|a| a.id_lower() == lower))
        .or_else(|| apps.iter().position(|a| a.id_lower().ends_with(&dotted)))
        .or_else(|| apps.iter().position(|a| a.exec_name() == lower))
}

/// Finds the application owning a window from its `WM_CLASS` (instance, class).
#[must_use]
pub fn match_window(apps: &[App], instance: &str, class: &str) -> Option<usize> {
    let inst = instance.to_lowercase();
    let class = class.to_lowercase();
    let is = |s: &str| !s.is_empty() && (s == class || s == inst);
    apps.iter()
        .position(|a| is(&a.wm_class))
        .or_else(|| apps.iter().position(|a| is(&a.id_lower())))
        .or_else(|| {
            apps.iter().position(|a| {
                let id = a.id_lower();
                is(id.rsplit('.').next().unwrap_or(&id))
            })
        })
        .or_else(|| apps.iter().position(|a| a.wm_class.is_empty() && is(&a.exec_name())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exec(s: &str) -> Vec<String> {
        parse_exec(s, "icon", "Name", Path::new("/x.desktop"))
    }

    #[test]
    fn exec_field_codes() {
        assert_eq!(exec("firefox-esr %u"), vec!["firefox-esr"]);
        assert_eq!(exec("app --name=%c %%x %F"), vec!["app", "--name=Name", "%x"]);
        assert_eq!(exec("app %i"), vec!["app", "--icon", "icon"]);
    }

    #[test]
    fn exec_quoting() {
        assert_eq!(exec(r#"sh -c "echo \"hi\" there""#), vec!["sh", "-c", "echo \"hi\" there"]);
        assert_eq!(exec(r#""/opt/My App/run" -x"#), vec!["/opt/My App/run", "-x"]);
    }

    fn app(id: &str, exec: &str, class: &str) -> App {
        App {
            id: id.into(),
            name: id.into(),
            exec: vec![exec.into()],
            icon: String::new(),
            wm_class: class.into(),
            terminal: false,
            no_display: false,
        }
    }

    fn write(dir: &Path, rel: &str, text: &str) -> PathBuf {
        let path = dir.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        path
    }

    #[test]
    fn reads_desktop_entries() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(
            dir.path(),
            "editor.desktop",
            "# comment\n[Desktop Entry]\nType=Application\nName=My\\sEditor\nExec=editor --new %F\nIcon=accessories-text-editor\n\
             StartupWMClass=MyEditor\nTerminal=false\nNoDisplay=true\n[Desktop Action new]\nName=Ignored\nExec=ignored\n",
        );
        let app = parse(&path, "editor".into()).unwrap();
        assert_eq!((app.name.as_str(), app.exec.clone()), ("My Editor", vec!["editor".to_string(), "--new".into()]));
        assert_eq!((app.icon.as_str(), app.wm_class.as_str()), ("accessories-text-editor", "myeditor"));
        assert!(app.no_display && !app.terminal);
        let skipped = [
            "[Desktop Entry]\nType=Link\nName=L\nExec=x\n",
            "[Desktop Entry]\nName=H\nExec=x\nHidden=true\n",
            "[Desktop Entry]\nName=No command\n",
            "[Desktop Entry]\nName=Only codes\nExec=%U\n",
        ];
        for text in skipped {
            assert!(parse(&write(dir.path(), "x.desktop", text), "x".into()).is_none(), "{text}");
        }
        assert!(parse(&dir.path().join("missing.desktop"), "m".into()).is_none());
        // The name defaults to the id.
        let app =
            parse(&write(dir.path(), "n.desktop", "[Desktop Entry]\nExec=tool\nTerminal=true\n"), "n".into()).unwrap();
        assert_eq!((app.name.as_str(), app.terminal), ("n", true));
    }

    #[test]
    fn the_first_directory_wins_and_subdirectories_prefix_the_id() {
        let (user, system) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        write(user.path(), "applications/a.desktop", "[Desktop Entry]\nName=Mine\nExec=a\nHidden=true\n");
        write(system.path(), "applications/a.desktop", "[Desktop Entry]\nName=System A\nExec=a\n");
        write(system.path(), "applications/b.desktop", "[Desktop Entry]\nName=B\nExec=b\n");
        write(system.path(), "applications/kde4/c.desktop", "[Desktop Entry]\nName=C\nExec=c\n");
        let apps = load_apps_in(&[user.path().to_path_buf(), system.path().to_path_buf()]);
        let ids: Vec<&str> = apps.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, ["b", "kde4-c"], "the user's hidden a.desktop hides the system one");
    }

    #[test]
    fn unescapes_values() {
        assert_eq!(unescape(r"a\sb\tc\nd\re\\f\qg\"), "a b\tc\nd\re\\f\\qg\\");
    }

    #[test]
    fn pinned_entries_resolve_by_path_and_by_exec() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(dir.path(), "custom.desktop", "[Desktop Entry]\nName=Custom\nExec=/opt/custom/bin/run\n");
        let mut apps = Vec::new();
        assert_eq!(resolve(&mut apps, &path.to_string_lossy()), Some(0), "an absolute path is read");
        assert_eq!(resolve(&mut apps, &path.to_string_lossy()), Some(0), "and only once");
        assert_eq!(resolve(&mut apps, "run"), Some(0), "the program's name");
        assert_eq!(resolve(&mut apps, "/no/such.desktop"), None);
        assert_eq!(match_window(&apps, "run", "Run"), Some(0));
        assert_eq!(match_window(&apps, "", ""), None);
    }

    #[test]
    fn matching() {
        let mut apps = vec![
            app("firefox-esr", "/usr/lib/firefox-esr/firefox-esr", ""),
            app("org.xfce.mousepad", "mousepad", ""),
            app("thunar", "thunar", ""),
            app("code", "/usr/share/code/code", "code-oss"),
        ];
        assert_eq!(match_window(&apps, "Navigator", "firefox-esr"), Some(0));
        assert_eq!(match_window(&apps, "mousepad", "Mousepad"), Some(1));
        assert_eq!(match_window(&apps, "thunar", "Thunar"), Some(2));
        assert_eq!(match_window(&apps, "code-oss", "Code-oss"), Some(3));
        assert_eq!(match_window(&apps, "xterm", "XTerm"), None);
        assert_eq!(resolve(&mut apps, "mousepad"), Some(1));
        assert_eq!(resolve(&mut apps, "Thunar.desktop"), Some(2));
    }
}
