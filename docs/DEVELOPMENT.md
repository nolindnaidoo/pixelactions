# Developing pixelactions

Everything about building, testing, and shipping this repository.
[AGENTS.md](../AGENTS.md) is the engineering-standards document — layout
rules, control-flow style, coordinate conventions, the testing bar; this
file is the practical companion. On any conflict, AGENTS.md wins.

## Build from source

```bash
git clone https://github.com/nolindnaidoo/pixelactions
cd pixelactions
cargo build --workspace
```

Rust 1.88+ (the enforced MSRV). Linux needs input-synthesis build
dependencies first:

```bash
# Debian/Ubuntu
sudo apt-get install -y libxdo-dev libxkbcommon-dev pkg-config
```

macOS and Windows need nothing beyond a Rust toolchain.

**pixelactions calls the `pixelcoords` binary** for relocation and
verification, so put it on your `PATH` (`cargo install pixelcoords`).
`pixelactions doctor` reports whether it found one and which version.

## Workspace layout

Two crates, one boundary:

- **`crates/pixelactions-core`** — pure logic, zero platform
  dependencies, `#![forbid(unsafe_code)]`. Modules: `convert`
  (coordinate spaces, the conversion a wrong answer would mean clicking
  the wrong place), `flow` (the flow-file schema, parsed strictly),
  `plan` (resolving labels against a session), `verb` (the chained-argv
  grammar), `chord` (the key names a chord may use), `protocol` (the
  `serve` line-protocol wire types), `report` (run reports and the
  exit-code contract), `display` (which display server a Linux session
  runs), `stream` (placing a pixel inside a Wayland input region), and
  `virtualdesk` (normalizing a pixel into the grid Windows takes).

  The last three are the pattern to copy when a platform needs
  arithmetic: the OS call lives in the binary, the math that decides
  *where* lives here, where it is tested without a screen.
- **`crates/pixelactions`** — the binary: CLI (`cli`, `main`), session
  loading (`session`), shelling out to pixelcoords (`verify`), input
  synthesis behind a trait (`inject`), the run loop (`run`), the line
  protocol server (`serve`), `doctor`, and the cfg-gated platform
  modules: `mac` (the Accessibility grant), `win` (`SendInput`, DPI
  awareness, the virtual desktop), and `portal` + `eis` (the Wayland
  grant and its wire protocol).

The rule that keeps the boundary honest: if a platform type appears in
core, that is a bug. New logic goes in core when it can (where it must be
unit-tested), in the binary only when it needs the OS.

**All three drive surfaces share one implementation.** Chained argv, a
flow file, and a protocol request all build the same `Flow` and go
through `plan` → `run::execute`. A surface that grows its own copy of
relocation, verification, or the kill switch is a bug — the three will
drift and only one will be tested.

## The checks

Run exactly what CI runs before every push:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

CI (`.github/workflows/ci.yml`) runs eight required jobs on every PR —
all must pass before anything merges to `main`:

| Job | What it enforces |
|-----|------------------|
| `test` (macOS, Windows, Ubuntu) | fmt, clippy pedantic `-D warnings`, tests, build — per OS |
| `xvfb` | a real synthetic event, posted to a live X server and read back |
| `msrv` | the workspace builds on Rust 1.88 |
| `policy` | no inline `#[allow(...)]` anywhere (workspace-level relaxations only) |
| `coverage` | 90% line coverage floor **per module** in core |
| `audit` | `cargo audit` |

`xvfb` is the only place CI proves injection rather than building it, and
X11 is the only platform whose display server runs on a runner. It stays a
smoke test: a headless X server is not a desktop, and nothing there proves
a click reached an application. **macOS, Windows and Wayland are verified
by hand on real hardware** — see Testing below.

**Check the other platforms before pushing.** A helper reachable only from
one platform's `#[cfg]` module still looks used on that platform — and is
dead code everywhere else, where clippy's `-D warnings` fails the build.
This is the most common way a green local run turns into a red CI one.
Catch it locally:

```bash
rustup target add x86_64-unknown-linux-gnu x86_64-pc-windows-msvc
for t in x86_64-unknown-linux-gnu x86_64-pc-windows-msvc; do
  cargo clippy --target "$t" --workspace --all-targets -- -D warnings
done
```

Clippy only checks, so no cross-linker or system libraries are needed.

Tests are the other half, and they cannot be cross-run. Two traps have
already cost a red CI run each:

- A **path assertion written as a string literal** passes on Unix and
  fails on Windows, because `PathBuf::join` inserts the platform's own
  separator. Compare against a `PathBuf` built with `join`.
- A **coordinate assertion under `Space::Auto`** passes on macOS and fails
  elsewhere, because `Auto` resolves to logical points on macOS and
  physical pixels on Windows and X11. Pin `settings.space` in any test
  that asserts a converted number.

## Testing

- Core is pure; everything in it is unit-tested, and the conversion —
  the module where a bug means clicking the wrong place — carries
  property tests in `crates/pixelactions-core/tests/`.
- The run loop is testable without a screen because injection sits
  behind a trait: `Recording` in `inject.rs` records what it was asked
  to do and moves nothing. Ordering, settling, verification, the kill
  switch, and refusal behavior are all covered that way. Follow that
  pattern; do not mock the window system.
