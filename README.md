# pixelactions

The executor half of the pixelcoords loop: **consume human-verified
coordinates, perform the interaction, confirm it landed.**

[pixelcoords](https://github.com/nolindnaidoo/pixelcoords) freezes your
screen, lets you mark labeled regions, and writes pixel-exact
coordinates with crops, drift re-location, and point verification.
pixelactions reads that session and acts on it — declaratively, from a
file that references regions by **label**, never by raw coordinate, so
a flow survives the UI moving.

```
find  →  act  →  assert
```

## Status

**Early, and macOS only.** The loop works end to end: resolve a label to
its click point, re-locate it against a fresh capture, act, and confirm.
Windows and X11 are next; nothing here is published yet.

```bash
pixelactions plan flow.toml       # resolve every step, act on nothing
pixelactions run flow.toml --yes  # perform it, verifying each step
pixelactions doctor --probe       # prove input permission, harmlessly
```

```toml
session = "~/Downloads/pixelcoords-captures/20260728-182121-117"

[[step]]
action = "click"
target = "submit"

[[step]]
action = "type"
text = "hello@example.com"

[[step]]
action = "wait_for"
target = "confirmation"
```

## What makes it different

- **It acts where regions are *now*.** Before running, every target is
  re-located against a fresh capture; a region that moved yields
  corrected coordinates, so a session captured last month still works.
- **It refuses rather than guesses.** A region that can't be found
  unambiguously stops the run before anything is injected. A corrected
  point that lands outside its own marked region is refused too — that
  combination means the crop matched something else.
- **It distinguishes "executed" from "verified".** The OS accepting an
  event is not the app reacting to one, and the report says which
  happened.
- **Waiting is observable, not hopeful.** `wait_for` polls with real
  captures instead of sleeping and hoping.
- **Exit codes are the API**: 0 done, 1 a step failed, 2 malformed
  question, 3 refused.

## Documentation

- [docs/FLOW.md](docs/FLOW.md) — the flow file: every step and setting
- [docs/CLI.md](docs/CLI.md) — commands, flags, and the exit-code contract
- [docs/OUTPUT.md](docs/OUTPUT.md) — run, plan, and doctor reports

## Design

The full design set lives in [`design/`](design/README.md): market
research, the input-injection foundations per platform, architecture
decisions, the spec draft, non-goals, milestones, and the contract
between the two tools.

## Why this exists

Nothing maintained executes desktop input from declarative files with
verification, cross-platform. The near neighbors are Windows-only,
macOS-only, mobile-only, or welded to a VM; the incumbent everyone
actually uses (PyAutoGUI) is unmaintained with no Wayland support; and
computer-use agents shell out to xdotool in containers. Coordinates are
the layer that works where accessibility trees don't exist — canvas
apps, games, streamed desktops, legacy software.

MIT. Built by [nolindnaidoo](https://github.com/nolindnaidoo).
