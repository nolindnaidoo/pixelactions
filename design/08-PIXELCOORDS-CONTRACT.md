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
`capture`, `name`, `color`, `measures`). Additive-only remains the rule;
a breaking change bumps schema and pixelactions gates on it.

The rule has now survived a real test. pixelcoords went 0.2 → 0.7 and
schema 1 never moved: core 0.2 still parses a session that 0.7 wrote,
`measures` and every shape kind included, and `find --json` still carries
every field this tool reads under the same name. Additive-only is a
promise that has been kept, not merely stated.

## Minimum *binary* version, and why it moves with the crate

`doctor::MIN_PIXELCOORDS` is **0.7.0**, matching the `pixelcoords-core`
pin. The two are one decision, not two: pixelactions links the crate
*and* shells out to the binary, so a crate that knows about `resolve`
beside a binary that does not is a pairing `doctor` would otherwise
bless and the run loop would then fail on. Move them together or not at
all.

## What 0.1.0 needs from pixelcoords: nothing new

Shipped v0.1.1 is sufficient. `session.json` + `pixelcoords-core`
(schema types, `click_point`) + `find --json` cover resolution,
relocation, and post-action verification. Start building against it.

## Overlaps with pixelcoords' roadmap — all landed

These were written while they were still someone else's roadmap items.
They have all shipped, so the column that matters now is what
pixelactions does about each.

- **`resolve`** — *the seam.* Shipped in pixelcoords 0.3.0. One call
  returning the click point in the space the platform's input API
  wants, optionally relocated. It removes the reassembly (find →
  parse bbox → compute click point → redo DPI conversion) that every
  executor would otherwise perform, and keeps the conversion math in
  the crate that owns it. **Consume it** — pixelactions' own
  reassembly was always the placeholder.
- **`emit --format json`** — did **not** ship, and should not. What
  this asked for was "give me the click point in every space,
  machine-readable"; `resolve --json` is that, and `emit` stayed what
  it is — ready-to-paste code for a named tool, where each format
  *defines* its units. One interchange, not two.
- **`wait`** — shipped in pixelcoords 0.4.0. pixelactions' `wait_for`
  step should **call it**, not reimplement it. Its existence in
  pixelcoords is justified independently (agents want it standalone).
- **`diff`** — shipped in pixelcoords 0.5.0. Useful to pixelactions as
  a post-action verification stronger than `assert` (state changed vs
  point-inside). Consume, don't rebuild.
- **Color readout** — shipped; every selection now carries the colour
  under its click point. The assertion this was kept for ("the button
  is still blue/enabled") still needs something pixelcoords does not
  have: a way to sample the colour *now*. That is a pixelcoords issue
  before it is a pixelactions one, and it is not filed here.
- **Multi-monitor selection** — shipped. pixelactions inherits its
  identity-stable monitor matching. Same problem, one solution.

## What pixelactions must NOT push into pixelcoords

- Anything about *acting*. pixelcoords stays a measurement tool; its
  non-goals list already says it never drives the mouse, and that
  boundary is part of why it's trustworthy.
- Flow orchestration, retries, scheduling.
- Agent/MCP surfaces for execution (a read-only MCP for sessions would
  be defensible; an execution one is not).
