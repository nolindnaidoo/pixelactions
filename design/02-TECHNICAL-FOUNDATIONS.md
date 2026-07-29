<!-- Compiled 2026-07-28 from live web research; sources inline. Design
input, not shipped fact. Re-verify crate versions, compositor support, and
portal APIs before relying on any specific claim. -->

# pixelactions — 2026 input-injection foundations research

Scope note: I verified crate/version and platform-API facts against primary sources (Apple docs, MS Learn, freedesktop portal spec, Peter Hutterer's blog, enigo repo). Where a claim is behavior-dependent or I could not pin a hard source, I flag it. The single biggest cross-cutting finding for your design: **"physical-pixel coordinates" is not a portable concept.** Windows `SendInput` (DPI-aware) and X11/XTEST work in physical pixels; macOS CoreGraphics works in *logical points*; Wayland absolute motion works in a *screencast stream's logical space*. Your injector must own a per-platform pixel→native conversion; the sister tool emitting physical pixels is only half the job.

---

## 1. Rust crates — the 2026 landscape

| Crate | Latest | Platforms | Wayland | Verdict |
|---|---|---|---|---|
| **enigo** | 0.6.x (0.6.1 per [libraries.io](https://libraries.io/cargo/enigo), [docs.rs](https://docs.rs/crate/enigo/latest)) | Win, macOS, X11 stable; Wayland + libei experimental (feature-gated) | Yes, but buggy/gated | **Credible foundation** |
| **reis** | 0.6.1 ([crates.io](https://crates.io/crates/reis)) | Linux only (libei/EIS transport) | This IS the Wayland path | **Use directly for Wayland** |
| **rdev** | 0.5.x, original ~unmaintained ([docs.rs](https://docs.rs/rdev/)) | Win, macOS, X11 | No | Listen+inject, dormant |
| **rdevin** | fork-of-fork w/ RustDesk patches ([github](https://github.com/justdeeevin/rdevin)) | Win, macOS, X11 | No | Maintained rdev successor |
| **mouce** | — ([github](https://github.com/daidi/mouce)) | Win, macOS, X11 | No | Small, niche |
| **tfc** (The Fat Controller) | activity ~2022 ([repo](https://rustrepo.com/repo/Kerndog73-The-Fat-Controller)) | Win, macOS, X11 | No | Stale |
| autopilot-rs | dormant (verify on crates.io) | Win, macOS, X11 | No | Avoid — includes bitmap/screen but old |
| inputbot / mouse-keyboard-input / control-craft | niche | partial | uinput (m-k-i) | Not foundations |

**Recommendation: enigo is the credible base for macOS/Windows/X11, and you should not write those three platform layers yourself** — enigo already handles the DPI-awareness dance on Windows and the CGEvent plumbing on macOS. Its API is coordinate-aware: `Coordinate::{Abs,Rel}`, `Direction::{Click,Press,Release}`, `Button`, `Key::Unicode(char)` + `Key::Other`/`raw()` for keycodes, `Mouse`/`Keyboard` traits ([docs.rs](https://docs.rs/enigo/latest/enigo/)). Origin is top-left, +x right/+y down.

**But budget to own the Wayland layer directly on `reis` + `ashpd`.** enigo's Wayland/libei support is explicitly "hidden behind feature flags" because of bugs ([README](https://github.com/enigo-rs/enigo), [github](https://github.com/enigo-rs/enigo)), its libei path only works on **GNOME 46+**, and its plain "wayland" feature uses the `virtual_keyboard`/`input_method` protocols (text/keyboard-centric), not a general absolute-pointer path. For a tool whose entire value proposition is *exact physical-pixel placement on Wayland*, you want direct control of the portal + EIS handshake and the screencast-stream coordinate linkage (see §5), which enigo abstracts away imperfectly.

**Critical enigo caveat for your use case — coordinate-space inconsistency.** enigo's `Abs` coordinates are **physical pixels on Windows** (it temporarily switches the thread to per-monitor DPI awareness) but **logical points on macOS** and **pixels on X11**. So feeding it the sister tool's physical-pixel JSON unmodified will be correct on Windows/X11 and *wrong on any Retina/HiDPI Mac*. You must convert physical→points for macOS before calling enigo (÷ `backingScaleFactor` of the display containing the point), or bypass enigo's Mac path.

---

## 2. macOS — CGEvent recipe

**API core.** `CGEventCreateMouseEvent(src, type, CGPoint, button)` + `CGEventCreateKeyboardEvent(src, keycode, keydown)` → `CGEventPost(tap, event)`. Drags are their own event type (`kCGEventLeftMouseDragged`) posted *while a button-down is held*; a plain move between down/up won't drag. Double-clicks require setting the `kCGMouseEventClickState` field to 2. Posting events back-to-back too fast gets coalesced/dropped — space them.

**Tap location (reliability).** `CGEventPost` takes a tap:
- `kCGHIDEventTap` — lowest level, injects where HID events enter the window server, i.e. as close to "real hardware" as possible. **Use this for general synthesis.**
- `kCGSessionEventTap` — session level; includes remote events ([forum refs](https://developer.apple.com/forums/thread/112081)).
- `kCGAnnotatedSessionEventTap` — session-scoped delivery.
- Targeted delivery: `CGEventPostToPid` / `CGEventPostToPSN` to drive one process.

**Coordinate space — the big gotcha.** CGEvent mouse coordinates are in the **global display coordinate space measured in points (logical), not pixels**, origin at the top-left of the main display; secondary monitors can carry negative coordinates ([CGDisplayBounds docs](https://developer.apple.com/documentation/coregraphics/cgdisplaybounds(_:)?language=objc), [Apple High-Res guide](https://developer.apple.com/library/archive/documentation/GraphicsAnimation/Conceptual/HighResolutionOSX/APIs/APIs.html)). `backingScaleFactor` (1.0 or 2.0, `NSScreen.backingScaleFactor`) maps points→pixels. **To place at a physical pixel:** find the display containing it, then `point = pixel / backingScaleFactor` in that display's space. Under macOS *scaled* HiDPI modes the backing store resolution ≠ native panel resolution, so "physical pixel" is itself ambiguous — decide whether your sister tool means backing-store pixels or panel pixels and document it. `CGWarpMouseCursorPosition` / `CGDisplayMoveCursorToPoint` move the cursor without generating a move event (useful to pre-position, but you still post the click) ([docs](https://developer.apple.com/documentation/coregraphics/cgdisplaymovecursortopoint(_:_:)?language=objc)).

**Permissions — get this exactly right:**
- **Accessibility** (`AXIsProcessTrusted`, System Settings ▸ Privacy & Security ▸ Accessibility) is what gates **posting** synthetic events. This is your required permission ([enigo Permissions.md](https://github.com/enigo-rs/enigo/blob/main/Permissions.md), [hacktricks](https://hacktricks.wiki/en/macos-hardening/macos-security-and-privilege-escalation/macos-security-protections/macos-input-monitoring-screen-capture-accessibility.html)).
- **Input Monitoring** is for *listening* (event taps in listen mode, IOHIDManager) — you need it only for a kill-switch hotkey listener, not for injection.
- **Screen Recording** only if you screenshot to verify landing (§6).
- Kernel gate: `CGXSenderCanSynthesizeEvents()` checks a `hid-control` entitlement; unentitled/untrusted senders get "*Sender is prohibited from synthesizing events*" logged ([objective-see 0x36](https://objective-see.org/blog/blog_0x36.html)).

**Sandbox / notarization.** **App Sandbox blocks the Accessibility API outright** — `AXIsProcessTrusted()` returns false even with hardened runtime + entitlements, so a **Mac App Store build cannot inject via CGEvent** ([Apple forum 707680](https://origin-devforums.apple.com/forums/thread/707680), [810677](https://developer.apple.com/forums/thread/810677)). A **notarized, hardened-runtime CLI/app distributed outside the App Store is the correct distribution model** and works fine once the user grants Accessibility. (Note the asymmetry: *listening* can be done sandboxed via CGEventTap + Input Monitoring, but *injecting* cannot.)

**Keycode vs Unicode text.**
- `CGEventKeyboardSetUnicodeString(event, len, buf)` injects arbitrary characters **layout-independently** — best for typing text ([Apple docs](https://developer.apple.com/documentation/coregraphics/cgevent/keyboardsetunicodestring(stringlength:unicodestring:))). Caveat: *some apps (games, secure fields, Qt in some configs) ignore the Unicode string and re-translate from the virtual keycode + event state* ([Qt forum](https://forum.qt.io/topic/19330/), Apple docs), so it's not 100% universal.
- Virtual keycodes are **layout-dependent** and only the ANSI US layout is defined in Carbon — the same keycode is a different character on Dvorak ([Clipy/Sauce](https://github.com/Clipy/Sauce)). **You must use real keycodes + `CGEventFlags` modifiers for chords** (Cmd+C, Ctrl+Shift+…) — the Unicode-string path cannot express shortcuts.
- Modifiers: set `CGEventSetFlags(event, kCGEventFlagMaskCommand|…)` on the key event; for stubborn apps also post the modifier key's own keyDown/keyUp around the chord and keep flags consistent across the sequence.

---

## 3. Windows — SendInput recipe

**API.** `SendInput(n, INPUT[], sizeof(INPUT))` with `INPUT{type=MOUSE|KEYBOARD, ...}`. Prefer `SendInput` over the legacy `mouse_event`/`keybd_event`.

**Absolute mouse across monitors.** Use `MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK` together. Coordinates are normalized 0–65535 where (0,0)=upper-left, (65535,65535)=lower-right ([MOUSEINPUT docs](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-mouseinput)). Without `VIRTUALDESK` the range maps to the **primary monitor only** — always set it for multi-monitor ([filipvalentin writeup](https://filipvalentin.github.io/blog/2024/08/how-to-simulate-moving-the-mouse-cursor-through-winapi-multimonitor-setup)). Normalize against the virtual-desktop metrics, not a single screen:

```
xVirt = GetSystemMetrics(SM_XVIRTUALSCREEN);  cx = GetSystemMetrics(SM_CXVIRTUALSCREEN);
yVirt = GetSystemMetrics(SM_YVIRTUALSCREEN);  cy = GetSystemMetrics(SM_CYVIRTUALSCREEN);
nx = round( (px - xVirt) * 65535.0 / (cx - 1) );   // note the -1
ny = round( (py - yVirt) * 65535.0 / (cy - 1) );
```

The **`-1` off-by-one is real**: screen coords span 0..(width-1); dividing by full width leaves the rightmost/bottom pixel unreachable and every pixel slightly short ([libuiohook #21](https://github.com/kwhat/libuiohook/issues/21)). Round, don't truncate.

**DPI.** For `GetSystemMetrics`/coordinates to be in true physical pixels, the process must be **Per-Monitor-DPI-Aware v2**; otherwise Windows virtualizes coordinates and you'll be off on scaled monitors. enigo sidesteps this by temporarily switching the calling thread's DPI awareness for queries ([README](https://github.com/enigo-rs/enigo)). If you roll your own, set a PMv2 manifest or call `SetThreadDpiAwarenessContext`.

**UIPI / elevation (hard limit).** User Interface Privilege Isolation prevents a lower-integrity process from injecting into higher-integrity windows — you **cannot drive an elevated app** (Task Manager, an admin window) unless your process is elevated or has **UIAccess** (signed + installed in a secure location like `Program Files`) ([enigo Permissions.md](https://github.com/enigo-rs/enigo/blob/main/Permissions.md), [MS UIAccess policy](https://learn.microsoft.com/en-us/previous-versions/windows/it-pro/windows-10/security/threat-protection/security-policy-settings/user-account-control-only-elevate-uiaccess-applications-that-are-installed-in-secure-locations)). The **UAC secure desktop and the login screen run on a separate desktop/Session-0-isolated surface that `SendInput` cannot reach at all** — document this as a non-goal.

**Keyboard: scan codes vs virtual keys.**
- `KEYEVENTF_SCANCODE` (send `wScan`, hardware scan code) is **more robust**, especially for games / DirectInput / Raw Input, because the VK can change with layout while the scan code is fixed ([KEYBDINPUT docs](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-keybdinput), [cplusplus forum](https://cplusplus.com/forum/windows/77886/)). **Pitfall:** on key-up you must OR `KEYEVENTF_KEYUP | KEYEVENTF_SCANCODE` — dropping the scancode flag on release makes keys "stick" ([gamedev.net](https://www.gamedev.net/forums/topic/581515-)). Extended keys (arrows, right-Ctrl, etc.) need `KEYEVENTF_EXTENDEDKEY`.
- `KEYEVENTF_UNICODE` sends a `VK_PACKET`; `TranslateMessage` turns it into `WM_CHAR` with your Unicode char — **layout-independent text entry** ([KEYBDINPUT docs](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-keybdinput)). Same limitation as macOS: some games/apps read scan codes and ignore `WM_CHAR`, and it cannot express shortcuts. Use Unicode for text, scan/VK+modifier for chords.

---

## 4. Linux X11 — XTEST

`XTestFakeMotionEvent(dpy, screen_number, x, y, delay)`, `XTestFakeButtonEvent`, `XTestFakeKeyEvent(dpy, keycode, is_press, delay)` ([man page](https://linux.die.net/man/3/xtestfakemotionevent)). Notes:
- Coordinates are **pixels** on the root window of `screen_number`; pass `-1` for the pointer's current screen. With XRandR, multiple monitors are one big screen, so use global pixel coords.
- **XTest takes keycodes, not keysyms** — map with `XKeysymToKeycode`. For characters not on the current keymap, **xdotool temporarily remaps a spare keycode**, injects, and restores — replicate this for arbitrary Unicode/layout-independent typing.
- **Must `XFlush`/`XSync` after posting** or events sit in the buffer; xdotool uses `CurrentTime` delay + `XSync(dpy, False)` ([xdo.c](https://github.com/jordansissel/xdotool/blob/main/xdo.c)). `XSync` confirms the *server* processed the fake event, not that the target app handled it.
- **No permission model** — any X client can inject into any other. That's exactly the isolation hole Wayland closes, and why "just use XWayland/XTEST" degrades on Wayland (see below).

---

## 5. Linux Wayland — the hard one

Wayland deliberately forbids arbitrary cross-client input injection. There is no XTEST equivalent you can just call. Three real strategies, ranked by honesty/portability for **exact-pixel** placement:

### Strategy A (sanctioned, recommended): xdg-desktop-portal RemoteDesktop + libei/EIS
Flow ([portal spec](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.RemoteDesktop.html)): `CreateSession` → `SelectDevices` (bitmask KEYBOARD=1, POINTER=2, TOUCHSCREEN=4) → `Start` (raises a **user-consent dialog**) → then either the portal `Notify*` methods **or** `ConnectToEIS` (portal v2+) to obtain an fd you hand to a libei **sender** context. Peter Hutterer's July-2026 writeup: *"libei is a transport layer for logical input events… the EIS implementation [the compositor] is in control of virtually everything — which devices are available, when they can send events"* ([who-t 2026-07](http://who-t.blogspot.com/2026/07/libei-integrations-in-xdg-remotedesktop.html)). Once you `ConnectToEIS`, you must send **exclusively** via EIS — the `Notify*` methods then error ([spec](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.RemoteDesktop.html)).

**The absolute-coordinate crux.** `NotifyPointerMotion` (relative dx/dy) needs no context, but **`NotifyPointerMotionAbsolute` requires a `stream` parameter — a PipeWire node id from a ScreenCast session — and coordinates are in that stream's logical space** ([spec](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.RemoteDesktop.html)). So **to place the pointer at an exact pixel on Wayland you must also open a linked ScreenCast session** and map your physical pixel into the captured stream's logical coordinates. This is the fundamental reason Wayland absolute injection is much harder than X11/Windows, and it means "exact physical pixel" on Wayland is really "exact pixel within a screencast stream you were granted."

**Persistence / UX.** `persist_mode` 0 (none) / 1 (while app runs) / 2 (until revoked); `Start` returns a single-use `restore_token` you replay via `SelectDevices` to avoid re-prompting ([spec](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.RemoteDesktop.html)). Persist the token so a CLI run doesn't dialog every invocation.

**Rust bindings.** `reis` 0.6.1 is a pure-Rust EI/EIS implementation ([crates.io](https://crates.io/crates/reis)); its own docs say the API is *"currently incomplete and subject to change… should probably do more to provide a higher level API"* — usable but you'll write glue. Pair with `ashpd` for the portal D-Bus calls (RemoteDesktop + ScreenCast). libei itself hit **1.0 (stable EI protocol) in June 2023** and is in the portals since **1.17 (mid-2023)**, with session persistence since **portals 1.21.0** ([phoronix](https://www.phoronix.com/news/libei-1.0-Emulated-Input), [who-t](http://who-t.blogspot.com/2026/07/libei-integrations-in-xdg-remotedesktop.html)).

**Compositor support (2026):**
- **GNOME/mutter** — mature; enigo's libei path targets **GNOME 46+**. xdg-desktop-portal-gnome implements RemoteDesktop + ConnectToEIS.
- **KDE Plasma/kwin** — `ConnectToEIS` implemented in xdg-desktop-portal-kde calling kwin's `connectToEIS`; **KDE Connect 26.04+ prefers `ConnectToEIS` on Wayland** ([KDE portal source](https://github.com/KDE/xdg-desktop-portal-kde/blob/master/src/remotedesktop.cpp), [KDE issue #12](https://invent.kde.org/plasma/xdg-desktop-portal-kde/-/issues/12)).
- **wlroots / Sway / Hyprland** — the weak spot. xdg-desktop-portal-wlr historically centers on ScreenCast; RemoteDesktop/libei coverage is uneven and Hyprland runs its own portal/protocol stack ([wlroots #2378](https://github.com/swaywm/wlroots/issues/2378)). Test per-compositor; don't assume the portal path exists.

### Strategy B (universal, coordinate-blind): ydotool / uinput
Creates a virtual input device via `/dev/uinput`, so it works **under any compositor** because it's kernel-level, below Wayland ([ydotool README](https://github.com/ReimuNotMoe/ydotool/blob/master/README.md)). Costs: needs the **`ydotoold` daemon with root or a udev rule granting `/dev/uinput`**; a startup race where the compositor hasn't enumerated the new device yet ([manpage](https://manpages.ubuntu.com/manpages/resolute/en/man1/ydotool.1.html)). **Absolute positioning is effectively broken — `mousemove --absolute` jumps to the top-left regardless of coordinates** ([issue #250](https://github.com/ReimuNotMoe/ydotool/issues/250)), because a uinput ABS axis maps to a device coordinate range the compositor doesn't tie to screen pixels, and there's no cursor-position feedback. **Reliable for relative motion, scroll, buttons, and typing; unusable for exact-pixel placement.** For pixelactions' core promise this is a fallback for keyboard/relative only.

### Strategy C (compositor-specific): wlroots virtual-pointer/keyboard protocols
On the wlroots family, `zwlr_virtual_pointer_v1` supports `motion_absolute` against a named output extent, and `zwp_virtual_keyboard_v1` + `input_method` handle keys — this is what enigo's non-libei "wayland" feature uses (keyboard/text) ([enigo](https://github.com/enigo-rs/enigo)). `virtual_pointer` gives you a real absolute path on Sway/Hyprland/river **without a portal dialog**, but it's wlroots-only and doesn't exist on GNOME/KDE.

**Honest portable Wayland plan:** try portal-RemoteDesktop+libei (with a screencast stream for absolute) first → on wlroots compositors fall back to `zwlr_virtual_pointer` for absolute → last resort ydotool/uinput for relative+keyboard only, clearly telling the user absolute placement is unavailable. And be explicit in docs that **on Wayland, exact-pixel placement requires a user consent grant and, for absolute motion, a screencast session** — there is no silent path, by design.

---

## 6. Cross-cutting concerns

**Timing / synchronization.** No mainstream injector gets a real "the target app processed this" acknowledgment. The OS accepts the event; the destination app's handling is asynchronous and invisible. So tools use **fixed delays**: pyautogui `PAUSE` (0.1s default) after every call, xdotool `--delay`, enigo/ydotool fire-and-forget. X11 `XSync` only confirms the *server* processed the fake event, not the app. Practical pattern for pixelactions: small inter-event delays (down→up, and move steps within a drag so the app registers hover/drag), then, when correctness matters, **poll an observable** (pixel/screenshot/AX element) with a timeout rather than trusting a sleep.

**Text across layouts / IME.** Two distinct paths, and you want both:
- *Text*: use the Unicode/text path (`CGEventKeyboardSetUnicodeString`, `KEYEVENTF_UNICODE`, Wayland/EIS keysym-or-text) — layout-independent, no QWERTY assumption. On X11, emulate this via xdotool-style temporary keymap remapping.
- *Chords/shortcuts*: use **physical keycodes + modifiers** — the Unicode path cannot express Ctrl+C/Cmd+Shift+4.
- **IME (CJK/dead keys)**: synthetic keycodes flow through the IME composition pipeline and may produce composition state rather than final characters; Unicode-string injection can bypass or conflict with the IME. Prefer the text path for literal strings and warn that IME-active targets are unpredictable.

**Safety / abort / kill-switch.** pyautogui's FAILSAFE works by **checking the real cursor position at the start of every call and raising if it's in a screen corner** — it depends on you slamming the *physical* mouse there, and there's a ~0.1s post-call window to do it ([pyautogui docs](https://pyautogui.readthedocs.io/en/latest/)). **This design partially breaks for a tool that is itself moving the mouse**: your synthetic moves can drift into a corner and false-trigger, or fight the user's real mouse. A robust kill-switch is instead a **global hotkey listener on a separate thread**, independent of the injection path — which needs *listening* permissions distinct from injection: Input Monitoring on macOS, a low-level keyboard hook (`WH_KEYBOARD_LL`) on Windows, `XGrabKey` on X11, and the **GlobalShortcuts portal** on Wayland (you can't grab keys freely there either). Belt-and-suspenders: a deadman/watchdog timer that halts a run if it exceeds N events/seconds, plus honoring a hardware ESC. Note the reliability trap that CGEvent taps can be *silently disabled* by the system (timeout / code-signing races) — a listener-based kill-switch must re-enable its tap ([danielraffel 2026-02](https://danielraffel.me/til/2026/02/19/cgevent-taps-and-code-signing-the-silent-disable-race/)).

**Did the action land?** The honest answer: **low-level injectors (xdotool, ydotool, enigo, SendInput, CGEventPost) do not verify — they fire and forget.** Verification lives in a higher layer: Sikuli/pyautogui/computer-use agents **screenshot and image-match/OCR/AX-query** to confirm state changed ([SikuliX](https://www.softwaretestinghelp.com/sikuli-tutorial-part-1/), pyautogui `locateOnScreen`). If pixelactions wants landing confirmation, plan an optional verification layer: capture a region and diff, or query the accessibility tree — which itself needs **Screen Recording on macOS**, a **ScreenCast grant on Wayland**, and AX/UIA permissions. Keep it optional so the core injector stays permission-light.

---

## 7. Gnarliest known pitfalls (build the test matrix around these)

1. **"Physical pixels" is non-portable.** Windows/X11 = physical px; **macOS CGEvent = logical points** (÷ backingScaleFactor, ambiguous under scaled HiDPI); **Wayland absolute = screencast-stream logical space**. Your one JSON coordinate needs three conversions.
2. **enigo's own coordinate inconsistency** (Win physical-px vs macOS points) means naïve reuse mis-places on Retina Macs.
3. **Windows 65535 off-by-one** — divide by `(dimension − 1)` and round, or you never reach the right/bottom edge and drift everywhere.
4. **Forgetting `MOUSEEVENTF_VIRTUALDESK`** — silently maps to the primary monitor; secondary-screen clicks land on the wrong display.
5. **Windows DPI virtualization** — without PMv2 awareness, physical coords are silently rescaled; queries and clicks disagree.
6. **Scancode key-up sticking** — must OR `KEYEVENTF_KEYUP | KEYEVENTF_SCANCODE`; missing extended-key flag on arrows/right-modifiers.
7. **UIPI/elevation & secure desktop** — cannot drive elevated apps without elevation/UIAccess; UAC prompt and login screen are unreachable. Non-goal.
8. **macOS Accessibility ≠ Input Monitoring ≠ Screen Recording** — inject needs Accessibility; getting this wrong = silent no-op with "prohibited from synthesizing events" in logs.
9. **macOS App Sandbox blocks injection entirely** — no Mac App Store distribution; ship notarized-outside-store.
10. **Unicode-string text is ignored by some apps/games** (they re-read the keycode); and it cannot express shortcuts — need a separate keycode+modifier path.
11. **Wayland absolute motion requires a ScreenCast stream** and a user-consent dialog; there is no silent absolute path — the whole premise needs a permission UX and restore-token persistence.
12. **ydotool absolute is broken** (jumps to top-left) and needs root/udev + daemon with an enumeration race — relative/keyboard only.
13. **wlroots/Hyprland portal gaps** — RemoteDesktop/libei coverage is uneven in 2026; plan a `zwlr_virtual_pointer` fallback and per-compositor testing.
14. **X11 XTEST needs XFlush/XSync** or events never flush; and keysym→keycode remapping for off-layout characters.
15. **No landing acknowledgment anywhere** — sleeps are guesses; drags need intermediate move events; fast posts get coalesced/dropped (macOS especially).
16. **Kill-switch conflicts with self-injection** — corner-failsafe false-triggers; a real kill-switch is a separate listener thread needing its own (listening) permissions and, on Wayland, the GlobalShortcuts portal.

**Sources:** [enigo repo](https://github.com/enigo-rs/enigo) · [enigo Permissions.md](https://github.com/enigo-rs/enigo/blob/main/Permissions.md) · [enigo docs.rs](https://docs.rs/enigo/latest/enigo/) · [reis crate](https://crates.io/crates/reis) · [rdevin](https://github.com/justdeeevin/rdevin) · [Apple CGDisplayBounds](https://developer.apple.com/documentation/coregraphics/cgdisplaybounds(_:)?language=objc) · [Apple keyboardSetUnicodeString](https://developer.apple.com/documentation/coregraphics/cgevent/keyboardsetunicodestring(stringlength:unicodestring:)) · [objective-see Synthetic Reality](https://objective-see.org/blog/blog_0x36.html) · [MS MOUSEINPUT](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-mouseinput) · [MS KEYBDINPUT](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-keybdinput) · [libuiohook #21](https://github.com/kwhat/libuiohook/issues/21) · [multimonitor writeup](https://filipvalentin.github.io/blog/2024/08/how-to-simulate-moving-the-mouse-cursor-through-winapi-multimonitor-setup) · [XTestFakeMotionEvent](https://linux.die.net/man/3/xtestfakemotionevent) · [xdotool xdo.c](https://github.com/jordansissel/xdotool/blob/main/xdo.c) · [RemoteDesktop portal spec](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.RemoteDesktop.html) · [who-t libei 2026](http://who-t.blogspot.com/2026/07/libei-integrations-in-xdg-remotedesktop.html) · [phoronix libei 1.0](https://www.phoronix.com/news/libei-1.0-Emulated-Input) · [ydotool](https://github.com/ReimuNotMoe/ydotool/blob/master/README.md) · [ydotool #250](https://github.com/ReimuNotMoe/ydotool/issues/250) · [KDE portal](https://github.com/KDE/xdg-desktop-portal-kde/blob/master/src/remotedesktop.cpp) · [pyautogui docs](https://pyautogui.readthedocs.io/en/latest/) · [SikuliX](https://www.softwaretestinghelp.com/sikuli-tutorial-part-1/) · [CGEvent tap silent-disable](https://danielraffel.me/til/2026/02/19/cgevent-taps-and-code-signing-the-silent-disable-race/)
