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
bounds = true        # refuse points that leave their marked region
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
- **`bounds`** (default `true`) — refuse a corrected point that lands
  outside its own marked region. That combination means the crop matched
  something else, and acting on it would click an unknown thing.
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