- **Input synthesis and permission behavior are verified by manual runs
  on real hardware**, per platform, and stated plainly as such. A green
  suite proves the coordinates were computed, never that anything moved.
- Every bug fix ships with a regression test that fails before the fix.

Measuring coverage like CI does:

```bash
rustup component add llvm-tools-preview
cargo install cargo-llvm-cov
cargo llvm-cov -p pixelactions-core --summary-only
```

Scoped to core, as the floor is — measuring `--workspace` folds in
platform plumbing the floor was never meant to cover.

## Verifying against a real session

You need a pixelcoords session to run anything. Capture one
interactively (`pixelcoords`, mark a region, save), then:

```bash
pixelcoords find --session DIR              # is the screen still what it was?
pixelactions plan --session DIR click:label # resolve, touch nothing
pixelactions run  --session DIR click:label --yes
```

`plan` before `run` on any session you have not acted on before. The
`serve` surface is exercised the same way — pipe NDJSON into it:

```bash
printf '%s\n' '{"id":1,"do":"hello","version":1}' \
              '{"id":2,"do":"verify","target":"label"}' \
              '{"id":3,"do":"bye"}' \
  | pixelactions serve --session DIR
```

## Platform coordinate work

Input APIs disagree about units: macOS `CGEvent` speaks logical points,
Windows `SendInput` and X11 `XTEST` speak physical pixels.
`convert::Space::Auto` resolves that in `native_space()` — one place, no
call site guessing. Never write platform coordinate math from
assumption; `design/02-TECHNICAL-FOUNDATIONS.md` cites primary sources
for each platform's conventions and known off-by-ones.

Two of those platforms need a second hop after `Space::Auto`, and both
put the arithmetic in core rather than in the injector:

- **Windows** takes 0..65535 over the virtual desktop, not pixels.
  `virtualdesk::normalize` divides by `dimension − 1` and rounds — the
  off-by-one that otherwise makes the rightmost column unreachable — and
  refuses a point off the desktop rather than letting Windows clamp it to
  an edge and click there.
- **Wayland** takes a position inside a granted region, learned at
  runtime; `stream::place` does that hop.

**Read the dependency's code, not its README, when the claim is about
coordinates.** enigo's README says it handles the Windows DPI dance;
0.6.1's `move_mouse` normalizes against the *primary monitor* and carries
a `TODO` about `MOUSEEVENTF_VIRTUALDESK`. That is why the Windows pointer
path is ours and the keyboard is still enigo's.

## Releases

Before every publish, walk this list — the ones with easy misses first:

1. **Update the install snippet in `crates/pixelactions-core/README.md`.**
   A pre-1.0 caret pin like `= "0.1"` resolves to the newest 0.1.x, not
   0.2.x — so a reader copy-pasting from crates.io lands on the old API.
   Bump the string to the current minor before every minor cut. (The
   sister project shipped a release without this and had to cut a patch
   solely to fix the crates.io page.)
2. Bump the workspace version and the `pixelcoords-core` dep pin in
   both `crates/pixelactions-core/Cargo.toml` and
   `crates/pixelactions/Cargo.toml` if pixelcoords-core has moved.
3. Write the CHANGELOG entry.
4. Run `cargo fmt --all --check`, `cargo clippy --workspace --all-targets
   -- -D warnings`, `cargo test --workspace`.
5. Dry-run: `cargo publish -p pixelactions-core --dry-run`.


Versioning policy (also stated in [CHANGELOG.md](../CHANGELOG.md)):
pre-1.0, **minor** for features and any break to the CLI, the flow file,
or the line protocol; **patch** for fixes; 1.0.0 when those three are
declared stable.

- Features land through issues → PRs (`Closes #N`), each writing its
  CHANGELOG entry under the upcoming version's heading in the same PR.
  The version in `Cargo.toml` does not move in feature PRs.
- The release cut: one PR bumps the workspace version and the core dep
  pin. Then tag `v<X.Y.Z>` — the tag triggers
  `.github/workflows/release.yml`, which builds macOS binaries (arm64 +
  x86_64), a Linux x86_64 binary and a Windows x86_64 binary, then opens a
  **draft** GitHub release with the archives attached. A target belongs in
  that matrix only once injection actually works on the platform;
  shipping a binary that refuses to inject would imply support this build
  does not have.
- crates.io publish order matters: `cargo publish -p pixelactions-core`
  first, then `-p pixelactions` (the binary's dep pin must resolve).
  Publishes are manual and deliberate; nothing in CI publishes.
- `pixelcoords-core` is a **crates.io dependency with a caret range,
  never a path dependency** — see the compatibility contract in
  AGENTS.md. The sister repo releases on its own schedule; this one
  upgrades deliberately.

## Repository governance

- `main` accepts pull requests only — a branch ruleset blocks direct
  pushes, force pushes, and deletion, and requires all eight CI jobs
  green to merge. Anyone can open a PR; only the maintainer merges.
- Dependabot checks weekly (cargo + GitHub Actions, grouped PRs) and
  security advisories immediately. Patch/minor updates auto-merge once
  every required check passes; majors wait for human review. Actions are
  pinned to commit SHAs; Dependabot maintains the pins.
- Issue forms require the diagnostics a report needs (`doctor` output,
  OS, version); the feature form points at the non-goals.
