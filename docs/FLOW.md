# Flow file reference

A flow is a TOML file listing steps that reference a pixelcoords session
**by label**. Labels survive the UI moving; coordinates do not.

```toml
session = "~/Downloads/pixelcoords-captures/20260728-182121-117"

[settings]
relocate = true      # re-locate regions before acting (default)
verify = "each"      # each | end | none
space = "auto"       # auto | physical | logical
settle_ms = 120      # pause around each injected event
timeout_ms = 10000   # how long a wait_* step may poll
poll_ms = 400        # gap between polls; each poll is a screen capture
failsafe = true      # stop if the cursor is slammed into a screen corner
failsafe_margin = 10 # how close to a corner counts

[[step]]
action = "click"
target = "submit"
```

Parsing is strict: an unknown key or action is an error at parse time,
never a silently skipped step.

## Steps

| Action | Fields | What it does |
|--------|--------|--------------|
| `click` | `target` | Move to the region's click point and click |
| `double_click` | `target` | The same, twice, inside the OS double-click window |
| `drag` | `from`, `to` | Press at one region, interpolate motion, release at another |
| `scroll` | `target`, `amount`, `axis?` | Hover a region and turn the wheel over it |
| `type` | `text` | Type literal text through the platform's Unicode path |
| `key` | `chord` | Press a chord, e.g. `cmd+shift+s` |
| `verify` | `target` | Confirm the region still matches its saved crop |
| `wait_for` | `target` | Poll until the region appears |
| `wait_gone` | `target` | Poll until the region disappears |
| `pause` | `ms` | Wait a fixed duration |

**Text versus chords.** `type` uses the layout-independent Unicode path
and *cannot* express shortcuts; `key` uses physical keys and modifiers.
This split is not a style choice — no platform offers one mechanism that
does both. Named keys for chords: `cmd`/`command`/`meta`, `ctrl`, `alt`
/`option`, `shift`, `tab`, `enter`/`return`, `esc`, `space`,
`backspace`, and the four arrows. Anything else must be a single
character.

**Waiting beats sleeping.** The OS accepts an event long before the app
finishes reacting. `settle_ms` exists for the small gaps hardware needs;
when you actually need to know something happened, use `wait_for` — it
polls with real captures and tells the truth.

## Scrolling

```toml
[[step]]
action = "scroll"
target = "results"   # what to hover — a wheel event goes under the cursor
amount = -3          # 15° wheel clicks; positive down/right, negative up/left
axis = "vertical"    # optional; "horizontal" for side-scrolling panes
```

Two things make `scroll` unlike every other step.

**`amount` is the one value in this tool that is not exact.** It counts
wheel clicks, and how far a click travels depends on the reader's own OS
scroll-speed setting. The same flow moves a different distance on a
different machine. Nothing can convert it, the way coordinates are
converted — so do not write flows that depend on landing somewhere
precise.

**A scroll is never verified against its own region.** It changes that
region on purpose, so checking the crop would fail exactly when the step
worked. A scroll always reports `executed`, even under
`verify = "each"`. Confirm it with a `wait_for` on whatever it was
supposed to bring into view:

```toml
[[step]]
action = "scroll"
target = "results"
amount = 3

[[step]]
action = "wait_for"
target = "footer"
```

Because the amount is advisory, the reliable pattern is to scroll *until
something appears* rather than by a fixed distance — which needs a loop,
and a flow file has none by design. That is a job for
[the line protocol](PROTOCOL.md).

## Settings

- **`relocate`** (default `true`) — before acting, ask pixelcoords where
  each region is *now*. Regions that moved yield corrected coordinates;
  regions that cannot be found unambiguously stop the run before
  anything is injected.
- **`verify`** — `each` confirms after every step that touched a region,
  `end` only at the finish, `none` not at all. A `verify` step always
  checks regardless.
- **`space`** — `auto` means what this platform's input API expects:
  logical points on macOS, physical pixels on Windows and X11. Override
  only if you know why.
- **Trusting a correction** is not a setting. A relocated point is acted
  on when pixelcoords found it **unambiguously and above the score
  floor**, and not otherwise. Distance is deliberately not part of that
  test: one wheel click moves a page ~80 physical pixels, so any rule
  tying a correction to its original rect would refuse every scrolled
  UI while the match itself stayed perfect. A crop that matches in more
  than one place produces no correction at all, and stops the run.
- **`failsafe`** (default `true`) — the kill switch. Before every step,
  the cursor is read; if it is within `failsafe_margin` of any screen
  corner, the run stops without injecting that step. Grabbing the mouse
  is what a person does when automation goes wrong, and a corner takes
  no aim — it is the one control that works while the automation holds
  the keyboard and your terminal is not focused.

  It is unambiguous because a flow only ever moves the cursor to a
  *marked region's* click point, and nobody marks a region in the dead
  corner of a screen. If the cursor cannot be read at all, the step
  fails rather than proceeding unchecked — a safety check that silently
  stops evaluating is worse than one that was never claimed.

## Paths

`session` accepts a directory or a `session.json` path, and expands a
leading `~/`.
