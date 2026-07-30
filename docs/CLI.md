# Command reference

Exit codes are the API everywhere:

| Code | Meaning |
|------|---------|
| 0 | every step executed, and verified where verification was asked for |
| 1 | a step failed honestly — target missing, verification failed, timeout |
| 2 | the question was malformed — bad flow file, missing session, unknown label |
| 3 | refused — permission missing, unsupported platform, screen no longer matches, kill switch tripped, `--yes` absent |

Splitting 3 from 2 matters: "I can't do this here" is operationally
different from "you asked wrong", and a CI job wants to tell them apart.
Splitting 3 from 1 matters for the same reason in the other direction: a
failure may be worth retrying, and a refusal never is.

## The kill switch

Put the cursor in a screen corner and the run stops before its next
step. This is the one control that works while the automation holds the
keyboard and your terminal is not focused — no hotkey to configure, no
extra permission, nothing to remember but the reflex you already have.

It is unambiguous because a flow only ever moves the cursor to a
*marked region's* click point, and nobody marks a region in the dead
corner of a screen. Tune or disable it with `failsafe` and
`failsafe_margin` in [FLOW.md](FLOW.md) — a stopped run reports
`refused` and exits 3.

## Where steps come from

Two spellings, one meaning. A flow file and a chain of argv verbs build
the *same* `Flow`, so resolution, relocation, and verification are
identical either way — and learning one teaches the other.

```bash
pixelactions run --flow signup.toml --yes
pixelactions run --session DIR click:submit type:"hi" key:cmd+s wait:done --yes
```

| Argv verb | Flow action |
|---|---|
| `click:LABEL` | `click` |
| `double:LABEL` | `double_click` |
| `drag:FROM>TO` | `drag` |
| `scroll:LABEL>N` | `scroll` (vertical) |
| `hscroll:LABEL>N` | `scroll` (horizontal) |
| `type:TEXT` | `type` |
| `key:CHORD` | `key` |
| `verify:LABEL` | `verify` |
| `wait:LABEL` | `wait_for` |
| `gone:LABEL` | `wait_gone` |
| `pause:MS` | `pause` |

`scroll:` borrows drag's `>`: `scroll:results>3` goes down, `-3` up.
The amount is required — it is already the least predictable value in
the tool, and defaulting it would hide that.

`type:` keeps everything after the first colon, so
`type:"time: 10:30"` types the whole string. A chain is parsed
all-or-nothing: one bad verb fails the invocation before anything runs.

Chaining exists because a chained run does **one** relocation pass
instead of one per invocation — and because process spawn, while cheap
(~3 ms), is not free.

## `plan`

```
pixelactions plan [--flow FILE | --session DIR VERB:ARG...] [--json]
                  [--space auto|physical|logical]
```

Resolve every step and print what would happen — each coordinate after
conversion, with the monitor and scale it came from. Touches nothing.
This is the permanent dry run, not a temporary phase: seeing the numbers
before anything moves is how a wrong click gets caught.

## `run`

```
pixelactions run [--flow FILE | --session DIR VERB:ARG...] [--json] --yes
```

Perform the flow. Without `--yes` it prints what it would do and exits 3
— injection is never a side effect of a typo.

Order of operations: resolve every label (a missing one fails the whole
flow before any input), re-locate the regions the run will **act on** if
`relocate` is on — not the ones it merely waits for — refuse if any of
them cannot be found unambiguously, then step through: kill switch,
check, act.

**Output arrives as it happens.** Confirming a region is a real screen
capture and template match, so a run takes seconds; each region found and
each step finished prints immediately rather than after the run. `--json`
is the exception: machine output is a single document, so it is written
once at the end. A failed step stops the run and the rest are
recorded as skipped rather than silently dropped.

`--json` emits a run report: per step, the points **actually used**
(corrections included), the outcome, timing, and the failure detail.

## `serve`

```
pixelactions serve --session DIR
```

Speak the line protocol on stdin/stdout — one JSON request per line, one
JSON response back — so a program in any language owns the loop and
pixelactions does the steps. **stdout carries protocol messages only;
logs go to stderr.** Closing stdin ends the session, as does `bye`.

Full reference, including a Python client: [PROTOCOL.md](PROTOCOL.md).

