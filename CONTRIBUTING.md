# Contributing to Dockling

Thanks for helping! Bug reports, ideas and pull requests are all welcome.

## Reporting a problem

Open an [issue](https://github.com/alexandreravelli/dockling/issues) with the output of
`dockling --version`, your distribution and desktop, and `dockling --check-config`. Start the dock
with `DOCKLING_DEBUG=1 dockling --replace` from a terminal to see what it does (icons it looks
for, X errors). For a security problem, follow [SECURITY.md](SECURITY.md) instead.

## Setting up

Dockling needs Rust 1.92 or newer; the GTK 3 headers are only needed for the settings window:

```sh
sudo apt install -y build-essential curl git libgtk-3-dev xvfb
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
git clone https://github.com/alexandreravelli/dockling
cd dockling
```

The code is split in three:

- the library (`src/lib.rs`): the configuration file and the `.desktop` database, shared by
  both programs and documented on [docs.rs](https://docs.rs/dockling);
- the dock (`src/main.rs`, `dock.rs`, `icons.rs`, `text.rs`): X11 with x11rb, drawing with
  tiny-skia;
- the settings window (`src/bin/dockling-settings.rs`): GTK 3.

Try a change without touching your own dock: `cargo run -- --replace`, then `dockling --replace`
to get yours back.

## Before you open a pull request

CI runs these checks; run them first:

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features   # tests/x11.rs starts the dock on Xvfb when it is installed
cargo doc --no-deps --no-default-features --features svg
cargo deny check            # cargo install cargo-deny
```

- Keep pull requests small, with one topic each, and add tests for new behaviour.
- Document every public item of the library; examples in doc comments are run as tests.
- Keep the dock light: no toolkit in the dock, and think twice before adding a dependency.
- Write code, comments, messages and docs in English.

## Commit messages

Commits follow [Conventional Commits](https://www.conventionalcommits.org/): `type(scope): summary`,
in the imperative, under 72 characters, with a body that explains why when it is not obvious.

| Type | For |
| --- | --- |
| `feat` | a new feature |
| `fix` | a bug fix |
| `docs`, `test`, `refactor`, `style`, `perf` | documentation, tests, code changes without new behaviour |
| `build`, `ci`, `chore` | dependencies, packaging, workflows, maintenance |

Add `!` after the type (`feat!:`) for a change that breaks the configuration file, the command
line or the library's API. The release notes and the next version number are computed from these
messages.

## Releases

[release-plz](https://release-plz.dev) keeps a release pull request open: it bumps the version
from the commit messages and writes the [changelog](CHANGELOG.md). Merging it makes the Release
workflow run every check, build the Debian package and the binaries, publish the GitHub release
and, once enabled, the crates.io version.
