//! Dockling: an ultra-lightweight, Ubuntu-style dock for X11 desktops (XFCE, ...).

mod config;
mod desktop;
mod dock;
mod icons;
mod text;

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
  -V, --version           Print the version
  -h, --help              Print this help

Configuration: ~/.config/dockling/config.toml (created on first start)";

fn main() {
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
            std::process::exit(2);
        }
    };
    if let Err(e) = result {
        eprintln!("dockling: {e}");
        std::process::exit(1);
    }
}
