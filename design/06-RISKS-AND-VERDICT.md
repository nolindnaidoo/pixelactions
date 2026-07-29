# Risks and the honest verdict

## Is the idea legitimate?

**Yes — with a narrower claim than "universal."** The research found no
maintained, cross-platform, single-binary CLI that executes desktop
input from declarative files with verification. The nearest neighbors
are each missing a leg:

- **terminator** (mediar-ai, $2.8M raised, Rust, YAML flows,
  "deterministic workflows, AI for recovery") — **Windows-only,
  accessibility-tree based**
- **agent-desktop** — Rust single binary, exit codes — **macOS-only**,
  a11y-based
- **Maestro** — beloved declarative flow UX — **mobile/web only**
- **openQA / os-autoinst** — validates our exact data model at
  industrial scale ("needles" = screenshot + labeled regions JSON +
  fuzzy matching + assertions) — **Perl monolith welded to QEMU/VNC**
- **PyAutoGUI** — the incumbent everyone uses — **effectively
  unmaintained, zero Wayland, black screenshots**
- **nut.js** — went proprietary-subscription in 2024; the fork is stale
- **SikuliX** — archived 2026, handed to OculiX; JVM-heavy

So the combination — cross-platform incl. Wayland · single binary ·
declarative files · human-verified coordinate ground truth · template
self-healing · assert-gated execution with exit codes — is genuinely
unoccupied.

## The four real risks

1. **Substrate pain, not competition.** Wayland absolute placement needs
   portal consent + a linked screencast stream; macOS needs Accessibility
   and can't ship sandboxed; Windows can't touch elevated apps. This is
   months of per-platform engineering and the reason the gap exists.
   *Mitigation: spike Wayland first (see architecture). If it can't be
   made honest, narrow the claim before writing product code.*
2. **The a11y-first tide.** Both credible new entrants chose
   accessibility trees *for* determinism; UiPath and Power Automate
   treat vision as fallback. Coordinates+templates must be positioned as
   the layer for where trees don't exist — never as the general answer.
   *Mitigation: the positioning is a first-class deliverable, not
   marketing polish. Say what it isn't (05-NON-GOALS).*
3. **Solo-maintainer economics in this exact niche are documented
   misery**: nut.js gave up publicly, TagUI's sponsor walked, xdotool
   went quiet four years, SikuliX outlived its maintainer's patience.
   *Mitigation: scope small, keep the non-goals hard, and let the
   pixelcoords loop — not feature breadth — be the reason to use it.*
4. **Two tools, one story.** pixelactions is worthless without
   pixelcoords sessions; pixelcoords gets more valuable with an
   executor. That's a virtuous pair but also a coupling risk: the
   session schema becomes a public contract in a stronger sense.
   *Mitigation: schema changes stay additive; pixelactions pins a
   minimum schema version and says so.*

## The tailwind

Computer-use agents are executing clicks today through **xdotool in a
Docker container** (Anthropic's reference), **pyautogui snippets**
(OSWorld standardized on it, so most research agents inherit its
platform holes), or **vendor-specific cloud VMs**. Meanwhile the
research literature is converging on exactly our architecture: LLM for
flexibility, deterministic replay for correctness, with pre-execution
click verification as a named missing primitive. An executor that acts
only on human-verified ground truth and verifies after every step is the
guardrail that literature is asking for.

## What would make me abandon it

Stated now, while it's cheap to be objective:

- If the Wayland spike shows absolute placement can't be made reliable
  across GNOME + KDE + wlroots, the "universal" thesis is dead. A
  Windows/mac/X11 tool is still useful but competes with a crowded
  field on much weaker grounds — reconsider then.
- If a maintained cross-platform equivalent ships first, the honest
  move is to contribute rather than duplicate.
