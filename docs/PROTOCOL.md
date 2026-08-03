# The line protocol

```
pixelactions serve --session DIR
```

One JSON object per line in, one JSON object per line out. This is how a
program in **any** language drives pixelactions: every language can
spawn a process and write to a pipe, so none of them need FFI, a native
module, or a package released in lockstep with this binary.

The framing is the one LSP, esbuild, and MCP all converged on.

## Rules

- **One JSON object per line.** No embedded newlines, in either
  direction.
- **stdout is protocol only. stderr is logs.** A client must never read
  stderr output as failure.
- **Closing stdin is the graceful shutdown.** There is no daemon, no PID
  file, no auto-start, and no detached process — `serve` is always a
  child of whatever launched it, so there is no lifetime to manage and no
  stale-daemon class of bug.
- **Say `hello` first.** The handshake is what lets this protocol change
  later without breaking programs written against it today.
- **One request in flight at a time.** `id` is echoed on every response,
  so pipelining can be added later without a breaking change.
- **No network surface, ever.** This process holds the permission to
  click and type on your machine; a listener would lend that permission
  to anything that can reach the port.

## When it cannot start

A session that does not exist, or a missing input permission, is
reported the same way as anything else: **one `error` line on stdout**,
then the process exits 3. A startup failure that only reached stderr
would leave a client staring at a closed pipe, and the rule above says
stderr is never failure — so it would have no idea why.

```
{"result":"error","detail":"cannot read /nope/session: No such file or directory (os error 2)"}
```

## Requests

`do` names either a control message or a step. The step vocabulary is
exactly the flow file's `action` names — `{"do":"click"}` is the wire
form of `action = "click"` — so there is one set of verbs to learn.

| Request | Shape |
|---|---|
| handshake | `{"do":"hello","version":1}` — optional `"settings":{…}` |
| click | `{"do":"click","target":"submit"}` |
| double-click | `{"do":"double_click","target":"file"}` |
| type | `{"do":"type","text":"hello@example.com"}` |
| key | `{"do":"key","chord":"cmd+s"}` |
| drag | `{"do":"drag","from":"card","to":"bin"}` |
| scroll | `{"do":"scroll","target":"results","amount":3}` — optional `"axis":"horizontal"` |
| verify | `{"do":"verify","target":"banner"}` |
| changed | `{"do":"changed","target":"panel"}` — optional `"tolerance":2.5` |
| wait for | `{"do":"wait_for","target":"confirmation"}` |
| wait gone | `{"do":"wait_gone","target":"spinner"}` |
| pause | `{"do":"pause","ms":250}` |
| re-locate | `{"do":"relocate"}` |
| end | `{"do":"bye"}` |

`id` is optional on every request and echoed when present.

`settings` on the handshake takes the same fields as a flow file's
`[settings]` table (`relocate`, `verify`, `space`, `settle_ms`,
`timeout_ms`, `poll_ms`, `failsafe`, `failsafe_margin`) and
applies for the whole session.
Unknown keys are an error, not a silent default — see
[FLOW.md](FLOW.md) for what each one means.

## Responses

Exactly one response per request, tagged by `result`.

```jsonc
{"id":1,"result":"welcome","version":1,"verbs":["click", …],"session":"/path"}
{"id":2,"result":"done","outcome":"verified","points":[{…}],"elapsed_ms":840}
{"id":3,"result":"done","outcome":"failed","detail":"timed out after 10000ms …","elapsed_ms":10004}
{"id":4,"result":"located","moved":["submit"],"missing":[]}
{"id":5,"result":"closed"}
{"id":6,"result":"error","detail":"no selection labeled \"nope\" in this session — it has: …"}
```

`outcome` uses the run report's vocabulary: **`verified`** (an
observation step's condition held), **`executed`** (input was posted —
acting steps always report this, since a click cannot confirm its own
outcome), **`failed`** (it ran and did not work — `detail` says why),
**`refused`** (a guard declined before anything was attempted: the kill
switch, or a region that could not be confirmed).

