# Risks and the honest verdict

## Is the idea legitimate?

**Yes. The value stands on macOS + Windows + X11 alone; Wayland is
upside, not foundation.** The research found no
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

1. **Substrate pain, not competition.** macOS needs an Accessibility
   grant and can't ship sandboxed; Windows can't touch elevated apps;
   Wayland needs portal consent plus a screencast-linked stream for
   absolute motion. This is real per-platform engineering and it is the
   reason the gap exists — barriers to entry cut both ways.
   *Mitigation: ship the three platforms that carry today's demand
   first, then climb the Wayland ladder. Precedent: pixelcoords' own
   Wayland capture looked impossible until the portal path worked.*
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

## Why Wayland is not the load-bearing question

Every demand signal the research found runs on non-Wayland substrates
today:

- **PyAutoGUI** — the incumbent the whole market actually uses — is
  Win/mac/X11 with zero Wayland, and unmaintained besides. The gap it
  leaves is on the platforms it already covers.
- **Anthropic's computer-use reference** shells out to **xdotool in a
  Docker container** — X11.
- **OSWorld** standardized the research agent action space on
  **pyautogui** — Win/mac/X11.
- **terminator** raised **$2.8M being Windows-only.**
- **CI rigs, kiosks, Citrix/VDI, desktop QA** — overwhelmingly Windows
  and X11.

A tool covering macOS + Windows + X11, with human-verified ground truth
and assert-gated execution, is already better than anything shipping.
Wayland then makes it the *only* option for a growing default-Wayland
Linux desktop — a differentiator to earn, not a gate to pass.

## What would actually make me abandon it

- If **all three** Wayland paths fail AND the mac/Windows/X11
  injection layer proves unreliable in practice — i.e. the core
  promise (exact placement, verified) can't be kept anywhere.
- If a maintained cross-platform equivalent with verification ships
  first, the honest move is to contribute rather than duplicate.

Not on the list: Wayland alone. A three-platform tool that keeps its
promises beats a four-platform tool that doesn't exist.
