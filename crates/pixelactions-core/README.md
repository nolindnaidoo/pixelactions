# pixelactions-core

<p align="center">
  <a href="https://crates.io/crates/pixelactions-core"><img src="https://img.shields.io/crates/v/pixelactions-core.svg" alt="crates.io" /></a>
  <a href="https://docs.rs/pixelactions-core"><img src="https://img.shields.io/docsrs/pixelactions-core.svg" alt="docs.rs" /></a>
  <img src="https://img.shields.io/badge/rustc-1.88+-93450a.svg" alt="MSRV: Rust 1.88+" />
  <img src="https://img.shields.io/badge/unsafe-forbidden-success.svg" alt="forbid(unsafe_code)" />
  <a href="https://github.com/nolindnaidoo/pixelactions/blob/main/LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License: MIT" /></a>
</p>

A coordinate is only worth having if something acts on it. pixelactions is
the second half of that loop: it reads a session a human marked in
[pixelcoords](https://github.com/nolindnaidoo/pixelcoords), resolves the
label to a point, performs the interaction, and confirms it landed.

This is its platform-free core: coordinate conversion, flow files, the
planner, the line protocol, the step vocabulary, and the run report — with
no input backend, no OS calls, and `#![forbid(unsafe_code)]`.

**Want the tool?** `cargo install pixelactions`.
**Want to build on it?** This crate — it is what you use to *generate*
flows, *parse* reports, or drive the protocol from Rust.

```toml
[dependencies]
pixelactions-core = "0.9"
```

## Coordinate spaces: the reason this crate exists

A pixelcoords session records **physical pixels**. Input APIs disagree
about what they want:

| Platform | Input API | Speaks |
|---|---|---|
| macOS | `CGEvent` | **logical points**, global space, origin top-left |
| Windows | `SendInput` | **physical pixels**, normalized across the virtual desktop |
| Linux / X11 | `XTEST` | **physical pixels** on the root window |

The same saved coordinate therefore needs a different conversion per
platform, and getting it wrong does not error — it clicks the wrong
place. `Space::Auto` resolves to whatever the current platform needs,
decided in exactly one place (`native_space()`), so no call site guesses.

```rust
use pixelactions_core::convert::{Space, native_space, to_space};
use pixelcoords_core::geometry::{Point, Size};
use pixelcoords_core::session::MonitorRecord;

let monitors = vec![
    // A Retina built-in display at the origin...
    MonitorRecord {
        index: 0,
        name: "Built-in".into(),
        primary: true,
        origin_px: Point::new(0, 0),
        size_px: Size::new(3600, 2338),
        scale: 2.0,
    },
    // ...and a 1x external panel to its left, at negative coordinates.
    MonitorRecord {
        index: 1,
        name: "External".into(),
        primary: false,
        origin_px: Point::new(-3440, 0),
        size_px: Size::new(3440, 1440),
        scale: 1.0,
    },
];

// Physical (850, 440) on the Retina display is logical (425, 220).
let point = to_space(&monitors, 850, 440, Space::Logical).expect("on a monitor");
assert_eq!((point.x, point.y), (425.0, 220.0));
assert_eq!(point.monitor, 0);

// The same physical coordinate on the 1x panel is unchanged, because
// conversion divides by the *containing* monitor's scale — never a
// global one. Mixed-DPI desktops are the normal case, not an edge case.
let external = to_space(&monitors, -2000, 400, Space::Logical).expect("on a monitor");
assert_eq!((external.x, external.y), (-2000.0, 400.0));
assert_eq!(external.monitor, 1);

// A point in the gap between monitors is refused, never clamped —
// clamping would click somewhere plausible and wrong.
assert!(to_space(&monitors, 99_999, 99_999, Space::Logical).is_none());

// What `Auto` means here, decided once:
assert!(matches!(native_space(), Space::Logical | Space::Physical));
```

`monitor_at` answers containment on its own, and `corners` /
`near_screen_corner` back the kill switch — a cursor found in a screen
corner stops a run, because grabbing the mouse is what a person does
when automation goes wrong.

## Resolving labels to points

`plan` turns a flow plus a session into concrete coordinates, or refuses.
It is **total**: every label resolves before any action runs, because a
half-executed flow is the worst outcome this tool can produce.

```rust
use pixelactions_core::convert::Space;
use pixelactions_core::flow::Flow;
use pixelactions_core::plan::{PlanError, plan};
use pixelcoords_core::session::SessionFile;

# fn main() -> Result<(), Box<dyn std::error::Error>> {
let session: SessionFile = serde_json::from_str(EXAMPLE_SESSION)?;
let flow = Flow::parse("session = \"s\"\n\n[[step]]\naction = \"click\"\ntarget = \"submit\"\n")?;

let resolved = plan(&flow, &session, Space::Logical)?;
assert_eq!(resolved.steps[0].points[0].x, 425.0);
assert_eq!(resolved.steps[0].summary, "click submit");

// A missing label fails planning, and the error names what does exist.
let typo = Flow::parse("session = \"s\"\n\n[[step]]\naction = \"click\"\ntarget = \"submti\"\n")?;
let error = plan(&typo, &session, Space::Logical).expect_err("unknown label");
assert!(matches!(error, PlanError::UnknownLabel { .. }));
assert!(error.to_string().contains("submit"));
# Ok(())
# }
#
# const EXAMPLE_SESSION: &str = r#"{
#   "schema": 1,
#   "app": { "name": "pixelcoords", "version": "0.7.7" },
#   "created_utc": "2026-07-29T00:00:00Z",
#   "monitors": [
#     { "index": 0, "name": "Built-in", "primary": true,
#       "origin_px": { "x": 0, "y": 0 },
#       "size_px": { "w": 3600, "h": 2338 }, "scale": 2.0 }
#   ],
#   "selections": [
#     { "shape": "rect", "label": "submit", "monitor": 0,
#       "px":        { "x": 800, "y": 400, "w": 100, "h": 80 },
#       "global_px": { "x": 800, "y": 400, "w": 100, "h": 80 },
#       "crop": "submit.png" }
#   ]
# }"#;
```

## Flow files

A flow references regions by **label**, never by raw coordinate. That
indirection is the point: a label survives the UI moving, and a diff
shows intent ("click submit") rather than arithmetic.

```rust
use pixelactions_core::flow::{Flow, Step, Verify};

# fn main() -> Result<(), Box<dyn std::error::Error>> {
let flow = Flow::parse(
    r#"
    session = "~/captures/checkout"

    [settings]
    verify = "each"
    timeout_ms = 30000

    [[step]]
    action = "click"
    target = "email"

    [[step]]
    action = "type"
    text = "a@b.com"

    [[step]]
    action = "wait_for"
    target = "confirmation"
    "#,
)?;

assert_eq!(flow.settings.verify, Verify::Each);
assert_eq!(flow.steps[0], Step::Click { target: "email".into() });

// Every label the flow will touch, for resolving up front.
assert_eq!(flow.targets(), vec!["email", "confirmation"]);
# Ok(())
# }
```

Parsing is **strict**: unknown keys are errors, not silent no-ops, so a
typo fails at parse time instead of skipping a step at run time. (Session
parsing, by contrast, is deliberately tolerant — unknown fields from a
newer pixelcoords are ignored, so the two tools release independently.)

## The same verbs, three ways

Chained argv, flow files, and the line protocol all build the same
`Step`, so learning one teaches the others and none can drift.

```rust
use pixelactions_core::flow::Step;
use pixelactions_core::protocol::{RequestBody, parse_request};
use pixelactions_core::verb::parse_all;

# fn main() -> Result<(), Box<dyn std::error::Error>> {
// Chained argv: `pixelactions run --session DIR click:submit type:"hi"`
let steps = parse_all(["click:submit", "type:hi", "scroll:results>-3"])?;
assert_eq!(steps[0], Step::Click { target: "submit".into() });

// The line protocol: `do` names the same action a flow file's `action`
// does, so `{"do":"click"}` is the wire form of `action = "click"`.
let request = parse_request(r#"{"id":1,"do":"click","target":"submit"}"#)
    .expect("a valid request");
assert_eq!(request.id, Some(1));
assert_eq!(
    request.body,
    RequestBody::Step { step: Step::Click { target: "submit".into() } }
);
# Ok(())
# }
```

Parsing a chain is all-or-nothing by design: a typo in step 7 must not
perform steps 1 through 6 first.

## Speaking the line protocol

`protocol` holds both directions of the wire format, so you can write a
Rust client for `pixelactions serve` — or an entirely different server
that speaks the same thing.

```rust
use pixelactions_core::protocol::{PROTOCOL_VERSION, Response, ResponseBody, supported_verbs};

let welcome = Response {
    id: Some(1),
    body: ResponseBody::Welcome {
        version: PROTOCOL_VERSION,
        verbs: supported_verbs(),
        session: "/captures/checkout".into(),
    },
};

// One JSON object per line, newline included — no embedded newlines, in
// either direction.
let line = welcome.to_line();
assert!(line.ends_with('\n'));
assert_eq!(line.matches('\n').count(), 1);
assert!(line.contains("\"result\":\"welcome\""));
```

The framing rules are the ones LSP, esbuild, and MCP all converged on:
one JSON object per line; **stdout carries protocol only and stderr is
logs**; closing stdin is the graceful shutdown; and a version handshake
first, so the protocol can change later without breaking programs written
against it today.

## Reporting what happened

`report` carries the vocabulary the whole tool reports in — and the
distinctions are the point.

```rust
use pixelactions_core::report::{RunReport, StepOutcome, StepReport};

// "The OS accepted the event" is not "the app reacted to it".
assert_eq!(StepOutcome::Verified.name(), "verified");
assert_eq!(StepOutcome::Executed.name(), "executed");

// And "it did not work" is not "I declined to try": a refusal is never
// worth retrying, so it earns its own exit code.
let refused = RunReport {
    schema: RunReport::SCHEMA,
    session: "/captures/checkout".into(),
    executed: true,
    steps: vec![StepReport {
        index: 0,
        summary: "click submit".into(),
        outcome: StepOutcome::Refused,
        points: Vec::new(),
        detail: Some("kill switch: the cursor is in a screen corner".into()),
        elapsed_ms: 14,
    }],
};
assert_eq!(refused.exit_code(), 3);
```

Exit codes are the API: **0** done · **1** a step failed honestly · **2**
malformed question · **3** refused.

## Modules

| Module | What it owns |
|---|---|
| `flow` | the flow file: steps, settings, parsing — strict, unknown keys refused |
| `verb` | the chained-argv grammar (`click:submit`, `scroll:list>3`) |
| `plan` | resolving labels to points, and refusing when it cannot |
| `convert` | physical ↔ logical, and which one this platform's input API wants |
| `protocol` | the line protocol: requests, responses, the advertised verbs |
| `report` | the run report — what executed, what verified, what failed |
| `audit` | the append-only record of what a run did |

Full API, every item:
**[docs.rs/pixelactions-core](https://docs.rs/pixelactions-core)**

Some modules are `pub` and **not** part of this API — key-chord parsing,
display-server detection, the Wayland screencast space and the Windows
virtual-desktop grid. They are public only because the binary is a
separate crate and can reach nothing else. They carry `#[doc(hidden)]`,
do not appear on docs.rs, and are not covered by this crate's
versioning.

## Testing

| Layer | What it covers |
|---|---|
| Unit tests | every module — **90% line coverage floor per module**, enforced in CI |
| Property tests | the conversion, because a bug there means clicking the wrong place |
| Doctests | every example in this file compiles and runs — the README *is* the crate docs |

Examples here are `include_str!`'d into the crate and run on every push, so
one that stopped compiling would fail CI rather than mislead you.

## Relationship to the other crates

`pixelactions` the binary is this crate plus an input backend: `CGEvent`,
`SendInput`, XTEST, or the Wayland portal. Anything decidable without
touching a device lives here.

`pixelcoords-core` owns the geometry and the session schema; this crate
depends on it and **never reimplements it**. Matching, capture and shapes
belong there. What belongs here is everything about *acting*: the space a
platform's input API expects, the vocabulary of steps, and the report.

## Also by nolindnaidoo

**Rust**

- **[pixelcoords](https://github.com/nolindnaidoo/pixelcoords)** - Mark pixel-exact coordinates machines can use · [pixelcoords.dev](https://pixelcoords.dev)
- **[scrape-le](https://github.com/nolindnaidoo/scrape-le/tree/main/crate)** - Check whether a page is scrapeable before the scraper is written · [crates.io](https://crates.io/crates/scrape-le)

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
