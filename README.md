<h1 align="center">pixelactions</h1>

<p align="center">
  <b>Consume human-verified coordinates, perform the interaction, confirm it landed</b><br/>
  <i>Click, type, chord, drag, scroll — from a chained CLI, a flow file, or a line protocol any language can drive</i>
</p>

<p align="center">
  <a href="https://github.com/nolindnaidoo/pixelactions/actions/workflows/ci.yml">
    <img src="https://github.com/nolindnaidoo/pixelactions/actions/workflows/ci.yml/badge.svg" alt="Build Status" />
  </a>
  <a href="https://docs.rs/pixelactions-core">
    <img src="https://img.shields.io/docsrs/pixelactions-core.svg" alt="docs.rs" />
  </a>
  <a href="https://crates.io/crates/pixelactions">
    <img src="https://img.shields.io/crates/v/pixelactions.svg" alt="crates.io" />
  </a>
  <img src="https://img.shields.io/badge/rustc-1.88+-93450a.svg" alt="MSRV: Rust 1.88+" />
  <a href="https://github.com/nolindnaidoo/pixelactions/blob/main/LICENSE">
    <img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License: MIT" />
  </a>
  <a href="https://pixelactions.dev">
    <img src="https://img.shields.io/badge/web-pixelactions.dev-00A0FF.svg" alt="pixelactions.dev" />
  </a>
</p>

<p align="center">
  <img src="https://github.com/nolindnaidoo/pixelactions/raw/main/docs/assets/demo.gif" alt="pixelactions demo: a chained command clicks a field, types an address, submits, and waits until the confirmation appears — reporting each step as it lands" style="max-width: 100%; height: auto;" />
</p>

