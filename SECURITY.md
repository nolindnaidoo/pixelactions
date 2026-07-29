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

The areas most worth scrutiny are the `unsafe` block in
`crates/pixelactions/src/mac.rs` (the only one, an FFI call to raise the
macOS Accessibility prompt), the coordinate conversion that decides
*where* input lands, the guards that decide whether to act at all (the
kill switch, and the refusal to act on a region pixelcoords could not
identify unambiguously), and the parsing of flow files and protocol
requests.
