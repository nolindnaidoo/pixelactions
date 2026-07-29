# pixelactions — design phase

Local design work for the executor half of the pixelcoords loop.
**Nothing is built. Nothing here is a public claim.**

| Doc | What it holds |
|---|---|
| [00-VISION.md](00-VISION.md) | The thesis, the `find → act → assert` loop, audience hypotheses, principles |
| [01-MARKET-RESEARCH.md](01-MARKET-RESEARCH.md) | 2026 landscape, tool-by-tool, the gap, agent-market signals, honest weaknesses |
| [02-TECHNICAL-FOUNDATIONS.md](02-TECHNICAL-FOUNDATIONS.md) | Per-platform injection APIs, permissions, coordinate spaces, the Wayland ladder, 16 known pitfalls |
| [03-ARCHITECTURE.md](03-ARCHITECTURE.md) | Decisions and reasons; the **PROVE FIRST** spike list |
| [04-SPEC-DRAFT.md](04-SPEC-DRAFT.md) | Flow file format, CLI surface, exit-code contract, the MCP question |
| [05-NON-GOALS.md](05-NON-GOALS.md) | What it refuses to become |
| [06-RISKS-AND-VERDICT.md](06-RISKS-AND-VERDICT.md) | Is it legitimate, the four risks, and what would make us abandon it |

## The one-paragraph version

pixelcoords produces human-verified, labeled, pixel-exact coordinates
with drift correction and point assertions. Nothing owns the other half
of that contract: executing interactions from it. The market has
Windows-only, macOS-only, mobile-only, and VM-QA-only answers, plus an
unmaintained Python incumbent with no Wayland support — while
computer-use agents shell out to xdotool in containers. pixelactions is
the missing executor: declarative flows referencing session labels,
acting only on verified coordinates, checking after every step, exiting
with codes a machine can read.

## Next step

Spike #1 in [03-ARCHITECTURE.md](03-ARCHITECTURE.md): Wayland absolute
placement end-to-end on GNOME and KDE. It's the load-bearing unknown —
everything else is engineering we know how to do.