[pixelcoords](https://github.com/nolindnaidoo/pixelcoords) freezes your
screen, lets you mark labeled regions, and writes pixel-exact
coordinates with crops, drift re-location, and point verification.
pixelactions reads that session and acts on it — referencing regions by
**label**, never by raw coordinate, so a run survives the UI moving.

```
find  →  act  →  assert
```

No account, no network surface, no daemon — one small native binary that
runs, acts, and exits. MIT-licensed, because the aim was to build the
best executor in this category and give it away.

## Status

**Early.** The loop works end to end on **macOS**, **Windows**, and
**Linux** — both X11 and Wayland (GNOME and KDE): resolve a label to its
click point, re-locate it against a fresh capture, act, and confirm. That
is every platform this tool set out to cover.

## Install

Prebuilt binaries for macOS (arm64 and x86_64), Linux (x86_64) and
Windows (x86_64) are on the
[releases page](https://github.com/nolindnaidoo/pixelactions/releases)
— download, unpack, run. Or build it with cargo:

```bash
cargo install pixelactions
```

Rust 1.88+ for the cargo route. pixelactions drives the pixelcoords
binary for capture-time work — install both:

```bash
cargo install pixelcoords pixelactions
```

Building on Linux needs the xkbcommon headers, because typing on Wayland
means looking a character up in the compositor's own keymap:

```bash
sudo apt-get install -y libxkbcommon-dev pkg-config
```

**The grant, per platform.** `pixelactions doctor --probe` proves what it
can rather than assuming it, and is the right place to answer any prompt —
a dialog appearing partway through an unattended run is worse than a
refusal.

- **macOS** asks for an Accessibility grant on first run. It attaches to
  the terminal that launches pixelactions, not to the binary.
- **Windows** asks for nothing — there is no grant, and nothing to
  install. What it has instead is a limit no permission lifts: **UIPI**.
  A process at medium integrity cannot send input to a window running
  elevated, to the UAC dialog, or to the login screen. Run the target
  unelevated, or run pixelactions elevated too; `doctor` reports which of
  the two you are, so the answer is a fact rather than a warning.
- **Linux/Wayland** asks you to share a screen, once. The grant is
  remembered in `$XDG_STATE_HOME/pixelactions/`, so later runs do not
  prompt. Sharing is not optional: exact pointer placement is measured
  against the region the compositor grants with it.
- **Linux/X11** asks nothing, because X11 has nothing to ask. Any client
  may inject into any other — which is the hole Wayland closes, and worth
  knowing about your own desktop rather than enjoying quietly. Nothing to
  install beyond the build deps above.

Which of the two Linux paths you get is decided from the session at
runtime, not at build time; `doctor` names it.

## Three ways to drive it

One binary, three surfaces, ranked. **Most people want the first.** Here
is the same task in each — fill a field and confirm the result.

### 1. Command line

```bash
pixelactions run --session ~/captures/checkout \
  click:email type:"a@b.com" key:enter verify:success --yes
```

Nothing to install, nothing to keep in sync. Verbs chain in one
invocation, which also means **one** relocation pass for the whole
sequence.

### 2. A flow file

```toml
session = "~/captures/checkout"

[[step]]
action = "click"
target = "email"

[[step]]
action = "type"
text = "a@b.com"

[[step]]
action = "key"
chord = "enter"

[[step]]
action = "verify"
target = "success"
```

```bash
pixelactions plan --flow checkout.toml       # every coordinate, acts on nothing
pixelactions run  --flow checkout.toml --yes
```

Same verbs as the command line. Reviewable in a diff — a pull request
shows *click submit*, not arithmetic.

### 3. The line protocol

```python
ui.send(do="click", target="email")
ui.send(do="type", text=row["email"])
ui.send(do="key", chord="enter")
if ui.send(do="verify", target="success")["outcome"] != "verified":
    failures.append(row)
```

```bash
pixelactions serve --session ~/captures/checkout
```

One long-lived process speaking JSON on stdin/stdout, so **a program in
any language owns the loop** — branching on what's on screen, retrying
with different data, reading a CSV, calling an API between steps. The
client above is forty lines of stdlib Python, in
[docs/PROTOCOL.md](https://github.com/nolindnaidoo/pixelactions/blob/main/docs/PROTOCOL.md).

Escalate on a symptom, not a feature list: one command, then chained
commands, then the protocol when you need loops, branching, and data.

**There is no embedded interpreter, and never will be.** Your bot is
written in your language, which is why this works with all of them
instead of the two we could afford to embed.

## What makes it different

- **It acts where regions are *now*.** Before running, every target is
  re-located against a fresh capture; a region that moved yields
  corrected coordinates, so a session captured last month still works.
- **It refuses rather than guesses.** Every region is re-confirmed
  immediately *before* the step that touches it, and a region that can't
  be found unambiguously stops the run with nothing injected. Ambiguity is
  the test, not distance: a match found in one place is that region
  however far it moved, which is what lets a flow survive a scrolled page.
- **It checks before acting, not after.** Acting on something changes it —
  a focused field grows a caret — so "the region still matches" after a
  click would mean the click did nothing. Outcomes are asserted by naming
  what should have changed.
- **It distinguishes "executed" from "verified".** The OS accepting an
  event is not the app reacting to one, and the report says which
  happened.
- **Waiting is observable, not hopeful.** `wait_for` polls with real
  captures and returns the instant the condition holds. No sleeps, at
  any layer, including the protocol.
- **Grabbing the mouse stops it.** Slam the cursor into a screen corner
  and the run halts before the next step — the one control that works
  while the automation holds your keyboard and the terminal is not
  focused.
- **Exit codes are the API**: 0 done, 1 a step failed, 2 malformed
  question, 3 refused.

## Platform status

| Platform | State |
|----------|-------|
| macOS | Supported — the loop works end to end; primary development platform |
| Windows | Supported — `SendInput` across the whole virtual desktop, kill switch included. One limit: UIPI — see above |
| Linux (X11) | Supported — XTEST in root-window pixels, kill switch included |
| Linux (Wayland) | Supported on GNOME and KDE, via the portal + EIS path. One caveat: no kill switch — see below |

Verified by running the loop, not by reading docs. On Wayland: a region
marked in pixelcoords on GNOME 46, relocated, clicked, and the
application reacted — placement exact, a rectangle dragged at (82, 328)
recorded as (82, 328). On X11: a marked calculator key clicked from an
unfocused window, then `esc`, `×3` and `enter` producing `7×3 = 21` —
which also proves off-layout typing, since `×` is on no US layout. On
Windows 11, a 3440×1440 desktop: placement measured by reading the cursor
back at (0, 0), mid-screen, and (3439, 1439) — the last pixel, which is
exactly the one the `65535 ÷ dimension` off-by-one makes unreachable —
exact at all three; a click landing on a button and the application
reacting; `ö` and `×` typed on a US layout; `ctrl+a`, `ctrl+shift+k` and
the arrow keys arriving as chords rather than as characters.

**Which Linux path you get is a runtime answer**, decided from the
session, because the same binary faces either one. Injecting through
XWayland on a Wayland session would reach X clients only, so the pointer
would travel over native windows that never receive the events — a run
that clicks through some windows and not others while reporting success.
That is why the choice is made once, from `XDG_SESSION_TYPE` and the
socket variables, and a session that cannot be named is refused rather
than guessed at.

**The Wayland caveat, stated plainly.** Wayland exposes no way to ask
where the pointer is — the same isolation that makes injection require
your consent also hides the pointer from other programs. So the corner
kill switch has nothing to watch, and a flow must opt out of it
deliberately:

```toml
[settings]
failsafe = false
```

Nothing is faked to avoid this. A stubbed cursor position would either
sit in a screen corner and abort every run, or disable the check while
appearing to keep it. `doctor` reports whether your compositor could
supply the pointer position through screencast metadata, which is what
lifting this needs.

**X11 has no such caveat**, because X11 will tell you where the pointer
is. The kill switch is armed by default there. What X11 does not have is
a permission model of any kind: any client may inject into any other, so
there is nothing to grant, and `doctor` says so rather than implying a
guard exists.

**Windows has its own limit, and it is the OS's, not this tool's.** UIPI
means a process at medium integrity cannot send input to an elevated
window, the UAC dialog, or the login screen. There is no permission that
lifts it and no workaround here; `doctor` reports whether the process is
elevated so the answer is a fact about your machine. Placement is measured
rather than assumed — but on a single-display machine, so **multi-monitor
and mixed-DPI layouts have not been run on real hardware yet**, only
unit-tested. Reports from a two-screen desk are the most useful thing
anyone could send.

Binaries ship for the platforms that are actually supported — all four
now. Shipping one for a platform that refuses to inject would imply
support a build does not have, which is the rule that kept Windows off the
releases page until this release. This table is kept honest — claims match
runs.

## Non-goals

Settled, so the same debates don't reopen. The full list with reasoning
is in [design/05-NON-GOALS.md](https://github.com/nolindnaidoo/pixelactions/blob/main/design/05-NON-GOALS.md).

- **No embedded interpreter** — not Python, not JS, not Lua. Your bot is
  written in your language and drives this over a pipe, which is why it
  works with every language instead of the two we could afford to embed.
- **No network surface** — no socket, no HTTP, no daemon. This process
  holds the permission to click and type; a listener would lend that to
  anything able to reach it.
- **No scripting language in flow files** — no loops, conditionals, or
  variables. That's the line between a tool and an RPA suite. Anything
  needing branching should be a real program calling the protocol.
- **No recorder.** "Record my clicks and replay them" produces
  unreviewable artifacts that break on the first UI change. Marks come
  from pixelcoords, with a human choosing what matters.
- **Not an accessibility-tree tool, and not a browser automation tool.**
  Those exist and are good. This is for where trees don't reach.

## Documentation

- [pixelactions.dev](https://pixelactions.dev) — the website: the loop, comparisons, how-to
- [docs/CLI.md](https://github.com/nolindnaidoo/pixelactions/blob/main/docs/CLI.md) — commands, chained verbs, the kill switch, exit codes
- [docs/FLOW.md](https://github.com/nolindnaidoo/pixelactions/blob/main/docs/FLOW.md) — the flow file: every step and setting
- [docs/PROTOCOL.md](https://github.com/nolindnaidoo/pixelactions/blob/main/docs/PROTOCOL.md) — the line protocol, with a client in full
- [docs/OUTPUT.md](https://github.com/nolindnaidoo/pixelactions/blob/main/docs/OUTPUT.md) — run, plan, and doctor reports
- [docs/DEVELOPMENT.md](https://github.com/nolindnaidoo/pixelactions/blob/main/docs/DEVELOPMENT.md) — builds, CI gates, releases
- [SKILL.md](https://github.com/nolindnaidoo/pixelactions/blob/main/SKILL.md) — for coding agents driving this tool
- [design/](https://github.com/nolindnaidoo/pixelactions/blob/main/design/README.md) — market research, foundations, decisions, milestones, the two-tool contract
- [CHANGELOG.md](https://github.com/nolindnaidoo/pixelactions/blob/main/CHANGELOG.md) — what changed and why
- [CONTRIBUTING.md](https://github.com/nolindnaidoo/pixelactions/blob/main/CONTRIBUTING.md) — bug reports and pull requests

## Why this exists

Nothing maintained executes desktop input from declarative files with
verification, cross-platform. The near neighbors are Windows-only,
macOS-only, mobile-only, or welded to a VM; the incumbent everyone
actually uses (PyAutoGUI) is unmaintained with no Wayland support; and
computer-use agents shell out to xdotool in containers. Coordinates are
the layer that works where accessibility trees don't exist — canvas
apps, games, streamed desktops, legacy software.

## License

MIT — see [LICENSE](https://github.com/nolindnaidoo/pixelactions/blob/main/LICENSE).

Built by [nolindnaidoo](https://github.com/nolindnaidoo).
