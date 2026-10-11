# Security policy

## Supported versions

Only the latest release gets fixes.

## Reporting a vulnerability

Please do not open a public issue. Report it privately through GitHub:
[Security > Report a vulnerability](https://github.com/alexandreravelli/dockling/security/advisories/new).

Say what an attacker can do, and how to reproduce it. The fix is released as soon as it is ready,
with an advisory that credits you unless you prefer otherwise.

## What is in scope

- Files the dock reads that another user or program could control: `.desktop` entries, icon
  themes, fonts and the configuration file (a crash on bad input counts).
- Commands the dock starts: `Exec` lines of desktop entries, the "Show Applications" command and
  the terminal.
- The X11 messages the dock accepts (`dockling --reload`, `--quit`).
