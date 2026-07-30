# Milestones — what ships, in what order, and why

The failure mode for a project like this is building the platform
matrix before proving anyone wants the loop. So the order is: **prove
the differentiator on one platform, use it daily, then widen.**

## 0.1.0 — the loop, macOS only

**Nothing in pixelcoords blocks this.** Shipped v0.1.1 already provides
the schema (labels, physical px, per-monitor scale), `click_point` via
`pixelcoords-core`, and `find` for capture-backed relocation and
verification. The roadmap issues are enhancements for later milestones,
not prerequisites — with one exception worth landing when convenient:
[`resolve` (issue #21)](https://github.com/nolindnaidoo/pixelcoords/issues/21),
which collapses find + click-point + space conversion into the single
call an executor actually wants.

**Scope:** perform a flow on macOS against a pixelcoords session, with:

- label → coordinate resolution (physical px → logical points via the
  containing display's scale — the conversion that makes it correct)
- actions: `click`, `double_click`, `type`, `key`, `drag`, `scroll`
- `relocate = true` — call pixelcoords `find` first, act on corrected
  coordinates, refuse to act if a region is missing or ambiguous
- `verify` — call pixelcoords `find --label X` after a step (it
  captures; `assert` does not)
- `plan` printing every resolved coordinate and mechanism, acting on
  nothing — permanent, not a phase
- `--json` run report; exit codes 0/1/2/3
- `doctor` — Accessibility grant state, displays, scale factors

**Landed here rather than later**, because each turned out to be small
once the run loop existed: `wait_for` / `wait_gone` polling, the
watchdog, the corner kill switch, and all three drive surfaces (chained
argv, flow files, `serve`).

**Built, then removed:** bounds enforcement — refusing a relocated point
that landed outside the rect it was marked in. It sounded right and was
measurably wrong: one wheel click moves a page ~80 physical pixels
against a 60px-tall region, so a single scroll locked a label out
permanently while pixelcoords still matched it at score 1.000, reporting
"the match found something else" when it had found exactly the right
thing. It contradicted the headline promise that a session keeps working
as the UI moves. What guards the real risk — a crop matching the *wrong*
instance — is `ambiguous`, which yields no correction and stops the run.

**Deliberately out:** Windows, Linux, MCP.

**Why macOS first:** it's the dev machine, it has the nastiest
coordinate conversion (Retina/scaled/multi-display), and getting it
right proves the coordinate layer — the actual product — before
multiplying platforms.

**Definition of done:** the author uses it for a real repetitive task
for a week without reaching for anything else.

## 0.2.0 — Wayland, rung A

**Reordered, and worth saying why.** This was planned for 0.5.0+, after
Windows and X11, as "earned upside". It landed first instead, because
development moved to a Linux desktop and the platform under the
developer's hands is the one that gets run daily — which is the whole
definition of done for 0.1.0 and applies just as well here. Windows and
X11 keep their scope and shift down a version each; no version is
skipped.

Portal `RemoteDesktop` linked to a `ScreenCast` session, acting over EIS,
on GNOME and KDE. The consent grant is remembered so the dialog happens
at setup time rather than mid-run.

Two things this milestone established that the research could not:

- The absolute-motion region arrives on the **EI device**, not from the
  PipeWire stream, so exact placement needs no PipeWire connection. That
  is what made rung A shippable on its own.
- There is no cursor position to read, so the kill switch is **refused**
  rather than stubbed, and a Wayland flow opts out in writing. Closing
  that gap needs the stream's cursor metadata, which is its own step.

**Done when:** a flow file resolves, relocates, acts and reports on a
Wayland session. Verified by hand on GNOME 46: a marked region clicked
and the application reacted, placement exact to the pixel.

## 0.3.0 — Windows

The platform carrying the most demand (CI rigs, Citrix/VDI, kiosks).
Adds the multi-monitor normalization work, `MOUSEEVENTF_VIRTUALDESK`,
the `−1` off-by-one, scancode/Unicode dual keyboard paths, and
per-platform `doctor` output.

## 0.4.0 — Linux/X11

Every agent stack shelling out to xdotool in a container is doing
coordinate injection on X11 with no relocation and no verification —
exactly the loop this tool closes. XTEST in root-window pixels, off-layout
typing via temporary keymap remapping, and the one platform where a real
display server can run in CI (Xvfb).

**Done when:** the same flow file runs unmodified on macOS, Wayland,
Windows and X11, and CI proves what it can on a headed runner.

## 0.5.0 — safety and orchestration

What remains here after the early landings above: an audit log.
Observable polling, the watchdog, and the kill switch shipped in
0.1.0. The kill switch landed as a corner check on
the cursor rather than a listener thread — no background thread, no
extra permission, no global hotkey to conflict with. This is what makes it
trustworthy for unattended runs — the difference between a convenience
and something you'd let an agent drive.

## ~~0.3.5~~ — programmability (shipped in 0.1.0; see 10-PROGRAMMABILITY-SPEC.md)

Argv chaining, `serve` (NDJSON over stdio), and `SKILL.md` all landed
early. Deliberately before the MCP question, because the CLI surface
reaches strictly more agents than a stdio MCP server does.

## 0.6.0 — the agent surface

`pixelactions mcp`: `find` / `act` / `assert` as MCP tools. The research
says agent stacks currently wrap pyautogui and robotjs with no
verification; this is the commercially relevant surface, and it's
worthless before the executor is proven — hence last, not first.

## Later — the rest of the Wayland ladder

Rung A shipped in 0.2.0. What remains, in order of how much it buys:

- **The kill switch gets eyes.** Consume the granted stream's cursor
  metadata over PipeWire so the corner check works on Wayland and
  `failsafe = false` stops being mandatory. The largest gap in the
  platform today.
- **`zwlr_virtual_pointer_v1`** (sway, Hyprland, river): wlroots
  compositors have uneven portal `RemoteDesktop` coverage but a real
  absolute-pointer protocol with no dialog.
- **uinput**, keyboard and relative only, with absolute **refused** —
  a uinput ABS axis maps to a device range the compositor does not tie to
  screen pixels, so clicking *near* the right place is worse than
  refusing.

## What "valuable" means, concretely

Success is not stars. In order:

1. **The author replaces a manual routine with a flow file** (0.1.0).
   If this doesn't happen, nothing else matters.
2. **Someone else runs a flow the author didn't write** — proves the
   session+flow artifact is portable and legible.
3. **A CI job gates on an exit code** — proves the contract works
   unattended.
4. **An agent stack calls it instead of pyautogui** — proves the
   thesis.