**`done` vs `error` is the same line the exit codes draw between 1 and
2.** A step that ran and failed honestly is a `done` with
`outcome: "failed"`. A request that could not be understood or resolved —
bad JSON, unknown verb, unknown label — is an `error`, and nothing was
attempted.

`points` carries the coordinates actually acted on, after conversion,
with the monitor and scale they came from. Absent for steps that touch no
region.

## Scrolling until something appears

`scroll` is the one verb that is meaningfully better here than in a flow
file. Its `amount` counts wheel clicks and depends on the reader's OS
scroll-speed setting, so a fixed distance is never reliable — you want to
scroll *until* something is visible, and that needs a loop your language
already has:

```python
while ui.send(do="verify", target="footer")["outcome"] != "verified":
    ui.send(do="scroll", target="results", amount=3)
```

A scroll always answers `executed`, never `verified`: it changes its own
region on purpose, so that region cannot confirm it. Confirm with a
separate `verify` or `wait_for`, as above.

## The kill switch applies here too

Before every step — including observation-only ones — the cursor is read
and checked against every screen corner. A person who slams the mouse
into a corner stops the session, and the step comes back as
`{"result":"done","outcome":"refused"}` with a `detail` naming the kill
switch. Your client should surface that and stop: a human intervened,
and `refused` is never worth retrying.

## Relocation in a serve session

A flow file's targets are exactly what it will touch, so `run` re-locates
everything up front and refuses as a whole. A serve session is
open-ended, so two things differ:

- **The relocation pass happens once, lazily, before the first acting
  step** — not at startup. A bot that opens with `wait_for` is waiting
  for a UI that is not on screen yet; refusing to start would make that
  impossible.
- **A missing region only blocks the steps that name it.** A session may
  describe ten regions a given bot never visits.

Send `{"do":"relocate"}` whenever you know the UI has moved. It reports
`moved` and `missing` and never refuses — refusal belongs at the moment
something tries to act blind.

## A client, in full

Forty lines of Python, no package to install. The same shape works in
any language with a subprocess API.

```python
import json, subprocess

class PixelActions:
    def __init__(self, session, settings=None):
        self.proc = subprocess.Popen(
            ["pixelactions", "serve", "--session", session],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True,
        )
        self.id = 0
        hello = {"do": "hello", "version": 1}
        if settings:
            hello["settings"] = settings
        self.send(**hello)

    def send(self, **request):
        self.id += 1
        self.proc.stdin.write(json.dumps({"id": self.id, **request}) + "\n")
        self.proc.stdin.flush()
        reply = json.loads(self.proc.stdout.readline())
        if reply["result"] == "error":
            raise RuntimeError(reply["detail"])
        return reply

    def close(self):
        self.send(do="bye")
        self.proc.stdin.close()
        self.proc.wait()

# Your loop, in your language, with your data.
ui = PixelActions("~/captures/checkout", settings={"timeout_ms": 30000})
ui.send(do="wait_for", target="login")
for row in rows:
    ui.send(do="click", target="email")
    ui.send(do="type", text=row["email"])
    ui.send(do="key", chord="enter")
    if ui.send(do="verify", target="success")["outcome"] != "verified":
        failures.append(row)
ui.close()
```

Note what is *not* in that example: no sleeps. `wait_for` and `verify`
poll with real screen captures and return the instant the condition
holds, so waiting costs actual latency rather than a guessed worst case.

## Why persistent at all

Not speed — state. A `wait_for` that polls a hundred times is a hundred
screen captures; one process reuses the capture path instead of paying
setup each time. And the whole session shares a single relocation pass.

If you don't need a loop, don't reach for this. Chained argv does the
same work with nothing to keep in sync:

```bash
pixelactions run --session DIR click:email type:"a@b.com" key:enter verify:success --yes
```
