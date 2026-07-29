# Changelog

All notable changes are recorded here, written as they land. Versions
follow [Semantic Versioning](https://semver.org). Pre-1.0 policy:
**minor** (0.x.0) for new features and for any breaking change to the
CLI, the flow file, or the line protocol; **patch** (0.x.y) for fixes.
1.0.0 comes when those three are declared stable.

## 0.1.0

The first release. The loop works end to end on **macOS**: resolve a
labeled region to its verified point, re-locate it against a fresh
capture, act, and confirm. Windows and X11 are the next milestone.

Requires **pixelcoords 0.1.2 or newer**, enforced before any run — older
captures composite the mouse pointer into the image, which makes
relocation unreliable in a way that presents as flakiness rather than a
version problem. The library dep on
[`pixelcoords-core`](https://crates.io/crates/pixelcoords-core) is at
`0.2`, which is what current pixelcoords ships; only the session-schema
types are used from it, and those did not change between the 0.1 and 0.2
lines.

### The three drive surfaces

One binary, three ways to reach it, all going through the same
`plan → run::execute` path so they cannot drift.

- **Chained argv** — `pixelactions run --session DIR click:submit type:"hi"
  key:cmd+s wait:done --yes`. Cheapest possible programmability: no
  protocol, no daemon, nothing to install. Also does **one** relocation
  pass for the whole sequence instead of one per invocation.
- **Flow files** — the same verbs saved and reviewable in a diff. A pull
  request shows *click submit*, not arithmetic.
- **The `serve` line protocol** — one JSON object per line on stdin and
  stdout, so a program in **any** language owns the loop. The framing is
  the one LSP, esbuild, and MCP all converged on: stdout is protocol only,
  stderr is logs, closing stdin is the graceful shutdown, and a version
  handshake means the protocol can change later without breaking existing
  programs. There is no embedded interpreter and never will be, because
  that is what makes this work for every language instead of the two we
  could afford to embed.

### Verbs

`click`, `double_click`, `type`, `key`, `drag`, `scroll`, `verify`,
`wait_for`, `wait_gone`, `pause`. Argv, flow files, and the protocol all
use the same names — learning one teaches the others.

**Scroll is the one action whose amount is not exact.** It counts 15°
wheel clicks, and how far that travels depends on the reader's OS scroll
speed. Nothing can convert it the way coordinates are converted, so the
docs say so plainly and the argv form refuses a missing or zero amount
rather than pretending. Scroll a little and wait for what should appear.

### Coordinate spaces

The session records physical pixels; input APIs disagree about what they
want. `Space::Auto` resolves to logical points on macOS and physical
pixels on Windows and X11, decided once in `native_space()` so no call
site guesses. Conversion divides by the **containing monitor's** scale,
which is what makes mixed-DPI layouts work.

### Refusing on evidence, not distance

A region is trusted when pixelcoords finds it **unambiguously and above
the score floor** — a match in one place is that region however far it
moved, which is what lets a flow survive a scrolled page. A crop matching
in more than one place yields no correction and stops the run before
anything is injected. An earlier rule refused any relocated point that
left its original rect; it was measurably wrong on hardware (one wheel
click moves a page ~80 physical pixels against a ~60px region) and was
removed rather than tuned.

### Checks are preconditions, not report cards

`verify = "each"` runs immediately **before** each acting step, not after.
Acting on a region changes it — a focused field grows a caret — so
checking after asks the wrong question: it reports failure when the click
worked, and success when the click was swallowed and nothing happened.
Acting steps always report `executed`; outcomes are asserted by naming
what should have changed (`wait_for` what appears, `wait_gone` what
disappears, `verify` another region).

The per-step check also refreshes coordinates mid-run, so a step that
reveals a banner and shifts everything below it does not leave later
steps clicking where things used to be.

### Safety

- **Kill switch.** Before every step the cursor is read and compared to
  every screen corner. Grabbing the mouse and slamming it into a corner
  stops the run — the one control that works while the automation holds
  the keyboard and the terminal is not focused. Corners rather than a
  hotkey because a corner takes no aim and costs no listener thread.
- **Watchdog.** A whole run has a time budget; a wedged flow cannot own
  the machine forever.
- **No network surface, ever.** `serve` speaks only to the process that
  launched it. This process holds the permission to click and type; a
  listener would lend that to anything able to reach it.
- **Refusals are their own outcome.** A guard that declined to act exits
  **3**, distinct from a step that ran and failed (**1**), because a
  failure may be worth retrying and a refusal never is.

### Failures carry their evidence

A timeout reports elapsed time, poll count, the best match score seen,
and a description of the last look. "Not found" without a score was the
complaint that filled pyautogui's issue tracker; a region matching in
three places and a region never appearing produce identical scores and
need different fixes.

### Runs report progress as they go

Each region confirmed before the run and each step as it finishes prints
immediately, rather than a silent terminal followed by a wall of text.
Output is flushed per line so it arrives whether stdout is a terminal or
a pipe. `--json` is the exception: machine output is one document and is
written whole at the end.

### Exit codes

The API: **0** every step ran and verified where asked, **1** a step
failed honestly, **2** the question was malformed, **3** the tool refused
to act (permission missing, unsupported platform, kill switch, region
could not be confirmed, `--yes` absent).

