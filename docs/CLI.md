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
flow before any input), re-locate regions if `relocate` is on, refuse if
any target cannot be found unambiguously, then step through — kill
switch, act, verify. A failed step stops the run and the rest are
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

`run` and `serve` enforce that minimum themselves before doing anything,
exiting 3 with the reason. It is not advisory: below the minimum,
pixelcoords composited the mouse pointer into captures, and since this
tool parks the pointer on whatever it just clicked, relocation failed in a
way that looks like flakiness rather than a version problem.

`--probe` proves input permission instead of assuming it: it reads the
cursor position, moves it one pixel, asks the OS where it ended up, and
puts it back. This exists because a missing grant makes event posting a
**silent** no-op — "the call succeeded" proves nothing. If the grant is
missing, the probe asks macOS for it, which raises the system dialog and
adds the calling application to the Accessibility list. Exits 3 when the
probe fails.

**The grant attaches to the application that launched pixelactions**,
not to the binary — a CLI inherits its terminal's permission. There will
never be a "pixelactions" entry in the Accessibility list.
