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

**Early. macOS. Resolution only — it does not yet inject input.**

That order is deliberate: a wrong coordinate is a click in the wrong
place, so the resolution is proven before anything moves. `plan` is
real today and will remain the permanent dry-run surface.

```bash
pixelactions plan flow.toml     # resolve every step, act on nothing
pixelactions doctor             # platform, permissions, sister tool
```

```toml
session = "~/Downloads/pixelcoords-captures/20260728-182121-117"

[[step]]
action = "click"
target = "submit"

[[step]]
action = "type"
text = "hello@example.com"
```

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
