---
name: pixelactions
description: Perform verified desktop interactions — click, type, chord, drag — against labeled regions captured by pixelcoords. Use when a GUI has no accessibility tree, no API, and no browser automation: canvas apps, games, streamed desktops (Citrix/VDI/VNC), legacy software, and cross-app OS flows.
---

# pixelactions

A CLI that clicks, types, and drags at coordinates a human marked and
verified, then confirms the interaction landed. It is the executor half
of a loop [pixelcoords](https://github.com/nolindnaidoo/pixelcoords)
starts.

## When to reach for this

Use it when there is no better handle on the UI:

- canvas-rendered apps, games, and custom-drawn widgets
- streamed pixels — Citrix, VDI, VNC, remote desktops
- legacy desktop software with no automation surface
- flows that cross application boundaries

**Do not use it for the web.** Playwright and Selenium own that and own
it well. **Do not use it where an accessibility tree exists** — a11y-first
tools are more robust there. Coordinates are the layer for where those
do not reach.

## What you cannot do without a human

You cannot create a session. A session comes from a person running
`pixelcoords` interactively: they freeze the screen, draw regions, and
label them. That human step is the point — it is what makes the
coordinates trustworthy. If no session exists, say so and ask for one;
do not attempt to synthesize coordinates.

## The loop

```bash
# 1. Can this run at all? Platform, permission, pixelcoords, displays.
pixelactions doctor --json

# 2. What is on screen right now, and where?
pixelcoords find --session DIR

# 3. Resolve without acting. Read the coordinates before anything moves.
pixelactions plan --session DIR click:submit

# 4. Act. --yes is mandatory; without it, nothing is injected.
pixelactions run --session DIR click:submit verify:done --yes --json
```

Never skip step 3 on a session you have not acted on before.

## Verbs

Chain as many as needed in one `run`; they are identical to the flow
file's actions and to the protocol's `do` values.

| Verb | Meaning |
|---|---|
| `click:LABEL` | click that region's verified point |
| `double:LABEL` | double-click it |
| `drag:FROM>TO` | press at one region, release at another |
| `scroll:LABEL>N` | hover a region and scroll it; `-N` scrolls the other way, `hscroll:` for sideways |
| `type:TEXT` | type literal text (layout-independent; cannot express shortcuts) |
| `key:CHORD` | press a chord, e.g. `cmd+s`, `ctrl+shift+p` |
| `verify:LABEL` | confirm the region still matches its crop |
| `wait:LABEL` | poll until it appears |
| `gone:LABEL` | poll until it disappears |
| `pause:MS` | fixed wait — a last resort, when nothing observable exists |

**Scroll by feel, verify by sight.** A scroll's amount counts wheel
clicks and depends on the user's OS scroll-speed setting, so it is never
exact. Never scroll a computed distance — scroll a little and check, in a
loop. A scroll always reports `executed`, never `verified`, because it
changes its own region on purpose; confirm it with a separate `verify` or
`wait:` on whatever should now be visible.

**Wait, do not sleep.** `wait:` and `gone:` poll with real screen
captures and return the instant the condition holds. A `pause:` where a
`wait:` would work is the single most common way to write a flaky flow.

## Exit codes — the contract

| Code | Meaning | What to do |
|---|---|---|
| 0 | every step ran, and verified where asked | continue |
| 1 | a step failed honestly — target missing, verification failed, timeout | read `detail`; the UI is not where you think |
| 2 | malformed question — bad flow, missing session, unknown label | fix the request; nothing was attempted |
| 3 | refused — no permission, unsupported platform, screen no longer matches, **kill switch tripped**, `--yes` absent | do not retry; the refusal names the fix |

**3 is not a failure to retry.** It means the tool declined to act, and
the message says why.

## Reading the report

`--json` gives per-step: the points **actually used** (relocation
corrections included), the outcome, timing, and any failure detail.

`outcome` has three values and the difference matters:

- `verified` — an observation step held: `verify:`, `wait:`, or `gone:`.
  Only these assert anything about the screen.
- `executed` — the input was posted. **This is not proof it worked.** The
  OS accepts an event long before an app reacts, and acting steps always
  report this — a click cannot confirm its own outcome, because clicking
  something changes it. To know an action worked, `wait:` for what it was
  supposed to produce.
- `failed` — it ran and did not work; `detail` says why
- `refused` — a guard declined before anything was attempted. Never
  retry this one; read `detail` and stop.

## Coordinate spaces — the thing to get right

The session stores **physical pixels**. Input APIs disagree about what
they want: macOS `CGEvent` speaks logical points, Windows `SendInput`
and X11 `XTEST` speak physical pixels. The default `space = "auto"`
resolves to whatever the current platform needs.

**Leave it on auto.** Overriding it is for diagnosing a specific
mismatch, not for normal use. Conversion divides by the *containing
monitor's* scale, so mixed-DPI multi-monitor layouts work — but only if
you do not second-guess the space.

## Driving it from a program

When you need branching, retries, or data, do not shell out per step —
run `pixelactions serve --session DIR` and speak the line protocol: one
JSON object per line on stdin, one back on stdout. Same verbs.
See [docs/PROTOCOL.md](docs/PROTOCOL.md).

There is no MCP server, deliberately. Every agent with a shell can run
this CLI; a local stdio MCP server is reachable by fewer of them.

## Safety

This tool moves a real human's mouse and keyboard on a real machine.

- Confirm with the user before the first `--yes` run of any session.
- Do not automate credential entry, payments, or destructive UI actions.
- A refusal (exit 3) is the tool working correctly. Report it; do not
  route around it by disabling `relocate` or `failsafe`.
- **The user can stop you by putting the cursor in a screen corner.**
  A step that comes back `refused` with "kill switch" in its detail was
  stopped by a person. Do not restart it; ask what they wanted instead.
