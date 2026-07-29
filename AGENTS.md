# pixelactions — engineering standards

This is the source of truth for how code in this repository is written,
tested, and reviewed. It applies to every contributor, human or
AI-assisted. It is deliberately the same document as its sister project
`pixelcoords` — the two are built by one developer to one standard, and
should read that way.

## What this project is

The executor half of a loop pixelcoords starts. pixelcoords freezes the
screen, a human marks labeled regions, and it writes pixel-exact
coordinates with crops, template re-location (`find`), and point
verification (`assert`). **pixelactions consumes that ground truth and
performs the interactions** — click, type, chord, drag — declaratively,
and confirms they landed.

Current scope: **macOS**, resolution and reporting. Injection is the
next milestone. See `design/07-MILESTONES.md`.

## Layout

- `crates/pixelactions-core` — pure logic: flow schema, plan resolution,
  coordinate-space conversion, run reports. **Zero platform deps,
  `#![forbid(unsafe_code)]`, everything unit-tested.** If a platform
  type (`enigo`, Core Graphics, Win32) appears here, that is a bug.
- `crates/pixelactions` — the binary: CLI, session loading, calling
  pixelcoords, and (next) input synthesis. Platform-specific code lives
  in cfg-gated modules.

Keep modules flat. No layers, registries, managers, or services. No
trait with a single implementation.

## The compatibility contract (read before touching session code)

The two tools must be independently releasable. Changes on either side
must never break the other, which is enforced structurally:

- **The dependency is one-way and forever.** pixelcoords does not know
  pixelactions exists. No cycle, no lockstep.
- **`pixelcoords-core` is a crates.io dependency with a caret range,
  never a path dependency.** The sister repo evolves on its own
  schedule; this repo upgrades deliberately.
- **Session reads are tolerant.** `session.json` is parsed through
  `pixelcoords-core`'s own types, which ignore unknown fields, so every
  additive schema change upstream is a no-op here. There is a test that
  says so (`session.rs`, `unknown_future_fields_are_ignored_not_fatal`)
  — keep it passing.
- **Our own config is strict.** Flow files use
  `serde(deny_unknown_fields)`: a typo is an error at parse time, not a
  silently skipped step. Tolerance is for other people's data, not ours.
- **Versions are checked, not assumed.** `doctor` reports the installed
  pixelcoords version and the session schema this build understands. A
  session from the future is refused with a message naming the fix.
- **Capture-time work belongs to pixelcoords.** We shell out to its
  binary for `find`; we never reimplement capture, geometry, or template
  matching. If something is missing there, the fix belongs there.

## Control-flow style

Flat over nested, guards over branches:

- **No statement-position `else`.** Guard clauses and early `return`
  (`if !ok { return ... }` / `let Some(x) = ... else { return }`), then
  the happy path. Ranked policies read as a list of early returns.
- **Value-position `if/else` is fine** — Rust's ternary.
- **`match` is preferred** over any chain of tests on the same value;
  use match guards rather than `if/else` inside arms.
- Prefer combinators where they read cleanly: `bool::then_some`,
  `Option::map/filter/is_some_and`, `?`.
- No nesting deeper than two levels inside a function; extract a helper.

## Hard rules

- **No inline `#[allow(...)]`** — fix the lint, or add a visible,
  commented relaxation to `[workspace.lints]` in the root `Cargo.toml`.
- **Clippy pedantic, deny warnings.** `cargo clippy --workspace
  --all-targets -- -D warnings` must pass exactly as CI runs it.
- **`unsafe` is forbidden in core**, and allowed in the binary only
  inside platform modules for OS API calls.
- **Strict parsing of our own inputs, never silent defaults.**
- **Dependencies are a cost.** Justify every new one in the PR body.
- **No async runtime.** This tool runs, acts, and exits.

## Coordinates (read before touching `convert` or `plan`)

- The session's authoritative space is **monitor-local physical
  pixels**, with `origin_px` giving the global desktop position and
  `scale` the DPI factor.
- **Input APIs disagree about what they want**: macOS `CGEvent` speaks
  logical points; Windows `SendInput` and X11 `XTEST` speak physical
  pixels; Wayland absolute motion speaks a screencast stream's space.
  `convert::Space::Auto` resolves to the right one per platform, and
  that decision lives in `native_space()` — one place, no call site
  guessing.
- Conversion divides by the **containing monitor's** scale, never a
  global one. Mixed-DPI layouts are the normal case, not an edge case.
- A point outside every described monitor is **refused**, never clamped.
  Clamping would click somewhere plausible and wrong.
- Never write platform coordinate math from assumption — the research
  in `design/02-TECHNICAL-FOUNDATIONS.md` cites primary sources for each
  platform's conventions and known off-by-ones.

## Safety

This tool synthesizes input. That earns rules the sister tool doesn't
need:

- **Planning is total.** Every label resolves before any action runs; a
  flow referencing a missing label fails whole rather than half-executed.
- **Dry-run (`plan`) is first-class and permanently supported** — it
  prints every resolved coordinate, after conversion, and touches
  nothing.
- **Reports distinguish "executed" from "verified".** "Nothing errored"
  is not "it worked", and the wire format says which.
- Injection, when it lands, gets a kill switch, a watchdog, and bounds
  enforcement before it gets convenience features.

## Testing

- **`pixelactions-core`: 90% line coverage floor per module.** Everything
  in core is pure; if something is hard to test there, the design is
  wrong.
- **Invariants get property tests** (`crates/pixelactions-core/tests/`).
  The conversion is the module where a bug means clicking the wrong
  place, so its rules are stated for every input, not just examples.
- **Every bug fix ships with a regression test** that fails before the
  fix.
- **Do not mock the window system.** Real injection and permission
  behavior are verified by manual runs on real hardware, per platform,
  and stated plainly as such — never claimed from a green test.
- Tests touching the filesystem use unique temp paths and clean up.

## Verification — the definition of done

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

A change is not done because it compiles; it is done when it is tested,
linted, documented where behavior changed, and honest — claims in docs
must match the code.

## Commits and pull requests

- Imperative subject; body explains the *why* and the user-visible
  consequence, not a list of files.
- One concern per PR. Refactors and behavior changes travel separately.
- If docs describe the thing you changed, update them in the same change.
