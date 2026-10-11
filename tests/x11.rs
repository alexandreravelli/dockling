//! The dock on a real X server: Xvfb, started for the test. Skipped where Xvfb is not
//! installed (`apt install xvfb`).

use std::path::Path;
use std::process::{Child, Command, Output, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

/// An Xvfb server, stopped when dropped.
struct Server {
    display: String,
    child: Child,
}

impl Server {
    fn start() -> Option<Server> {
        // A display number of its own, so tests running at the same time do not meet.
        let number = 100 + std::process::id() % 800;
        let display = format!(":{number}");
        let child = Command::new("Xvfb")
            .args([display.as_str(), "-screen", "0", "1280x800x24", "-nolisten", "tcp"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let socket = format!("/tmp/.X11-unix/X{number}");
        let ready = wait(|| Path::new(&socket).exists());
        let server = Server { display, child };
        ready.then_some(server)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Waits up to five seconds for `condition`.
fn wait(mut condition: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        if condition() {
            return true;
        }
        sleep(Duration::from_millis(50));
    }
    false
}

fn dockling(server: &Server, home: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_dockling"));
    cmd.env("DISPLAY", &server.display)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .stdin(Stdio::null());
    cmd
}

fn run(server: &Server, home: &Path, arg: &str) -> Output {
    dockling(server, home).arg(arg).output().unwrap()
}

#[test]
fn the_dock_starts_reloads_and_quits() {
    let Some(server) = Server::start() else {
        eprintln!("skipped: Xvfb is not installed");
        return;
    };
    let home = tempfile::tempdir().unwrap();
    let config = home.path().join(".config/dockling");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(config.join("config.toml"), "position = \"bottom\"\nautohide = true\nautohide_delay_ms = 100\n")
        .unwrap();

    let mut dock = dockling(&server, home.path()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn().unwrap();
    // The dock answers --reload once it owns its selection.
    assert!(wait(|| run(&server, home.path(), "--reload").status.success()), "the dock did not start");

    let second = dockling(&server, home.path()).output().unwrap();
    assert_eq!(second.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&second.stderr).contains("already running"));

    // Let the auto-hide timer fire, then quit.
    sleep(Duration::from_millis(300));
    assert!(run(&server, home.path(), "--quit").status.success());
    assert!(wait(|| dock.try_wait().is_ok_and(|s| s.is_some())), "the dock did not quit");
    let out = dock.wait_with_output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(out.stderr.is_empty(), "the configuration is valid: {}", String::from_utf8_lossy(&out.stderr));

    // Nothing is running any more.
    let after = run(&server, home.path(), "--quit");
    assert!(String::from_utf8_lossy(&after.stderr).contains("not running"));
}
