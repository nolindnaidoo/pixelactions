# pixelactions

The executor half of the pixelcoords loop: **consume human-verified
coordinates, perform the interaction, confirm it landed.**

[pixelcoords](https://github.com/nolindnaidoo/pixelcoords) freezes your
screen, lets you mark labeled regions, and writes pixel-exact
coordinates with crops, drift re-location, and point verification.
pixelactions reads that session and acts on it — referencing regions by
**label**, never by raw coordinate, so a run survives the UI moving.

```
find  →  act  →  assert
```

## Status

**Early, and macOS only.** The loop works end to end: resolve a label to
its click point, re-locate it against a fresh capture, act, and confirm.
Windows and X11 are next; nothing here is published yet.

## Three ways to drive it

One binary, three surfaces, ranked. **Most people want the first.** Here
is the same task in each — fill a field and confirm the result.

### 1. Command line

```bash
pixelactions run --session ~/captures/checkout \
  click:email type:"a@b.com" key:enter verify:success --yes
```

Nothing to install, nothing to keep in sync. Verbs chain in one
invocation, which also means **one** relocation pass for the whole
sequence.

### 2. A flow file

```toml
session = "~/captures/checkout"

[[step]]
action = "click"
target = "email"

[[step]]
action = "type"
text = "a@b.com"

[[step]]
action = "key"
chord = "enter"

[[step]]
action = "verify"
target = "success"
```

```bash
pixelactions plan --flow checkout.toml       # every coordinate, acts on nothing
pixelactions run  --flow checkout.toml --yes
```

Same verbs as the command line. Reviewable in a diff — a pull request
shows *click submit*, not arithmetic.

### 3. The line protocol

```python
ui.send(do="click", target="email")
ui.send(do="type", text=row["email"])
ui.send(do="key", chord="enter")
if ui.send(do="verify", target="success")["outcome"] != "verified":
    failures.append(row)
```

```bash
pixelactions serve --session ~/captures/checkout
```

One long-lived process speaking JSON on stdin/stdout, so **a program in
any language owns the loop** — branching on what's on screen, retrying
with different data, reading a CSV, calling an API between steps. The
client above is forty lines of stdlib Python, in
[docs/PROTOCOL.md](docs/PROTOCOL.md).

Escalate on a symptom, not a feature list: one command, then chained
commands, then the protocol when you need loops, branching, and data.

**There is no embedded interpreter, and never will be.** Your bot is
written in your language, which is why this works with all of them
instead of the two we could afford to embed.

## What makes it different

- **It acts where regions are *now*.** Before running, every target is
  re-located against a fresh capture; a region that moved yields
  corrected coordinates, so a session captured last month still works.
- **It refuses rather than guesses.** A region that can't be found
  unambiguously stops the run before anything is injected. Ambiguity is
  the test, not distance: a match found in one place is that region
  however far it moved, which is what lets a flow survive a scrolled
  page.
- **It distinguishes "executed" from "verified".** The OS accepting an
  event is not the app reacting to one, and the report says which
  happened.
- **Waiting is observable, not hopeful.** `wait_for` polls with real
  captures and returns the instant the condition holds. No sleeps, at
  any layer, including the protocol.
- **Grabbing the mouse stops it.** Slam the cursor into a screen corner
  and the run halts before the next step — the one control that works
  while the automation holds your keyboard and the terminal is not
  focused.
- **Exit codes are the API**: 0 done, 1 a step failed, 2 malformed
  question, 3 refused.

## Documentation

- [docs/FLOW.md](docs/FLOW.md) — the flow file: every step and setting
- [docs/CLI.md](docs/CLI.md) — commands, chained verbs, exit codes
- [docs/PROTOCOL.md](docs/PROTOCOL.md) — the line protocol, with a client
- [docs/OUTPUT.md](docs/OUTPUT.md) — run, plan, and doctor reports
- [SKILL.md](SKILL.md) — for coding agents driving this tool

## Design

The full design set lives in [`design/`](design/README.md): market
research, the input-injection foundations per platform, architecture
decisions, the spec draft, non-goals, milestones, and the contract
between the two tools.

## Why this exists

Nothing maintained executes desktop input from declarative files with
verification, cross-platform. The near neighbors are Windows-only,
macOS-only, mobile-only, or welded to a VM; the incumbent everyone
actually uses (PyAutoGUI) is unmaintained with no Wayland support; and
computer-use agents shell out to xdotool in containers. Coordinates are
the layer that works where accessibility trees don't exist — canvas
apps, games, streamed desktops, legacy software.

MIT. Built by [nolindnaidoo](https://github.com/nolindnaidoo).
