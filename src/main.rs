//! Dockling: an ultra-lightweight, Ubuntu-style dock for X11 desktops (XFCE, ...).

mod dock;
mod icons;
mod text;

use dockling::{config, desktop};
use std::path::PathBuf;
use std::process::ExitCode;

const HELP: &str = "\
Dockling - an ultra-lightweight application dock for X11

Usage: dockling [OPTION]

Options:
  (none)                  Start the dock
  --replace               Start the dock, replacing a running instance
  --reload                Make the running dock reload its configuration
  --quit                  Stop the running dock
  --print-default-config  Print the default configuration file
  --config-path           Print the configuration file location
  --check-config [FILE]   Check the configuration file and list its problems
  -V, --version           Print the version
  -h, --help              Print this help

Configuration: ~/.config/dockling/config.toml (created on first start)";

fn main() -> ExitCode {
    let arg = std::env::args().nth(1);
    let result = match arg.as_deref() {
        None => dock::run(false),
        Some("--replace") => dock::run(true),
        Some("--reload") => dock::send_command(dock::CMD_RELOAD),
        Some("--quit") => dock::send_command(dock::CMD_QUIT),
        Some("--print-default-config") => {
            print!("{}", config::DEFAULT_CONFIG);
            Ok(())
        }
        Some("--config-path") => {
            println!("{}", config::config_path().display());
            Ok(())
        }
        Some("--check-config") => return check_config(std::env::args().nth(2).map(PathBuf::from)),
        Some("-V" | "--version") => {
            println!("dockling {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some("-h" | "--help") => {
            println!("{HELP}");
            Ok(())
        }
        Some(other) => {
            eprintln!("dockling: unknown option {other}\n\n{HELP}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("dockling: {e:#}");
            ExitCode::FAILURE
        }
    }
}

/// `--check-config [FILE]`: lists the problems of the configuration file, one per line
/// (`file:line: message`, as compilers write them); fails when there is one.
fn check_config(file: Option<PathBuf>) -> ExitCode {
    let path = file.unwrap_or_else(config::config_path);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            println!("{}: no such file; the dock creates it with the default settings", path.display());
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("dockling: cannot read {}: {e}", path.display());
            return ExitCode::FAILURE;
        }
    };
    let warnings = config::parse(&text).warnings;
    for w in &warnings {
        match w.line {
            Some(line) => println!("{}:{line}: {}", path.display(), w.message),
            None => println!("{}: {}", path.display(), w.message),
        }
    }
    match warnings.len() {
        0 => {
            println!("{}: OK", path.display());
            ExitCode::SUCCESS
        }
        n => {
            let problems = if n == 1 { "1 problem" } else { &format!("{n} problems") };
            println!("{problems}: the dock skips these settings and uses all the others.");
            ExitCode::FAILURE
        }
    }
}
