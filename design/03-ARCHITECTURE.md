# Architecture — decisions and their reasons

Synthesized from `01-MARKET-RESEARCH.md` and `02-TECHNICAL-FOUNDATIONS.md`.
Every decision here is provisional until proven by a spike; the ones
marked **PROVE FIRST** gate the whole project.

## The finding that shapes everything

**"Physical pixels" is not a portable concept.** The research is
unambiguous:

| Platform | Injection API speaks | Conversion needed from a physical pixel |
|---|---|---|
| Windows | physical px, normalized 0–65535 across the virtual desktop | scale to 65535 range using `(dimension − 1)`, require per-monitor-v2 DPI awareness |
| Linux/X11 | physical px on the root window | none |
| macOS | **logical points**, global space, origin top-left | divide by the containing display's `backingScaleFactor`; ambiguous under scaled HiDPI modes |
| Linux/Wayland | **a screencast stream's logical space** | requires an active ScreenCast grant linked to the RemoteDesktop session |

pixelcoords records physical pixels *and* per-monitor `scale` — which is
exactly what makes the conversion possible. **The coordinate layer is
therefore the core of pixelactions, not the injection layer.** Getting
this right on four platforms is the actual product; clicking is the easy
part.

## Platform strategy

- **macOS / Windows / X11:** build on **enigo** (MIT, active, handles
  the Windows DPI-awareness dance and CGEvent plumbing). Do not
  hand-roll these three.
  - **Critical:** enigo's absolute coordinates are physical px on
    Windows/X11 but *logical points* on macOS. Our conversion layer must
    normalize before calling it; naïve pass-through mis-clicks on every
    Retina Mac.
- **Wayland:** own this layer directly (`reis` for libei/EIS + `ashpd`
  for portals). enigo's Wayland path is feature-gated, buggy, and
  GNOME-46+ only — insufficient for a tool whose promise is exact
  placement. Ladder, honest about degradation:
  1. **Portal RemoteDesktop + `ConnectToEIS`**, with a linked ScreenCast
     session for absolute motion (GNOME, KDE). Persist the
     `restore_token` so repeat runs don't re-prompt.
  2. **`zwlr_virtual_pointer_v1`** on wlroots compositors (Sway,
     Hyprland, river) — real absolute motion, no portal dialog.
  3. **uinput** (ydotool-style) — relative motion, buttons, keys only.
     Absolute is broken by design here (no screen-coordinate mapping),
     so we **refuse absolute placement** on this path rather than
     silently landing at 0,0.
- Every path reports which mechanism it used in the run log. A user
  should never wonder why a click landed differently on their machine.

## Text vs chords — two paths, always

- **Text**: the Unicode/text path (`CGEventKeyboardSetUnicodeString`,
  `KEYEVENTF_UNICODE`, EIS text) — layout-independent, no QWERTY
  assumption. On X11 this needs xdotool's temporary-keymap-remap trick.
- **Chords**: physical keycodes + modifier flags. The Unicode path
  cannot express Cmd+C.
- Documented caveats we will not hide: some apps (games, secure fields)
  ignore injected Unicode and re-read keycodes; IME-active targets are
  unpredictable.

## Verification — the differentiator

Low-level injectors fire and forget; **nothing in the market verifies
that an action landed.** Our loop:

```
find (re-locate region, drift-corrected)  →  act  →  assert (state check)
```

- `find` and `assert` already exist in pixelcoords. pixelactions calls
  them rather than reimplementing — one geometry/matching
  implementation, one source of truth.
- Waits are observable-based (poll a region until it matches, with a
  timeout), not sleep-based. Fixed inter-event delays remain only where
  the OS needs them (down→up spacing, drag interpolation).
- A step that cannot be verified says so; it does not claim success.

## Safety model

- **Dry-run is first-class** — print the resolved plan (every coordinate
  after conversion, per monitor) without injecting.
- **Bounds enforcement** — refuse to act outside marked regions unless
  explicitly allowed. Ground truth is the guardrail, not just the target.
- **Kill switch** as a separate listener thread, not pyautogui's
  corner-failsafe (which false-triggers when the tool itself moves the
  mouse). Needs listening permissions distinct from injection: Input
  Monitoring (macOS), `WH_KEYBOARD_LL` (Windows), `XGrabKey` (X11),
  GlobalShortcuts portal (Wayland). Plus a watchdog: halt if a run
  exceeds N events or N seconds.
- **Every run writes an audit log** — timestamp, step, resolved
  coordinate, mechanism, verification result.

## Non-negotiable honesty (permissions are not "setup friction")

The market research is blunt: "single binary, no setup" is *not* honest
on any platform. What we will state up front, in `doctor` and the README:

- macOS needs an **Accessibility** grant; **App Sandbox blocks injection
  entirely**, so distribution is notarized-outside-the-App-Store.
- Windows cannot inject into elevated apps without elevation/UIAccess;
  the UAC secure desktop and login screen are **unreachable — a stated
  non-goal**.
- Wayland requires a **consent dialog**, and absolute motion requires a
  screencast grant. There is no silent path, by design.
- X11 has no permission model at all — which is why Wayland exists.

## PROVE FIRST (spikes, in order)

Ordered by what carries today's demand, not by what's hardest.

1. **The loop**: `find → act → assert` against a real app whose UI moved
   between capture and run. This is the whole differentiator; prove it
   on the dev machine before anything else.
2. **macOS coordinate conversion** on a mixed Retina/non-Retina
   multi-monitor rig, including a scaled HiDPI mode.
3. **Windows multi-monitor normalization** with the `−1` off-by-one and
   a negative-origin secondary display.
4. **X11** — straightforward (physical px, XTEST), and it's the
   substrate agent stacks actually run on today (xdotool in containers).
5. **Wayland ladder**, climbed after the tool is real: portal + EIS +
   screencast-linked absolute on GNOME and KDE →
   `zwlr_virtual_pointer_v1` on wlroots → uinput for
   relative/keyboard-only with absolute honestly refused.

Wayland is a differentiator to earn, not a gate to pass. pixelcoords'
own Wayland capture looked impossible until the portal path worked; the
same patience applies here, and shipping three platforms first is what
funds that patience with real users.
