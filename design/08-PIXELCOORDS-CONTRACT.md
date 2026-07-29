# The pixelcoords contract — what each tool owns

Two tools, one loop. The coupling is the point, but it needs a stated
boundary or both roadmaps drift into each other.

## Division of ownership

| Concern | Owner | Why |
|---|---|---|
| Capture, freeze, human marking | **pixelcoords** | It's the whole tool |
| Session schema (`session.json`) | **pixelcoords** | One writer, one definition |
| Geometry, hit-testing, shapes | **pixelcoords core** | Already exists, tested, property-checked |
| Template re-location (`find`) | **pixelcoords** | Already exists; pixelactions calls it |
| Point verification (`assert`) | **pixelcoords** | Same |
| Coordinate → **platform input space** conversion | **pixelactions** | This is an *injection* concern; pixelcoords correctly records physical px + scale and stops there |
| Input synthesis | **pixelactions** | Obviously |
| Flow orchestration, retries, waits, safety | **pixelactions** | Obviously |

**The rule:** pixelactions never reimplements geometry or matching. If
it needs something from that layer, it calls pixelcoords or the fix
belongs in `pixelcoords-core`.

## How pixelactions consumes pixelcoords

Two options, and the decision matters:

- **A. Shell out** to the `pixelcoords` binary (`find --json`,
  `assert --json`). Zero coupling at build time, works with whatever
  version the user installed, matches how everything else in this
  ecosystem composes. Cost: process spawn per verification step, and a
  dependency on the binary being on PATH.
- **B. Depend on `pixelcoords-core` as a crate.** In-process, faster,
  type-safe. Cost: version lockstep, and it drags core's future changes
  into pixelactions' release cycle.

**Decision: A for the loop (find/assert), B for the schema.** Read
`session.json` through `pixelcoords-core`'s types (it's already a
published crate with the schema), but call the *binary* for capture-time
operations, because `find` needs a fresh screen capture — which is
platform work pixelcoords already owns and pixelactions shouldn't
duplicate.

## Minimum schema version

pixelactions pins a minimum `session.json` schema and says so in
`doctor`. Today that's schema 1 with the additive fields (`platform`,
`capture`, `name`). Additive-only remains the rule; a breaking change
bumps schema and pixelactions gates on it.

## What 0.1.0 needs from pixelcoords: nothing new

Shipped v0.1.1 is sufficient. `session.json` + `pixelcoords-core`
(schema types, `click_point`) + `find --json` cover resolution,
relocation, and post-action verification. Start building against it.

## Overlaps with pixelcoords' existing roadmap

Its 0.4/0.5 milestones already contain work that pixelactions depends
on or duplicates. Decide now, not at implementation time:

- **`resolve` (issue #21, filed for this)** — *the seam.* One call
  returning the click point in the space the platform's input API
  wants, optionally relocated. It removes the reassembly (find →
  parse bbox → compute click point → redo DPI conversion) that every
  executor would otherwise perform, and keeps the conversion math in
  the crate that owns it. Nice-to-have, not a blocker: pixelactions can
  do the reassembly itself until it lands.
- **`emit --format json` (issue #15)** — the human/tool-agnostic
  cousin of `resolve`. This is exactly
  "give me the click point in every space, machine-readable." It should
  land as planned and become the documented interchange, so a
  third-party executor could consume it too.
- **`wait` (issue #13)** — pixelcoords polls until a region matches.
  pixelactions' `wait_for` step should **call it**, not reimplement it.
  Its existence in pixelcoords is justified independently (agents want
  it standalone).
- **`diff` (issue #11)** — region comparison. Useful to pixelactions as
  a post-action verification stronger than `assert` (state changed vs
  point-inside). Consume, don't rebuild.
- **Color readout (issue #8)** — recorded per selection. A future
  pixelactions assertion ("the button is still blue/enabled") gets this
  for free. Good reason to keep #8 as specced.
- **Multi-monitor selection (issue #6)** — pixelactions inherits its
  identity-stable monitor matching. Same problem, one solution.

## What pixelactions must NOT push into pixelcoords

- Anything about *acting*. pixelcoords stays a measurement tool; its
  non-goals list already says it never drives the mouse, and that
  boundary is part of why it's trustworthy.
- Flow orchestration, retries, scheduling.
- Agent/MCP surfaces for execution (a read-only MCP for sessions would
  be defensible; an execution one is not).
