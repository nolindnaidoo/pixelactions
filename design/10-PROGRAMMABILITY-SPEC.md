# Programmability — the spec

Decisions drawn from `09-PROGRAMMABILITY.md`. This is what gets built.

## The shape

pixelactions is **one binary with three surfaces**, ranked. Most people
want the first.

1. **CLI, chained** — many actions in one invocation. No protocol, no
   daemon, nothing to install.
2. **Flow files** — the same actions, saved and reviewable in git.
3. **Line protocol (`serve`)** — one long-lived process speaking NDJSON
   on stdin/stdout, so a bot in any language owns the loop.

The bot lives in the user's language. **There is no embedded
interpreter, ever.**

## 1. Argv chaining

```
pixelactions run --session <dir> click:submit type:"hello" key:cmd+s wait:done
```

- Each argument is `verb:argument`; verbs match the flow file's actions
  one-for-one, so learning one teaches the other. (tmux's control-mode
  rule: the protocol verbs *are* the CLI verbs.)
- `--yes` is still required to inject; `plan` still resolves without
  acting.
- Chaining exists because spawn cost is real but small (~3 ms on macOS,
  measured; ~26 ms just to start Python), and because a chained run does
  **one** relocation pass instead of N.

Verb grammar:

| Argv form | Flow equivalent |
|---|---|
| `click:LABEL` | `action = "click"` |
| `double:LABEL` | `double_click` |
| `drag:FROM>TO` | `drag` |
| `type:TEXT` | `type` |
| `key:CHORD` | `key` |
| `verify:LABEL` | `verify` |
| `wait:LABEL` | `wait_for` |
| `gone:LABEL` | `wait_gone` |
| `pause:MS` | `pause` |

## 2. Line protocol (`serve`)

```
pixelactions serve --session <dir>
> {"id":1,"do":"click","label":"submit"}
< {"id":1,"outcome":"verified","point":{...},"elapsed_ms":840}
```

Rules, taken from the stdio bindings that already work everywhere (LSP,
esbuild, MCP):

- **One JSON object per line**, no embedded newlines.
- **stdout carries only protocol messages; stderr is for logs.** A
  client must not treat stderr output as failure.
- **Closing stdin is the graceful shutdown.** No PID file, no
  auto-start, no detached daemon — the process is always a child of its
  caller, which is why there is no lifetime to manage and no Gradle-style
  stale-daemon class of bug.
- **Version handshake on first exchange.** A `hello` naming the protocol
  version and the verbs this build supports. ~30 lines that prevent
  every future protocol change from breaking every bot. (ripgrep's
  `--json` shipping without a version field is the cautionary tale.)
- **Request/response, one in flight**, with `id` echoed so concurrency
  can be added later without a breaking change. No async runtime.

**Why persistent at all** — not speed. State: a `wait_for` that polls
100 times is 100 screen captures, and one process reuses the capture
path instead of paying setup 100 times. Plus one relocation pass for the
whole session.

## 3. Agent surface: a skill file, not an MCP server

`SKILL.md` in the repo: when to reach for the tool, the action loop, the
exit-code table, the JSON shapes, and the coordinate-space explanation
(the one thing an agent will otherwise get wrong).

Reasons this beats shipping MCP now:

- **OpenAI's agents cannot reach a local stdio MCP server** — it needs a
  remote URL — while every agent with a shell can run this CLI. The CLI
  surface has strictly wider reach.
- MCP would mean JSON-RPC plumbing plus, realistically, an async runtime
  — against this repo's dependency rules.
- Once `serve` exists and is JSON-RPC-shaped, an MCP adapter is thin.
  Defer, don't foreclose.

## What we deliberately do not build

| Not building | Why |
|---|---|
| Local socket / HTTP server | This tool holds the permission to click and type; a listener lends that to anything that can reach it (the Docker-socket problem). localhost HTTP additionally invites DNS rebinding — MCP's own TypeScript SDK shipped that CVE in 2025. Stdio has no listener, so none of this exists. |
| Embedded Lua/JS/Python interpreter | You end up owning the interpreter. k6 had to fork its JS engine when upstream had one maintainer. |
| PyO3 / napi native modules | ~70-combination build matrix, and on macOS it makes `python3` the holder of the Accessibility grant — so the permission breaks on a Python upgrade. One signed binary is a stable permission identity. |
| A DSL in flow files (loops, conditionals, variables) | That is the language the user's language already is. Flows stay reviewable in a diff. |
| gRPC / protobuf / WebSocket | Local, single client, tiny payloads. A pipe moves gigabytes per second. |
| Published language packages, for now | A 40-line `pixelactions.py` in the README beats a package that must be released in lockstep with the binary (Playwright's version-match pain). |

## Auto-waiting is the default, or it does not exist

The strongest datum in the research: **28,400 Python files import
pyautogui and call `time.sleep`; only ~272 use `minSearchTime`**, the
retry that ships inside `locateOnScreen`. It exists, it is not the
default path, so nobody knows it exists.

Consequences for us:

- Observation verbs retry by default; the instant, non-retrying form
  must look different (Playwright's lesson: make the flaky spelling
  visibly different from the correct one).
- Evaluate at least once even at `timeout = 0`, so a zero timeout is a
  clean probe.
- Return the instant the condition holds — pay actual latency, not
  worst-case.
- **Timeout errors carry evidence**: elapsed, polls, region, best score
  seen. "Not found" without a score is the complaint that made
  pyautogui's issue tracker.

## The gap we own

Neither Anthropic's nor OpenAI's computer-use tools have any assert or
wait-for-condition primitive — only a dumb sleep. Their only
verification is "screenshot again and ask the model." A deterministic
assert with an exit code is strictly better, and it works in a shell
loop, in CI, and inside a sandbox that cannot call an MCP tool.

## Presentation

Copy esbuild's structure: **one set of docs, three tabs (CLI / script /
protocol), CLI as the default tab**, every option documented once rather
than three times. The README states three ways, ranked, with a default
("most people want the CLI"), and shows *the same task* in each surface
so the mapping is legible. Escalation is justified by a symptom, not a
feature list: one command → chained commands → protocol when you need
loops, branching, and data.
