# Milestones — what ships, in what order, and why

The failure mode for a project like this is building the platform
matrix before proving anyone wants the loop. So the order is: **prove
the differentiator on one platform, use it daily, then widen.**

## 0.1.0 — the loop, macOS only

**Scope:** `pixelactions run flow.toml` on macOS, against a pixelcoords
session, with:

- label → coordinate resolution (physical px → logical points via the
  containing display's scale — the conversion that makes it correct)
- actions: `click`, `type`, `key`, `drag`, `scroll`
- `relocate = true` — call pixelcoords `find` first, act on corrected
  coordinates, refuse to act if a region is missing or ambiguous
- `verify` — call pixelcoords `assert` after a step
- `--dry-run` printing every resolved coordinate and mechanism
- `--json` run report; exit codes 0/1/2/3
- `doctor` — Accessibility grant state, displays, scale factors

**Deliberately out:** Windows, Linux, MCP, `wait_for` polling, bounds
enforcement beyond "target must exist", kill-switch (dry-run and short
flows carry the safety load at this size).

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

`wait_for` (observable polling, not sleeps), bounds enforcement, the
kill-switch listener thread, watchdog timeout, audit log. This is what
makes it trustworthy for unattended runs — the difference between a
convenience and something you'd let an agent drive.

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