Reach for it when a flow can't express the job: branching on what's on
screen, retrying with different data, reading rows from a CSV, calling an
API between steps. Below that bar, chained argv is simpler and has
nothing to keep in sync.

## `doctor`

```
pixelactions doctor [--json] [--probe]
```

Reports the platform, the coordinate space its input API expects, the
session schema this build understands, the pixelcoords binary it will
call and whether it is new enough, and — on macOS — whether Accessibility
is granted.

On **Linux** it also reports what had to be discovered rather than
assumed, because the same binary faces a different windowing system
depending on which session you logged into. The two servers report
different things, because they have different things to report.

Wayland:

```
platform:        linux
supported:       yes
session:         wayland
input path:      portal RemoteDesktop + EIS
portal:          RemoteDesktop v2 · ScreenCast v5 · devices 0b111
grant:           remembered — no dialog expected
kill switch:     no eyes on Wayland in this build (the compositor could provide them)
native space:    Physical
```

X11:

```
platform:        linux
supported:       yes
session:         x11
input path:      XTEST on the root window
display:         :0 — connected
grant:           none needed — any X client may inject into any other, which is the hole Wayland closes
kill switch:     armed — X11 reports the pointer position, so the corner check works
native space:    Physical
```

`session` is the display server and `input path` is how events would be
sent (`none` if this session has no path at all). After that the lines
diverge:

- **X11** reports `display` — which display was tried and whether it
  answered, since both X11 failure modes are environmental — and no
  portal line at all. In `--json` those fields are **absent** rather than
  zero: a `portal_remote_desktop_version` of `0` would read as "the portal
  answered and said zero", which is a different and untrue thing.

  `supported` on X11 means **a server answered**, not merely that the
  session calls itself X11 — the check connects, because naming a session
  proves nothing about a server being on the other end of `DISPLAY`.
  Connecting costs a local socket round trip, prompts nobody and grants
  nothing, which is the whole X11 security story in one sentence. So `run`
  and `serve` refuse a dead display up front with exit 3 and the reason,
  instead of claiming support and failing later.
- **Wayland** reports `portal`, what xdg-desktop-portal offers
  (`RemoteDesktop` must be v2 or newer for `ConnectToEIS`), and whether a
  remembered screen share means no dialog.

When input is unavailable, `supported` gives the reason instead of a bare
"no", so a session with no display server, or a compositor without the
portal, says which it is.

`run` and `serve` enforce that minimum themselves before doing anything,
exiting 3 with the reason. It is not advisory: below the minimum,
pixelcoords composited the mouse pointer into captures, and since this
tool parks the pointer on whatever it just clicked, relocation failed in a
way that looks like flakiness rather than a version problem.

`--probe` proves input permission instead of assuming it, and is the
right moment to answer a permission prompt: setup time, with a human
present, rather than partway through an unattended run.

On **macOS** it reads the cursor position, moves it one pixel, asks the
OS where it ended up, and puts it back. This exists because a missing
grant makes event posting a **silent** no-op — "the call succeeded"
proves nothing. If the grant is missing, the probe asks macOS for it,
which raises the system dialog and adds the calling application to the
Accessibility list.

On **X11** it does the same thing, for the same reason it can: X11 will
answer where the pointer is. `XSync` alone would prove only that the
server *processed* a fake event, which is not the same as having acted on
it. Both directions are tried, because X11 clamps a move to the screen and
a pointer parked on the right edge cannot go further right:

```
probe:           the cursor moved, and the OS confirmed where it went
```

On **Wayland** it performs the real grant and reports what that
established — a pointer that takes coordinates, and a region to aim
inside — then says plainly that placement is **not confirmed**, because
nothing on Wayland will say where the pointer went:

```
probe:           input was granted and accepted, NOT confirmed
```

The JSON carries this as `moved` and `confirmed` separately: `moved` is
"the platform accepted it", `confirmed` is "the OS proved it". macOS and
X11 set both; Wayland cannot set the second. It is the same distinction a
run report draws between `executed` and `verified` — "nothing errored" is
not "it worked".

Exits 3 when the probe fails outright; an accepted-but-unconfirmed probe
exits 0, because the grant genuinely works.

On macOS, **the grant attaches to the application that launched
pixelactions**, not to the binary — a CLI inherits its terminal's
permission. There will never be a "pixelactions" entry in the
Accessibility list. X11 has no grant to attach to anything.
