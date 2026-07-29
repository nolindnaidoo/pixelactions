# Spec draft — file format, CLI, exit codes

Provisional. The point of writing it now is to find the contradictions
early, not to freeze it.

## The actions file

A flow is a small declarative file (TOML — matches pixelcoords' config
idiom; a JSON emitter can come later for generators). It **references a
pixelcoords session by label**, never by raw coordinate — that
indirection is what makes flows survive UI drift and stay reviewable in
git.

```toml
session = "~/Downloads/pixelcoords-captures/20260728-182121-117"

# Optional: refuse to run if the screen no longer matches the capture.
[settings]
verify = "each"        # each | end | none
relocate = true        # run find first; act on corrected coordinates
bounds = "strict"      # strict: never act outside a marked region
timeout_ms = 5000      # per-step observable wait

[[step]]
action = "click"
target = "submit"      # a label from session.json

[[step]]
action = "type"
text = "hello@example.com"

[[step]]
action = "key"
chord = "cmd+s"        # physical keycodes + modifiers

[[step]]
action = "drag"
from = "handle"
to = "dropzone"

[[step]]
action = "wait_for"
target = "confirmation"   # poll until the region matches its saved crop

[[step]]
action = "assert"
target = "confirmation"   # verify state; failure fails the run
```

Open questions this format has to answer:
- Do steps need explicit monitor scoping, or is the label enough
  (labels already carry a monitor in session.json)?
- Offsets within a region — `target = "submit"` plus `offset = [10, 4]`?
  Or is "click the region's click point" always right?
- Loops/conditionals: **probably not.** See non-goals — the moment this
  grows control flow it becomes a scripting language, and the market is
  full of those. A flow that needs branching should be driven by a real
  program calling pixelactions per step.
  **Answered:** no, and the `serve` line protocol is how a real program
  does the driving. See `10-PROGRAMMABILITY-SPEC.md`.

## CLI surface (mirrors pixelcoords' shape deliberately)

> **Superseded by what shipped** — see [`docs/CLI.md`](../docs/CLI.md).
> The sketch below is kept as the record of the thinking. What changed:
> `--dry-run` became the `plan` subcommand (a mode you can forget is a
> mode you will forget), the one-shot `do` became chained
> `verb:argument` arguments to `run`, and `serve` was added for
> programmatic drivers.

```
pixelactions run <flow.toml> [--dry-run] [--json] [--log FILE]
pixelactions doctor [--json]      # permissions, compositor, injection path
pixelactions do click --session <dir> --label submit   # one-shot, no file
pixelactions verify <flow.toml>   # resolve + validate without acting
```

- `--dry-run` prints every resolved coordinate (post-conversion, with
  the mechanism named) and exits 0 without injecting.
- `--json` emits a machine-readable run report: per step, the resolved
  coordinate, the mechanism, the verification result, timing.

## Exit codes (the API, per house rule)

| Code | Meaning |
|---|---|
| 0 | every step executed and verified |
| 1 | a step failed honestly — target not found, assert failed, timeout |
| 2 | the question was malformed — bad flow file, missing session, unknown label |
| 3 | refused for safety — permission missing, absolute unsupported on this compositor, bounds violation |

Splitting 3 from 2 matters: "I can't do this here" is operationally
different from "you asked wrong," and a CI job wants to tell them apart.

## The MCP question (deferred, not dismissed)

The market research says agent stacks currently execute clicks through
pyautogui/xdotool wrappers, and that MCP desktop servers are thin. A
`pixelactions mcp` mode exposing `find`/`act`/`assert` as tools is
plausibly the commercially relevant surface. **Decide after the CLI
works** — an MCP server over a broken injector is worthless, and the
CLI is the honest proving ground.
