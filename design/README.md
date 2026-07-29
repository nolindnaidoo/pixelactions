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
| [07-MILESTONES.md](07-MILESTONES.md) | What ships in what order, and what "valuable" means concretely |
| [08-PIXELCOORDS-CONTRACT.md](08-PIXELCOORDS-CONTRACT.md) | Who owns what across the two tools; overlaps with pixelcoords' roadmap |
| [09-PROGRAMMABILITY.md](09-PROGRAMMABILITY.md) | Research: how tools become "a CLI and an engine" — transports, bindings, agent surfaces, docs patterns |
| [10-PROGRAMMABILITY-SPEC.md](10-PROGRAMMABILITY-SPEC.md) | **The decisions**: three surfaces, the line protocol, what we refuse to build |

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

Spike #1 in [03-ARCHITECTURE.md](03-ARCHITECTURE.md): the
`find → act → assert` loop on this machine, against an app whose UI
moved between capture and run. That's the differentiator; the platform
matrix is engineering we know how to do, and Wayland is upside earned
after the tool is real.

Then 0.1.0 per [07-MILESTONES.md](07-MILESTONES.md) — macOS only,
deliberately — and its definition of done is not a release: it's the
author replacing a real manual routine with a flow file and not
reaching for anything else for a week.
