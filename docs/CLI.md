# Command reference

Exit codes are the API everywhere:

| Code | Meaning |
|------|---------|
| 0 | every step executed, and verified where verification was asked for |
| 1 | a step failed honestly — target missing, verification failed, timeout |
| 2 | the question was malformed — bad flow file, missing session, unknown label |
| 3 | refused — permission missing, unsupported platform, screen no longer matches, `--yes` absent |

Splitting 3 from 2 matters: "I can't do this here" is operationally
different from "you asked wrong", and a CI job wants to tell them apart.

## `plan`

```
pixelactions plan <FLOW> [--json] [--space auto|physical|logical]
```

Resolve every step and print what would happen — each coordinate after
conversion, with the monitor and scale it came from. Touches nothing.
This is the permanent dry run, not a temporary phase: seeing the numbers
before anything moves is how a wrong click gets caught.

## `run`

```
pixelactions run <FLOW> [--json] --yes
```

Perform the flow. Without `--yes` it prints what it would do and exits 3
— injection is never a side effect of a typo.

Order of operations: resolve every label (a missing one fails the whole
flow before any input), re-locate regions if `relocate` is on, refuse if
any target cannot be found unambiguously, then step through — bounds
check, act, verify. A failed step stops the run and the rest are
recorded as skipped rather than silently dropped.

`--json` emits a run report: per step, the points **actually used**
(corrections included), the outcome, timing, and the failure detail.

## `doctor`

```
pixelactions doctor [--json] [--probe]
```

Reports the platform, the coordinate space its input API expects, the
session schema this build understands, the pixelcoords binary it will
call, and — on macOS — whether Accessibility is granted.

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
