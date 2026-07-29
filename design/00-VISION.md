# pixelactions — vision (draft)

**Status: design phase. Nothing here is built. Claims in this folder are
intent, not shipped fact — the inverse of the pixelcoords rule, stated
so nobody confuses the two.**

## The thesis

pixelcoords settled the first half of a loop: a human marks regions
once, and machines get pixel-exact, labeled, verifiable coordinates —
with `assert` to score a point and `find` to re-locate regions after
UI drift. But the JSON still ends at a tool we don't control: pyautogui
scripts, xdotool one-liners, hand-rolled glue. The consumer side of the
contract is unowned.

pixelactions is the second half: **a universal executor that consumes
session.json and performs the interactions** — click, type, drag,
scroll, key chords — declaratively, deterministically, and verified.

## The loop that doesn't exist anywhere

```
find  →  act  →  assert
```

Re-locate the region by its saved crop (drift-corrected), perform the
action at the verified coordinate, then assert the expected state.
Every step exits 0/1/2. A failed find refuses to act; a failed assert
reports exactly which step lied. Actions stop being fire-and-forget.

## Who it's for (hypotheses to validate against research)

1. **Computer-use agents** — stochastic planners need a deterministic,
   auditable action layer with guardrails: act only inside human-marked
   regions, verify after every step, abort on drift.
2. **Desktop QA/CI** — coordinate-based end-to-end tests for the apps
   accessibility trees can't reach (canvas, games, Electron quirks,
   remote desktops), runnable headed on real rigs.
3. **The Wayland-stranded** — where xdotool died and nothing universal
   replaced it.
4. **Anyone scripting the same clicks daily** — kiosk resets, data
   entry, legacy apps without APIs.

## Design principles (inherited from pixelcoords)

- Exit codes are the API. JSON everywhere a script would read.
- Honesty over magic: refuse loudly (drift, missing permission,
  unsupported compositor) rather than guess silently.
- One small native binary. No runtime, no daemon unless a platform
  forces one.
- Safety is a feature: dry-run first-class, bounds enforcement (never
  act outside marked regions unless told), kill-switch, human-visible
  action log.
- The session.json contract is the seam: pixelactions consumes it,
  never redefines it.

## Questions the research answered

- **Is the Wayland input path mature enough to promise?** Partially:
  portal RemoteDesktop + libei works on GNOME and KDE (absolute motion
  requires a linked screencast grant), wlroots needs its own protocol,
  uinput can't do absolute at all. Verdict: a laddered, honestly-labeled
  implementation — and **not a gate on the project**, since every
  current demand signal runs on macOS/Windows/X11 today. See
  `06-RISKS-AND-VERDICT.md`.
- **CLI or MCP for agents?** Both, in that order. Agent stacks today
  wrap pyautogui/robotjs with no verification; an MCP surface is
  plausibly the commercially relevant one, but it's worthless over an
  unproven executor. CLI first (`07-MILESTONES.md`).
- **Format?** Standalone flow file referencing session *labels*, never
  raw coordinates — that indirection is what survives UI drift and stays
  reviewable in git (`04-SPEC-DRAFT.md`).
- **Where's the RPA line?** No loops, no conditionals, no recorder —
  written down in `05-NON-GOALS.md` before any code exists.

## The finding that changed the design

**"Physical pixels" is not portable.** Windows and X11 take physical
pixels; macOS CGEvent takes *logical points*; Wayland absolute motion
lives in a screencast stream's space. The coordinate-conversion layer is
therefore the actual product — clicking is the easy part — and
pixelcoords' per-monitor `scale` recording is what makes it tractable.
