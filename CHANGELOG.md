# Changelog

All notable changes are recorded here, written as they land. Versions
follow [Semantic Versioning](https://semver.org). Pre-1.0 policy:
**minor** (0.x.0) for new features and for any breaking change to the
CLI, the flow file, or the line protocol; **patch** (0.x.y) for fixes.
1.0.0 comes when those three are declared stable.

## Unreleased — 0.5.0

**The seam closes.** `design/08-PIXELCOORDS-CONTRACT.md` decided that this
tool should consume pixelcoords' `resolve`, `wait` and `diff` rather than
reimplement them — written while all three were still roadmap items over
there. They shipped, nothing prompted a revisit, and this release is that
revisit.

### The pixelcoords floor moves to 0.7.0

`pixelcoords-core` goes 0.2 → 0.7, and `doctor`'s `MIN_PIXELCOORDS` goes
0.1.2 → 0.7.0. **`doctor` now refuses a pixelcoords older than 0.7.0**,
where before it accepted anything from 0.1.2 up. If you have been running
an older one, that is the one user-visible change here.

Both pins move together because they are one decision: this tool links
the crate *and* shells out to the binary, so a crate that knows about
`resolve` beside a binary that does not is a pairing `doctor` would
otherwise bless and the run loop would then fail on.

**Nothing needed porting.** Five minor versions of pixelcoords cost three
struct literals in test fixtures — `color` and `measures`, both additive.
Schema 1 never moved, and core 0.2 still parses a session 0.7 wrote.
That is the additive-only promise in `design/08` surviving a real test
rather than merely being stated, and it is worth writing down as
evidence.

### `resolve` is pixelcoords' again, and clicks land where it says

The click point, the label lookup, the monitor lookup and the hop from
monitor-local to global coordinates now come from
`pixelcoords_core::resolve`. `design/08` called that the seam and said
why: reassembling it here is how a consumer gets DPI wrong in a way the
crate that owns the geometry cannot. The per-platform units rule — macOS
logical, Windows and X11 physical — is likewise read from
`pixelcoords_core::space` rather than restated here.

**A resolved point is now a whole coordinate, and on macOS that can move
a click by one logical point.** The conversion used to divide in `f64` and
keep the fraction, on the argument that rounding is the enemy of a click
landing where it was aimed. Measured at the boundary that decides
anything, it is the reverse: every injector converts to an integer before
synthesizing, macOS by truncating, so the fraction was never spent — it
was discarded one step later, less accurately. At scale 3.0 a physical
1625 truncated to 1623 where rounding gives 1626: two pixels of error
against one.

The visible effect is that the two halves of the loop stop disagreeing.
Against the same session on a 2x display, `pixelcoords resolve --units
auto` and `pixelactions plan` both now answer `(297, 235)`; before, this
tool said `234`.

Global answers also come from the session's recorded `global_px` instead
of being re-derived as `monitor.origin_px + px.click_point()`. For a
session pixelcoords wrote these agree — it is the same shape, already
translated — and not re-deriving it is the point.

### `wait` and `gone` stop re-searching the whole screen

`wait_for` and `wait_gone` polled by spawning `pixelcoords find` once per
iteration. `find` searches the entire frame for a region's saved crop —
the expensive operation — and each spawn also paid a process start and a
fresh parse of the session.

They now make a single blocking call to `pixelcoords wait`, which scores
each region **where the session recorded it**, in one process, parsing the
session once. `design/08` said this in advance: *"pixelactions' `wait_for`
step should call it, not reimplement it."*

The threshold does not change. `--min-score` is deliberately not passed,
so it stays at pixelcoords' default of 0.9 — the same floor `find` applies
internally.

**A timeout now says whether the region moved.** Scoring in place cannot
see a region that shifted, so "did not match" covers both "never appeared"
and "appeared somewhere else". When the budget runs out, one full-frame
`find` is spent — once, when the answer is already bad — purely to tell
those apart:

```
timed out after 5000ms waiting for "submit" to appear (20 polls, best
match score 0.050) — last look: found (score 0.99) — it is on screen,
(0, -120) physical px from where it was marked, so `wait` was watching
the old position
```

Before, that message could report a score and nothing else. Which of the
two it was is the difference between a user guessing and a user fixing.

## 0.4.0 — 2026-07-30

**Windows**, through `SendInput` across the whole virtual desktop. The
same flow file that runs on macOS, Wayland and X11 runs here, which was
the bar design/07 set for this milestone and closes the platform matrix
this tool set out to cover.

### How it works

- **The pointer does not go through enigo, and that is the whole story of
  this release.** enigo 0.6.1 normalizes an absolute move against
  `SM_CXSCREEN`/`SM_CYSCREEN` — the *primary monitor* — and never sets
  `MOUSEEVENTF_VIRTUALDESK`; its `move_mouse` carries a `TODO` asking
  whether it should. Every coordinate on a secondary display would land on
  the primary one, silently, at a plausible-looking position. So absolute
  motion is written directly on `SendInput` in `win.rs`, with the
  normalization in `pixelactions_core::virtualdesk` where it is tested for
  every pixel on both axes. enigo keeps the keyboard, the buttons and the
  wheel, none of which carry a coordinate.
- **The `− 1` off-by-one, stated as a test rather than a comment.**
  Absolute coordinates run 0..65535 over `dimension − 1`, not `dimension`;
  dividing by the full width leaves the rightmost column and bottom row
  unreachable and every other pixel fractionally short. The rule is pinned
  as a round trip — every pixel of a desktop normalizes and reads back as
  itself — rather than as a handful of examples.
- **Per-monitor DPI awareness is declared at startup**, in `main`, before
  anything asks Windows about a coordinate. Without it Windows virtualizes
  every coordinate it reports and accepts against the primary monitor's
  scale, so a session's physical pixels and this process's idea of a pixel
  would be different quantities on any scaled display. pixelcoords
  declares the same awareness for the same reason; the two must agree, and
  `doctor` reports whether it actually holds.
- **A point off this machine's desktop is refused by name**, never
  clamped. Windows slides an out-of-range absolute event to the nearest
  edge and clicks there, which is the one outcome this tool exists to
  prevent. A session captured on a bigger machine now stops the run with
  the coordinate and the desktop it did not fit in.
- **UIPI is documented, not worked around.** A medium-integrity process
  cannot drive an elevated window, the UAC dialog, or the login screen.
  `doctor` reports whether *this* process is elevated, so the answer is a
  fact about your machine rather than a warning about both cases. No
  UIAccess signing dance; the refusal is the feature.
- **The kill switch is armed**, because Windows answers where the pointer
  is. Wayland remains the only platform carrying that exception.
- **Chords stay one table.** `cmd`/`command`/`meta`/`super` map to
  `Key::Meta`, which is `VK_LWIN` here and `Super_L` on X11, so a chord
  written on a Mac presses the right key on Windows.

### Verified

On Windows 11, a 3440×1440 display, by measurement rather than by a green
test: placement read back at (0, 0), mid-screen and (3439, 1439) — the
last pixel, the one the off-by-one makes unreachable — exact at all three;
a point at (3900, 1450) from a larger machine's session refused with exit
1 instead of clamped; a click landing on a button and the application
reacting; `ö` and `×` typed on a US layout through `KEYEVENTF_UNICODE`;
`ctrl+a`, `ctrl+shift+k` and `left` arriving as chords rather than as
characters.

**Not verified on hardware, and stated plainly:** multi-monitor layouts,
negative desktop origins, and mixed DPI. The author's Windows machine has
one display at (0, 0), where `MOUSEEVENTF_VIRTUALDESK` and a primary-only
mapping are indistinguishable. The arithmetic those cases depend on is
unit-tested against a desktop with a negative origin, but no run on real
hardware backs it yet.

## 0.3.0 — 2026-07-30

**Linux/X11**, through XTEST on the root window. The same flow file that
runs on macOS and Wayland runs here — and unlike Wayland, with no
exceptions: the kill switch works, so `failsafe` stays on.

X11 is where the thesis lands hardest. Every agent stack shelling out to
xdotool in a container is doing coordinate injection on X11 with no
relocation and no verification, which is the loop this tool closes.

Windows moves to 0.4.0. X11 landed first because it is the session the
developer logged into, the same reason Wayland took 0.2.0 ahead of both.

### How it works

- **XTEST in root-window pixels, with no conversion at all.** X11's input
  space *is* the space a session records: one coordinate system covering
  every output, origin at the top-left of the whole screen. With XRandR
  several monitors are one screen, so the session's global `origin_px`
  layout maps straight through, and `Space::Auto` already resolved to
  `Physical` here. A negative coordinate is refused by name — root
  coordinates start at (0, 0), and sending one would clamp the pointer to
  a corner and click there.
- **Typing does not care what your layout is.** A character the active
  keymap cannot reach is bound to a spare keycode for the keystroke and
  unbound afterwards. Verified by typing `×` (U+00D7, on no US layout)
  into a calculator on a US layout. This is the one thing X11 does that
  Wayland cannot: an EI keyboard is welded to the compositor's keymap.
- **No permission model, said out loud.** Any X client may inject into any
  other, so there is nothing to grant and nothing to check — which is
  precisely the hole Wayland closes. `doctor` reports it as the security
  story it is rather than as a convenience.
- **Support means a server answered.** There being nothing to ask
  permission for leaves exactly one question, so the availability check
  connects rather than trusting `XDG_SESSION_TYPE=x11` — naming a session
  says nothing about a server being on the other end of `DISPLAY`. A dead
  display is therefore a refusal up front (exit 3, naming the display and
  the usual causes) rather than a run that reports support and then fails
  while building the injector.

### The kill switch works here

X11 will tell you where the pointer is, so the corner check has something
to watch and `failsafe` needs no opting out. This is the Wayland caveat
resolved rather than repeated:

```
refused   1. click 7
          kill switch: the cursor is in a screen corner (110, 344), so the
          run stopped before this step
```

### doctor

Reports what had to be discovered rather than assumed, and now reports the
right things per server. An X11 session no longer carries Wayland's fields
at all — a portal version of `0` read as "the portal answered and said
zero", which was untrue — and `--probe` performs the same one-pixel proof
macOS does, because X11 answers where the pointer went:

```
session:         x11
input path:      XTEST on the root window
display:         :0 — connected
grant:           none needed — any X client may inject into any other
kill switch:     armed — X11 reports the pointer position
probe:           the cursor moved, and the OS confirmed where it went
```

### CI proves injection for the first time

X11 is the one platform whose display server runs in CI, so a new `xvfb`
job runs `doctor --probe` against a live Xvfb on every push. It is a smoke
test, not a substitute: a headless server is not a desktop, and the claims
above come from manual runs.

### Verified by hand

GNOME 46 on X11 (Ubuntu, 1280x977), against a pixelcoords 0.2.1 session:
a marked calculator key clicked from an unfocused window and the
application reacted; `7`, `esc`, `×3`, `enter` producing `7×3 = 21`; and
the kill switch refusing a step with the pointer position it read back.

## 0.2.0 — 2026-07-30

**Linux/Wayland**, through the sanctioned path: xdg-desktop-portal
`RemoteDesktop` linked to a `ScreenCast` session, acting over EIS. The
same flow file that runs on macOS runs here, with one deliberate
exception noted under the kill switch below.

Wayland forbids cross-client input injection by design, so unlike every
other platform this is a negotiation rather than a call: the user
consents once, and the grant is remembered.

### How it works

- **Consent is asked once, at setup time.** `SelectDevices` is sent with
  `persist_mode = 2` and the `restore_token` from `Start` is stored under
  `$XDG_STATE_HOME/pixelactions/wayland-restore-token`, then replayed on
  later runs. A dialog appearing mid-run would be worse than a refusal —
  a flow half-executes while a human is asked a question they are not
  there to answer — so the prompt belongs to the first
  `doctor --probe`.
- **A linked screen share is mandatory, not optional.** Absolute pointer
  placement is only meaningful inside a region the compositor grants, and
  it derives those regions from the shared streams. Cancelling that half
  of the grant is refused rather than silently degraded to relative
  motion.
- **The region comes from EIS, not from PipeWire.** The shared stream is
  what causes the region to exist, but its geometry arrives on the EI
  device — so exact placement needs no PipeWire connection. Verified
  against GNOME 46, which reports the region and its scale directly.
- **Typing goes through the compositor's own keymap.** There is no
  temporary-remap trick on Wayland the way there is on X11, so a
  character the active layout cannot reach is refused **by name** rather
  than typed as something else.
- **No async runtime**, as everywhere else in this tool: the portal
  handshake uses zbus's blocking API, and `reis`'s core is synchronous.

### The kill switch is refused, not faked

Wayland exposes no way to ask where the pointer is — the same isolation
that makes injection require consent also hides the pointer from other
programs. The corner kill switch therefore has nothing to watch.

This is reported rather than worked around. `failsafe` is on by default,
and a cursor that cannot be read **fails the step** naming
`failsafe = false`, which is the existing, tested behavior. So a flow
run on Wayland must opt out of the kill switch **deliberately**:

```toml
[settings]
failsafe = false
```

Degraded safety is a choice the flow author makes in writing, never a
silent default. A stubbed cursor position was rejected outright: `(0, 0)`
sits in a screen corner and would abort every run, and any other stub
would disable the check while appearing to keep it.

Lifting this needs the shared stream's cursor metadata, which is a
PipeWire connection this release does not open. `doctor` reports whether
the compositor offers that metadata, so the gap is visible.

### Also

- `doctor` reports the display server, the chosen input path, the portal's
  `RemoteDesktop` and `ScreenCast` versions and device types, whether a
  grant is remembered, and whether cursor metadata exists.
- `doctor --probe` distinguishes **accepted** from **confirmed**. On macOS
  the cursor is read back, so a probe is a proof; on Wayland the
  compositor accepts a placement and offers no way to check it, and the
  report says so rather than claiming a proof it does not have. The JSON
  gains a `confirmed` field alongside `moved` — the same distinction the
  run report draws between *executed* and *verified*.
- An X11 session is **refused** rather than half-served. Injecting through
  XWayland would reach X clients only, so the pointer would travel over
  native windows that never receive the events — a run that clicks
  through some windows and not others while reporting success. `plan`
  works on every session type, as always.
- New in `pixelactions-core`: `stream` (placing a physical pixel in a
  granted region, property-tested over mixed DPI, multiple outputs and
  negative origins) and `display` (deciding the display server from the
  environment, tested against every session shape).
- Building on Linux needs `libxkbcommon-dev`.

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

