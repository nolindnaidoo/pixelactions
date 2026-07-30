# Security policy

## Supported versions

The latest release and the current `main` branch.

## Reporting a vulnerability

Report privately via GitHub Security Advisories: the repository's
Security tab, then "Report a vulnerability". Please do not open a public
issue for security reports. You can expect an acknowledgment within a few
days.

## Scope notes

pixelactions **synthesizes keyboard and mouse input**, which makes its
threat model larger than its sister tool's. It makes no network calls and
opens no listening socket — `serve` speaks only to the process that
launched it, over stdin and stdout, deliberately: this process holds the
permission to click and type, and a listener would lend that permission
to anything able to reach it.

`pixelactions-core` carries `#![forbid(unsafe_code)]`, so every `unsafe`
in this project is in the binary's platform modules, calling an OS input
API. In shipping code that is:

- `crates/pixelactions/src/win.rs` — `SendInput` itself, plus the
  per-monitor DPI declaration, the virtual-desktop metrics, `GetCursorPos`,
  and the process-token read behind the elevation report
- `crates/pixelactions/src/mac.rs` — the `CGEvent` posting path and the
  FFI call that raises the Accessibility prompt
- `crates/pixelactions/src/eis.rs` — reading the compositor's keymap from
  the file descriptor EIS hands over

(`portal.rs` and `main.rs` also contain `unsafe`, but only in `#[cfg(test)]`
code, where edition 2024 requires it for `set_var`/`remove_var`.)

Also worth scrutiny, and arguably worth more of it: the coordinate
conversion that decides *where* input lands, the guards that decide
whether to act at all (the kill switch, and the refusal to act on a region
pixelcoords could not identify unambiguously), and the parsing of flow
files and protocol requests.
