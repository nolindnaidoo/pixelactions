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
- actions: `click`, `double_click`, `type`, `key`, `drag`
- `relocate = true` — call pixelcoords `find` first, act on corrected
  coordinates, refuse to act if a region is missing or ambiguous
- `verify` — call pixelcoords `find --label X` after a step (it
  captures; `assert` does not)
- `plan` printing every resolved coordinate and mechanism, acting on
  nothing — permanent, not a phase
- `--json` run report; exit codes 0/1/2/3
- `doctor` — Accessibility grant state, displays, scale factors

**Landed here rather than later**, because each turned out to be small
once the run loop existed: `wait_for` / `wait_gone` polling, bounds
enforcement, the watchdog, the corner kill switch, and all three drive
surfaces (chained argv, flow files, `serve`).

**Deliberately out:** Windows, Linux, MCP, `scroll`.

**Why macOS first:** it's the dev machine, it has the nastiest
coordinate conversion (Retina/scaled/multi-display), and getting it
right proves the coordinate layer — the actual product — before
multiplying platforms.

**Definition of done:** the author uses it for a real repetitive task
for a week without reaching for anything else.

## 0.2.0 — Windows + X11

The two platforms carrying today's demand (CI rigs, Citrix/VDI,
kiosks, and every agent stack shelling out to xdotool in a container).
Adds the multi-monitor normalization work, `MOUSEEVENTF_VIRTUALDESK`,
the `−1` off-by-one, scancode/Unicode dual keyboard paths, and
per-platform `doctor` output.

**Done when:** the same flow file runs unmodified on all three
platforms, and CI proves it on a headed runner where possible.

## 0.3.0 — safety and orchestration

What remains here after the early landings above: an audit log.
Observable polling, bounds enforcement, the watchdog, and the kill
switch shipped in 0.1.0. The kill switch landed as a corner check on
the cursor rather than a listener thread — no background thread, no
extra permission, no global hotkey to conflict with. This is what makes it
trustworthy for unattended runs — the difference between a convenience
and something you'd let an agent drive.

## ~~0.3.5~~ — programmability (shipped in 0.1.0; see 10-PROGRAMMABILITY-SPEC.md)

Argv chaining, `serve` (NDJSON over stdio), and `SKILL.md` all landed
early. Deliberately before the MCP question, because the CLI surface
reaches strictly more agents than a stdio MCP server does.

## 0.4.0 — the agent surface

`pixelactions mcp`: `find` / `act` / `assert` as MCP tools. The research
says agent stacks currently wrap pyautogui and robotjs with no
verification; this is the commercially relevant surface, and it's
worthless before the executor is proven — hence last, not first.

## 0.5.0+ — the Wayland ladder

Portal RemoteDesktop + EIS with a screencast-linked stream (GNOME,
KDE) → `zwlr_virtual_pointer_v1` (wlroots) → uinput for
relative/keyboard with absolute honestly refused. Earned upside, on top
of a tool that already works.

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
