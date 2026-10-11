//! The `dockling` command line, for what works without an X server.

use std::process::{Command, Output};

fn dockling(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_dockling")).args(args).env("DISPLAY", ":no-such-display").output().unwrap()
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn check_config_lists_the_problems() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "position = \"bottom\"\ntooltip = false\nicon_size = 900\n").unwrap();
    let out = dockling(&["--check-config", path.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    let p = path.display();
    assert_eq!(
        stdout(&out),
        format!(
            "{p}:2: unknown setting \"tooltip\". Did you mean \"tooltips\"?\n\
             {p}:3: icon_size = 900 is out of range (16 to 256); using 256\n\
             2 problems: the dock skips these settings and uses all the others.\n"
        )
    );

    std::fs::write(&path, dockling::config::DEFAULT_CONFIG).unwrap();
    let out = dockling(&["--check-config", path.to_str().unwrap()]);
    assert_eq!((out.status.code(), stdout(&out)), (Some(0), format!("{p}: OK\n")));

    let missing = dir.path().join("missing.toml");
    let out = dockling(&["--check-config", missing.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(0));
    assert!(!missing.exists(), "checking never creates the file");
}

#[test]
fn print_default_config_and_version() {
    let out = dockling(&["--print-default-config"]);
    assert_eq!(stdout(&out), dockling::config::DEFAULT_CONFIG);
    let out = dockling(&["--version"]);
    assert_eq!(stdout(&out), format!("dockling {}\n", env!("CARGO_PKG_VERSION")));
    assert!(stdout(&dockling(&["--help"])).contains("--check-config"));
}

#[test]
fn errors_are_explained() {
    let out = dockling(&["--frobnicate"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).starts_with("dockling: unknown option --frobnicate"));
    let out = dockling(&["--quit"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).starts_with("dockling: cannot connect to the X server"));
}
