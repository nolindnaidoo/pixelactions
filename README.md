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

> **Useful?** A star is how other developers find it —
> [★ GitHub](https://github.com/nolindnaidoo/pixelactions) ·
> [pixelactions.dev](https://pixelactions.dev)

A coordinate is only worth having if something acts on it. pixelactions is
the second half of that loop: it reads a session a human marked in
[pixelcoords](https://github.com/nolindnaidoo/pixelcoords), resolves the
label to a point, performs the interaction, and **confirms it landed**.

Actions name regions by label, never by coordinate. Nothing is injected
without `--yes`. Every step reports whether it *executed* or was
*verified* — because an OS accepting a click is not the same as the
application reacting to one.

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

## Sixty seconds

```bash
pixelcoords --out ./login            # mark the fields once, by hand
pixelactions plan --session ./login click:user   # see the coordinate, touch nothing
pixelactions run --session ./login \
  click:user type:"me@example.com" key:tab type:"hunter2" click:submit \
  wait:dashboard --yes
```

`plan` first is the habit worth forming: it prints every coordinate after
conversion, with the monitor it landed on, and moves nothing.

## Commands

| Command | What it does |
|---|---|
| `plan` | Resolve a flow and print what would happen. Touches nothing |
| `run` | Perform it — a flow file, or verbs chained on the command line. Needs `--yes` |
| `serve` | Speak the line protocol on stdin/stdout, so any language can drive it |
| `mcp` | Serve the executor over MCP, so a model can drive it |
| `doctor` | OS support, input permission, displays, and the pixelcoords it calls |

## Verbs

The same twelve everywhere — chained on the command line, in a flow file,
over the protocol, or as MCP steps:

| Verb | Form | What it does |
|---|---|---|
| `click` `double` | `click:LABEL` | Click, or double-click, the region's point |
| `type` | `type:TEXT` | Type text, including characters not on the layout |
| `key` | `key:CHORD` | A chord like `cmd+s` — arriving as keys, not characters |
| `drag` | `drag:FROM>TO` | Press at one region, release at another |
| `scroll` `hscroll` | `scroll:LABEL>N` | Wheel over a region; negative reverses |
| `verify` | `verify:LABEL` | Is the region still what it was? |
| `wait` `gone` | `wait:LABEL` | Block until it matches, or until it disappears |
| `changed` | `changed:LABEL` | Did this region change? The strongest post-action check |
| `pause` | `pause:MS` | Wait a fixed time, when there is genuinely no observable |

Full reference:
**[docs/CLI.md](https://github.com/nolindnaidoo/pixelactions/blob/main/docs/CLI.md)**
· flow files:
**[docs/FLOW.md](https://github.com/nolindnaidoo/pixelactions/blob/main/docs/FLOW.md)**

## Three ways to drive it

| | For | Start at |
|---|---|---|
| **Flow file** | a repeatable script you commit | [docs/FLOW.md](https://github.com/nolindnaidoo/pixelactions/blob/main/docs/FLOW.md) |
| **Line protocol** | your own program, in any language, owning the loop | [docs/PROTOCOL.md](https://github.com/nolindnaidoo/pixelactions/blob/main/docs/PROTOCOL.md) |
| **MCP** | a model driving it, with acting gated behind `--yes` | `pixelactions mcp --help` |

All three run the same planner and executor. A verb behaves identically
whichever way you reach it.

## Things worth knowing early

**Nothing moves without `--yes`.** `run` without it prints what it would
do and exits 3. This is the safety property everything else rests on.

**The kill switch.** Park the pointer in a screen corner and the run stops
before its next step. It works on macOS, Windows and X11. **On Wayland it
cannot** — the protocol will not report the pointer position — so a flow
there must set `failsafe = false` deliberately and in writing, or every
step refuses.

**Exit codes are the API:**

| Code | Meaning |
|---|---|
| 0 | every step executed, and verified where asked |
| 1 | a step failed honestly — target missing, verification failed, timeout |
| 2 | the question was malformed — bad flow, unknown label |
| 3 | refused — no `--yes`, kill switch, permission missing, unsupported platform |

**Executed is not verified.** The OS accepting an event says nothing about
the application reacting. Ask for `verify` or `changed` when it matters.

**One label, one region.** If two selections share a label the run refuses
rather than clicking whichever came first.

## Platform support

| Platform | Input path | Permission | Kill switch |
|---|---|---|---|
| macOS | CGEvent, logical points | Accessibility, granted to the launching terminal | Yes |
| Windows 11 | `SendInput`, whole virtual desktop | none — but **UIPI** blocks elevated targets | Yes |
| Linux (X11) | XTEST, root-window pixels | none — X11 has no permission model | Yes |
| Linux (Wayland) | portal + EIS | screen share, remembered after the first | **No — by protocol** |

Verified by running the loop on real hardware, not by reading docs.
Multi-monitor and mixed-DPI on Windows remain unrun; the open *Hand-verify*
issues carry the checklists and
[CONTRIBUTING.md](https://github.com/nolindnaidoo/pixelactions/blob/main/CONTRIBUTING.md)
the record.

## Testing

| Layer | What it covers |
|---|---|
| Unit + property tests | `pixelactions-core`, **90% line coverage floor per module** |
| Scenario tests | the binary driven against a **real display** on macOS, Windows and Linux every push — everything except input synthesis |
| X11 injection | a genuine synthetic event posted to a live X server and **read back** |
| Manual gates | whether a click reached an *application*, and the permission model — verified by hand, and said so plainly |

262 tests. CI runs fmt, clippy pedantic (`-D warnings`), the suite, MSRV,
`cargo audit`, and a policy job that fails on any inline `#[allow]`.

There is no performance table here on purpose: the timings that matter
belong to the matching pixelcoords does, and they are measured
[there](https://github.com/nolindnaidoo/pixelcoords/blob/main/docs/PERFORMANCE.md).

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

- **[pixelactions.dev](https://pixelactions.dev)** — demo, comparisons, how-to
- [docs/CLI.md](https://github.com/nolindnaidoo/pixelactions/blob/main/docs/CLI.md) — every command, verb, flag, and exit code
- [docs/FLOW.md](https://github.com/nolindnaidoo/pixelactions/blob/main/docs/FLOW.md) — the flow file format, every action and setting
- [docs/PROTOCOL.md](https://github.com/nolindnaidoo/pixelactions/blob/main/docs/PROTOCOL.md) — the line protocol, for driving it from any language
- [docs/OUTPUT.md](https://github.com/nolindnaidoo/pixelactions/blob/main/docs/OUTPUT.md) — the run report schema
- [docs/DEVELOPMENT.md](https://github.com/nolindnaidoo/pixelactions/blob/main/docs/DEVELOPMENT.md) — building, CI gates, tests, releases
- [CHANGELOG.md](https://github.com/nolindnaidoo/pixelactions/blob/main/CHANGELOG.md) — what changed and why
- [SECURITY.md](https://github.com/nolindnaidoo/pixelactions/blob/main/SECURITY.md) — the threat model of a tool that moves your mouse

## Also by nolindnaidoo

**Rust**

- **[pixelcoords](https://github.com/nolindnaidoo/pixelcoords)** - Mark pixel-exact coordinates machines can use · [pixelcoords.dev](https://pixelcoords.dev)

**VS Code Extensions** — every tool in the family, one page: **[letools.dev](https://letools.dev)**

- **[String-LE](https://marketplace.visualstudio.com/items?itemName=nolindnaidoo.string-le)** - Extract string values for i18n from JSON, YAML, CSV, TOML, INI, and .env
- **[Numbers-LE](https://marketplace.visualstudio.com/items?itemName=nolindnaidoo.numbers-le)** - Extract numeric values from JSON, YAML, CSV, TOML, INI, and .env
- **[EnvSync-LE](https://marketplace.visualstudio.com/items?itemName=nolindnaidoo.envsync-le)** - Spot missing keys across your .env files, with a markdown report
- **[Paths-LE](https://marketplace.visualstudio.com/items?itemName=nolindnaidoo.paths-le)** - Extract file paths from JS/TS imports, JSON, HTML, CSS, TOML, CSV, and .env
- **[Secrets-LE](https://marketplace.visualstudio.com/items?itemName=nolindnaidoo.secrets-le)** - Detect and sanitize credentials locally, before you commit
- **[Scrape-LE](https://marketplace.visualstudio.com/items?itemName=nolindnaidoo.scrape-le)** - Check whether a page is scrapeable before you write the scraper
- **[Colors-LE](https://marketplace.visualstudio.com/items?itemName=nolindnaidoo.colors-le)** - Extract and analyze colors from CSS, SCSS, LESS, Stylus, HTML, JS/TS, and SVG
- **[URLs-LE](https://marketplace.visualstudio.com/items?itemName=nolindnaidoo.urls-le)** - Extract URLs from documentation, configs, and code
- **[Regex-LE](https://marketplace.visualstudio.com/items?itemName=nolindnaidoo.regex-le)** - Find, test, and validate the regex patterns in the current file
- **[Dates-LE](https://marketplace.visualstudio.com/items?itemName=nolindnaidoo.dates-le)** - Extract and analyze dates from logs, configs, and code

**Contact Developer** — [GitHub](https://github.com/nolindnaidoo) · [LinkedIn](https://www.linkedin.com/in/nolindnaidoo/)

## License

MIT — see [LICENSE](https://github.com/nolindnaidoo/pixelactions/blob/main/LICENSE).
