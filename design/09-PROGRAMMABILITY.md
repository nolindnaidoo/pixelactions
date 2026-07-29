<!-- Compiled 2026-07-29 from live web research; sources inline. Design
input, not shipped fact. Some MCP spec details are flagged by the author
as needing verification before implementation. -->

# Programmable engine design for `pixelactions` — research report

*Research July 2026. Measurements marked "measured here" were run on this machine. Claims from secondary sources are labeled.*

---

## 0. Recommendation first

**Build three things, in this order. Build nothing else.**

### Tier 1 — argv chaining + universal `--json` + exit codes (do this now)

Make one invocation able to perform *many* actions, and make every command speak JSON on stdout and a grep-style exit code.

```bash
pixelactions run --session <dir> \
  click:submit  wait:300  type:"hello"  key:cmd+s  assert:saved --timeout 5000
```

This is exactly what [`cliclick`](https://github.com/BlueM/cliclick) does (`cliclick c:123,456 w:500 t:hello` — "you pass an arbitrary number of commands as arguments") and what [`xdotool`](https://manpages.debian.org/testing/xdotool/xdotool.1.en.html) does with command chaining and its window stack ("xdotool supports running multiple commands on a single invocation… the result is saved to the window stack for future chained commands"). It collapses N spawns to 1 with **zero protocol**, zero daemon, zero new dependency.

Exit codes follow the [grep/diff convention](https://www.gnu.org/software/grep/manual/html_node/Exit-Status.html): `0` = did it / assertion held, `1` = assertion failed (**not an error**), `2` = tool error. `pixelcoords` already documents "exit codes are the API everywhere" — extend it.

### Tier 2 — one persistent stdio mode, NDJSON, `--protocol` (do this second)

```bash
pixelactions serve --session <dir>     # reads NDJSON requests on stdin, writes NDJSON on stdout
```

One JSON object per line in, one per line out, correlated by an `id`. This is the single highest-leverage thing you can build for "bots in any language," because it is:

- **The lowest-common-denominator IPC in every language.** Python `subprocess.Popen(stdin=PIPE, stdout=PIPE)`, Node `child_process.spawn`, Go `exec.Cmd`, Rust `std::process::Command`. No FFI, no wheels, no npm platform packages, no HTTP client, no async runtime *required* on the caller's side.
- **What every serious "engine + any language" system converged on.** LSP is [JSON-RPC over stdio](https://microsoft.github.io/language-server-protocol/overviews/lsp/overview/) ("a language server runs as a separate process and development tools communicate with the server using the language protocol over JSON-RPC"). Playwright's Python/Java/.NET bindings [spawn a Node driver and speak JSON-RPC over pipes](https://github.com/microsoft/playwright-python/blob/main/playwright/_impl/_transport.py). esbuild's JS API spawns the Go binary and talks a [length-prefixed binary protocol over stdin/stdout](https://github.com/evanw/esbuild/blob/main/lib/shared/stdio_protocol.ts) — "the JavaScript API communicates with the Go child process over stdin/stdout using this protocol." Git added [`filter.<driver>.process`](https://git-scm.com/docs/long-running-process-protocol) — "all communication is in pkt-line format over standard input and standard output" — for exactly this reason. MCP's stdio transport is [newline-delimited JSON-RPC](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/stdio).
- **Free MCP compatibility later.** MCP stdio *is* NDJSON-framed JSON-RPC. If your line protocol is JSON-RPC-shaped, an MCP server is a thin adapter, not a rewrite.

Rules to copy verbatim from the MCP stdio binding, which are load-bearing and cheap:
- Messages MUST NOT contain embedded newlines; stdout carries **only** protocol messages; **stderr is for logs** and the client must not treat stderr output as an error.
- Closing stdin is the graceful shutdown signal.
- Version-negotiate on the first exchange, like [git's handshake](https://git-scm.com/docs/long-running-process-protocol) (welcome message + version list + capability subset). This is ~30 lines and buys you the ability to change the protocol later without breaking every bot.

### Tier 3 — a `SKILL.md` / agent doc + a `--json` schema doc (do this third, it's cheap)

Not an MCP server. See §5.

### Do NOT build

| Don't build | Why |
|---|---|
| **A local HTTP/TCP server or Unix socket daemon** | It's a privilege-laundering machine and a support burden. See §1.4. |
| **An embedded scripting language (Lua/Rhai/JS)** | You may end up owning the interpreter. Grafana forked Goja into [Sobek](https://github.com/grafana/k6/issues/3773) because upstream had one time-constrained maintainer and Goja was k6's cornerstone plus a dependency of hundreds of extensions. That's a solo maintainer's nightmare. |
| **PyO3/napi native modules** | See §4. The whole point of the stdio protocol is that you never have to. |
| **A tool-per-CLI-subcommand MCP server** | See §5. |
| **A WebSocket/gRPC/protobuf layer** | Local, single-client, low-throughput. NDJSON over a pipe moves [3.7 GiB/s](https://mazzo.li/posts/fast-pipes.html). |
| **A CDKTF-style polyglot SDK suite** | HashiCorp, with vastly more resources, [deprecated CDKTF on 2025-12-10](https://developer.hashicorp.com/terraform/cdktf). |

---

## 1. One-shot CLI vs persistent stdio vs local socket

### 1.1 Spawn cost — measured, and smaller than the folklore

Measured on this machine (macOS, `python3` driving `subprocess.run`, 60 iterations, release binary):

| Command | mean | median | min | max |
|---|---:|---:|---:|---:|
| `/usr/bin/true` | 1.36 ms | 1.30 ms | 1.13 | 3.06 |
| `pixelcoords --version` | 9.72 ms | **3.42 ms** | 3.04 | **351.57** |
| `pixelcoords --help` | 3.36 ms | 3.25 ms | 3.01 | 4.35 |

The 351 ms outlier was the **first** run — macOS Gatekeeper/AMFI/XProtect assessment. Howard Oakley [instrumented that tax](https://eclecticlight.co/2025/05/04/last-week-on-my-mac-checking-code-can-take-longer-now/): **195 ms (Ventura/M1 Max) → 303 ms (Sequoia/M4 Pro)** for a CLI tool the system hasn't seen recently, XProtect alone going 0.030 s → 0.126 s as its Yara ruleset grew 218→381 rules. **Per binary-version, not per spawn**, but it lands on the first invocation after every install and update.

Cross-platform ([bdrung/startup-time](https://github.com/bdrung/startup-time), 1000× per program via a C launcher, Intel i5-2400S / Ubuntu 17.10):

| Language | Linux | RPi3 |
|---|---:|---:|
| C | 0.26 ms | 2.19 ms |
| **Rust** | **0.51 ms** | 4.42 ms |
| bash | 0.71 ms | 7.31 ms |
| Python 3 | **25.84 ms** | 197.79 ms |
| Ruby | 32.43 ms | 421.53 ms |

**The child process is cheaper than the interpreter calling it.** Spawning your Rust binary costs ~0.5 ms on Linux and ~3 ms on macOS; starting CPython costs ~26 ms.

Platform gotchas that *do* bite:
- **Linux primitive choice is a 10× swing** — [famzah's benchmark](https://blog.famzah.net/2018/12/19/posix_spawn-performance-benchmarks-and-usage-examples/): `vfork`+exec 9.0 relative CPU, `posix_spawn` 11.5, `fork`+exec 84.3, `popen()` 117.5. `fork` cost scales with the *parent's* memory, not the child's — [measured](https://movq.de/blog/postings/2023-11-26/0/POSTING-en.html) at 32,768 spawns: 1 thread 1.6 s, 512 threads **58.2 s**; `vfork` flat at 0.6 s.
- **macOS fork from a JIT parent is 100× worse** — [libuv#3050](https://github.com/libuv/libuv/issues/3050) measured 558 µs normal vs **45,600 µs** from a parent holding a `MAP_JIT` mapping (i.e. every Electron/Node/Chrome process). Fixed by [switching Darwin to `posix_spawn`](https://github.com/libuv/libuv/pull/3064).
- **Windows ~5 ms, degrading** — Bruce Dawson [measured](https://randomascii.wordpress.com/2018/10/15/making-windows-slower-part-2-process-creation/) 200 processes/second (5 ms each) normally, 13 ms with Application Verifier, 25 ms and unboundedly worse with its logging on.
- **Python's fast path is fragile** — [docs](https://docs.python.org/3/library/subprocess.html) say subprocess uses `vfork()` "when it is safe to do so," but the `posix_spawn` path requires `close_fds` false (it **defaults to True**), no `preexec_fn`/`pass_fds`/`cwd`, and an absolute path. The [gh-112334 regression](https://github.com/python/cpython/issues/112334) quantified it at 10,000 spawns: vfork 2.419 s vs fork 11.772 s — **242 µs vs 1.18 ms**, caused by a one-character condition error.
- **Node spawns badly** — [Val Town](https://blog.val.town/blog/node-spawn-performance): one process per HTTP request gives Node 651 req/s vs Deno 2,290, Bun 2,208, Rust 5,466. In production, "a single Val Town Node server cannot exceed 40 spawns/s. It spends 30% of its time with the main thread blocked on calls to spawn." Their fix was a **child-process pool**, not a native module — 3.4× improvement.

### 1.2 The historical evidence that persistent modes exist for a reason

| System | Why it added a persistent mode |
|---|---|
| **Git long-running filters** | The original clean/smudge interface "ran a command once per file." Git-LFS: "10k files meant 10k process executions which is horribly slow on Windows." Fix: [`filter.<driver>.process`](https://git-scm.com/docs/long-running-process-protocol) — "Git can process all blobs with a single filter command invocation for the entire life of a single Git command." |
| **`git cat-file --batch`** | "Spawning a new cat-file process each time is not ideal from a performance perspective." ([git docs](https://git-scm.com/docs/git-cat-file), [Gitea #6649](https://github.com/go-gitea/gitea/issues/6649)) |
| **tmux control mode** | ["Control mode is a special mode that allows a tmux client to be used to talk to tmux using a simple text-only protocol."](https://github.com/tmux/tmux/wiki/Control-Mode) `%begin`/`%end`/`%error` blocks carry (epoch seconds, command number, flags); `%`-prefixed async notifications carry state changes. **Design principle worth stealing verbatim: "The idea is that users of control mode use tmux commands… to control tmux rather than duplicating a separate command set."** Your protocol verbs should *be* your CLI verbs. |
| **Bazel persistent workers** | [Java builds "2–4 times faster," Bazel self-compile "about 2.5 times as fast," incremental edge case "a factor of 6."](https://bazel.build/remote/persistent) |
| **Playwright driver** | Non-JS bindings download a per-platform driver and speak JSON-RPC over pipes so users don't install Node. Language packages must version-match the driver. |
| **ydotool** | The [daemon exists](https://github.com/ReimuNotMoe/ydotool) because "when ydotool creates a virtual input device, it will take some time for your graphical environment to recognize and enable [it]… To solve this problem, a persistent background service, ydotoold, is made to hold a persistent virtual device." **This is the one automation-specific state argument, and it's a Linux/uinput problem you may not have.** |
| **`dotool`** | ["dotool reads actions from stdin"](https://sr.ht/~geb/dotool/), with `dotoold`/`dotoolc` as the daemon variant — "there is an initial delay registering the virtual devices, but you can keep writing commands to the same instance." |
| **adb** | The canonical complaint: `adb shell input tap 100 500` taking [~1 s, sometimes 3–5 s per invocation](https://github.com/Genymobile/scrcpy/issues/231) makes loops unusable. |

### 1.3 But be blunt: for *your* tool, spawn cost is not the argument

At ~3 ms/spawn on macOS, a 100-action flow pays 0.3 s of process overhead — against UI settle times of 50–500 ms per action. **Spawn cost is noise.** The crossover table:

| N calls | Linux 0.5 ms | Windows 5 ms | Win + AV 25 ms | macOS JIT parent 42 ms |
|---|---:|---:|---:|---:|
| 100 | 0.05 s | 0.5 s | 2.5 s | 4.2 s |
| 1,000 | 0.5 s | 5 s | 25 s | 42 s |
| 10,000 | 5 s | 50 s | 250 s | 420 s |

And the serialization tax reframes it further. One 1 MB round trip on Linux, warm ([serde json-benchmark](https://github.com/serde-rs/json-benchmark), [mazzo.li on pipes](https://mazzo.li/posts/fast-pipes.html)): spawn ~0.24–0.5 ms, serialize ~3.2 ms, pipe ~0.26 ms, parse ~1.8 ms — **serialization costs ~5× the spawn.** Your payloads are coordinates, and crops go to disk as files, so you sit in the small-payload regime where spawn dominates and spawn is cheap.

**The real arguments for a persistent mode are state and semantics, not milliseconds:**

1. **Screen-capture warm-up.** Any `assert`/`wait_for` needs a frame. `SCScreenshotManager` is async and stream setup has real cost; one process capturing repeatedly beats N processes each doing SCK setup. This dominates spawn by an order of magnitude.
2. **Retry loops.** `wait_for(condition, timeout=5s, poll=50ms)` is 100 captures. As one-shot CLI calls that's 100 processes *and* 100 capture warm-ups. As a protocol call it's one message.
3. **macOS TCC identity.** Accessibility/Screen Recording grants attach to the *requesting binary*. A wrapper in the middle breaks it — see the [Claude Code TCC issue](https://github.com/anthropics/claude-code/issues/36832): "macOS TCC ties the permission grant to the requesting process. Because `disclaimer` is the intermediary, granting permission to `node` doesn't satisfy future requests." **Corollary: a PyO3 extension makes `python3` the grantee, which is worse than a helper binary.**
4. **Drag/modifier state across steps** (OS-global, so one-shot mostly works — but a protocol makes it explicit and cancellable).
5. **Cancellation and progress**, which argv has no way to express.

### 1.4 Local socket / HTTP — the security argument against

For a tool that **synthesizes clicks and keystrokes**, a local listener is a privilege-laundering device. You hold the Accessibility/Screen-Recording grant; anything that can talk to your socket inherits it. Nailgun states the failure mode plainly: ["Nailgun is not secure. Although there are means to ensure that the client is connected from the local machine, there is not yet any concept of a 'user'. Any programs that run in Nailgun are run with the same permissions as the server itself."](https://github.com/facebookarchive/nailgun) That is a textbook [confused-deputy / permission re-delegation](https://hacktricks.wiki/en/macos-hardening/macos-security-and-privilege-escalation/macos-proces-abuse/macos-ipc-inter-process-communication/macos-xpc/macos-xpc-authorization.html) — and macOS XPC services get it wrong routinely (e.g. CVE-2025-25251, a helper that "accepted crafted XPC messages… lacking authorization gates").

If it's **TCP on localhost**, it's worse: any web page can reach it via [DNS rebinding](https://github.blog/security/application-security/localhost-dangers-cors-and-dns-rebinding/). This is not theoretical — the MCP TypeScript SDK shipped [GHSA-w48q-cv73-mx4w / CVE-2025-66414 (CVSS 7.6)](https://www.straiker.ai/blog/agentic-danger-dns-rebinding-exposing-your-internal-mcp-servers), letting a malicious website drive localhost MCP servers.

Unix sockets are better but not free: they get filesystem DAC/MAC ([LWN](https://lwn.net/Articles/984841/), [Matt Oswalt](https://oswalt.dev/2025/08/unix-domain-sockets/)), and you can check peer credentials — but `SO_PEERCRED` is Linux-only; macOS/BSD use `LOCAL_PEERCRED`/`LOCAL_PEERPID` at `SOL_LOCAL`, and Windows has no socket at all (named pipes, different namespace — [`interprocess` crate](https://docs.rs/interprocess) papers over it). That's three platform paths, a permissions story, and an auth story you'd have to own forever. Docker is the cautionary tale: [`/var/run/docker.sock` access "is equivalent to giving them root access to the host OS"](https://blog.quarkslab.com/why-is-exposing-the-docker-socket-a-really-bad-idea.html).

**Stdio sidesteps all of it.** The client *launched* you; there is no listener, no port, no origin check, no auth handshake, and the OS reclaims the process when the parent exits. The MCP spec makes the same call: Streamable HTTP servers MUST validate `Origin` and SHOULD bind only to 127.0.0.1; stdio has neither problem because there's no socket.

The counterweight to persistence generally: **daemons rot.** Gradle daemons are the canonical complaint — [stale processes retaining in-memory state, memory exhaustion, "1 busy & 6 stopped daemons could not be reused"](https://github.com/gradle/gradle/issues/14741). Keep your persistent mode **child-of-caller only** (no detached daemon, no PID file, no auto-start), and this whole class of bug never exists.

---

## 2. Pattern-by-pattern comparison

| Tool | Transport | Who owns the loop | What users complain about |
|---|---|---|---|
| **ffmpeg** | argv only | Caller | Docs. [`ffmpeg-all.html` is 2.6 MB / 68,367 lines](https://ffmpeg.org/ffmpeg-all.html); option meaning depends on position relative to `-i`; no task-first layer. libav* API so hard that most bindings just shell out to the binary. |
| **ImageMagick** | argv + MagickWand C API + ~25 language bindings | Caller | The [`/develop/` page](https://imagemagick.org/develop/) is an unranked prose list — Ruby has 4 competing bindings, Python 3. "You can use it from 25 languages" reads as "you're on your own." |
| **jq** | stdin/stdout filter | Caller | Syntax learning curve; libjq embedding is painful enough that people write [full reimplementations (gojq, jaq)](https://github.com/itchyny/gojq) rather than link it — "jq depends on the C standard library and has complex build scripts." |
| **ripgrep / fd** | argv + `--json` NDJSON | Caller | [Format is `{"type": "...", "data": {...}}` per line](https://docs.rs/grep-printer/latest/grep_printer/struct.JSON.html), types "may expand over time," **no version field** — a footgun. BurntSushi on library use: ["If you cannot create a child process, then your only option is to port ripgrep to Javascript or to devise a C API"](https://github.com/BurntSushi/ripgrep/discussions/2067); the `grep` crate README says ["This crate isn't ready for wide use yet."](https://raw.githubusercontent.com/BurntSushi/ripgrep/master/crates/grep/README.md) |
| **tmux** | Server + control mode (`-CC`), `%begin`/`%end` blocks + `%` notifications over a text stream | Client drives, server pushes async events | Text protocol is easy to parse but has no schema; iTerm2 is effectively the only full consumer. |
| **LSP / DAP** | JSON-RPC over stdio (Content-Length framing), sockets/pipes optional | Bidirectional; both sides can initiate | Framing + async request IDs are real work; but chosen because [JSON-RPC "is simple and libraries for it exist in basically every language"](https://microsoft.github.io/language-server-protocol/overviews/lsp/overview/). |
| **Playwright** | Per-language binding → JSON-RPC over pipes → Node driver | Caller (sync or async facade) | Bundle size ([~118 MB driver/node in Docker](https://github.com/microsoft/playwright-python/issues/2688)); [driver Node version lags](https://github.com/microsoft/playwright/issues/26753); client/server version must match exactly; Windows needs `ProactorEventLoop`. |
| **Puppeteer / CDP** | WebSocket, JSON-RPC-ish `{id, method, params}` | Caller | Raw CDP is low-level and version-churny; ["for almost every project, use a library like Playwright or Puppeteer"](https://github.com/aslushnikov/getting-started-with-cdp). |
| **WebDriver / Appium** | HTTP + JSON, session-oriented (`POST /session`) | Caller, strict request/response | [Unidirectional: "only allows for communication to happen in one direction at any time"](https://github.com/w3c/webdriver-bidi/blob/main/explainer.md) — no events, no streaming. Fixed by WebDriver BiDi (WebSocket). Also session-management overhead and spec-conformance drift ([Appium #11300](https://github.com/appium/appium/issues/11300)). |
| **Docker** | CLI → HTTP over Unix socket | Caller | ["The docker CLI is just a client that sends API requests to that socket"](https://docs.docker.com/reference/api/engine/) — but socket access ≡ root, and the [CLI reference never links to the API reference](https://docs.docker.com/reference/cli/docker/). |
| **Terraform providers** | Launch plugin, read handshake from **stdout**, connect gRPC | Core owns the loop | [Handshake-on-stdout means the plugin must block other stdout output](https://github.com/hashicorp/go-plugin/issues/152). Heavy for a small tool. |
| **AutoHotkey v2** | In-process script; `DllCall`/COM out; `AutoHotkey.dll` for embedding | Script owns the loop | Windows-only; embedding via `LoadLibrary`+`ahkdll` is niche. |
| **SikuliX** | Java API + Jython script runner | Script | JVM+Jython tax (~5 s init, classpath/jar wrangling); OCR quality is the top complaint. |
| **Robot Framework** | Keyword libraries in-process, **or Remote Library over XML-RPC** | Framework owns the loop | The remote interface is the "any language" escape hatch — but [only standard XML-RPC, no extensions](https://github.com/robotframework/robotframework/issues/1489). Right idea, dated wire format. |
| **Maestro** | YAML flows + GraalJS `runScript` | Maestro owns the loop | ["Maestro is not a JavaScript testing framework, but the team recognizes that not everything can (or should) be written in YAML."](https://maestro.dev/blog/maestro-announcing-javascript-http-request-support) JS is sandboxed, no filesystem; results return only via a mutable `output` object. |
| **esbuild** | JS/Go API → **length-prefixed binary protocol over stdin/stdout** → single long-lived Go child | Caller | Closest architectural analogue to what you want. Protocol note: ["You must send a response after receiving a request because the other end is blocking on the response coming back."](https://github.com/evanw/esbuild/blob/main/lib/shared/stdio_protocol.ts) |
| **terminator** (mediar-ai) | Rust core + napi TS + PyO3 Python + CLI + **MCP with 35+ tools** | Caller | The maximalist version of your question, [Windows-only](https://github.com/mediar-ai/terminator). 35+ MCP tools is well past the 30–50 degradation threshold (§5). |
| **cua** (trycua) | FastAPI computer-server: HTTP + WebSocket + MCP-over-HTTP | Caller | The [socket-server approach](https://cua.ai/docs/libraries/computer-server/WebSocket-API), and it needs sandboxes/VMs to be safe. |

---

## 3. Essential primitives

### The ~14 verbs that carry it

Evidence base: GitHub code search counts for `pyautogui.*` (per-file, includes forks — directional), AutoHotkey's load-bearing set, and the vendor computer-use action enums.

| # | Verb | Signature sketch | Evidence |
|---|---|---|---|
| 1 | `screenshot` | `(region?) -> Frame` | 10,480 files; substrate for every check |
| 2 | `click` | `(x, y, button, count, modifiers)` — **one function** | 14,904; `count` folds double/triple, `button` folds right/middle |
| 3 | `key` | `(chord: "cmd+shift+s")` | `press` 13,360 + `hotkey` 9,296 — one primitive |
| 4 | `move_to` | `(x, y, duration)` | 11,952 |
| 5 | `type_text` | `(s, interval)` — **must be Unicode-capable** | `write`+`typewrite` 10,840; [27.6k-view SO question on unicode](https://stackoverflow.com/questions/33151865/input-unicode-string-with-pyautogui) |
| 6 | `scroll` | `(dx, dy, x?, y?)` — one fn, not three | 4,624 |
| 7 | `cursor_position` | `() -> Point` | 7,880 |
| 8 | `key_down` / `key_up` | `(key)` | 3,904 — needed for holds |
| 9 | `mouse_down` / `mouse_up` | `(button, x?, y?)` | press-and-hold, drag composition |
| 10 | `drag_to` | `(x, y, button, duration)` | 2,592 — a real primitive; naive down/move/up drops intermediate motion events |
| 11 | `screen_info` | displays, bounds, **scale factor, coordinate space** | the #1 SO question in the tag (107k views, ["window handle"](https://stackoverflow.com/questions/43785927/python-pyautogui-window-handle)) + [Retina bugs #281](https://github.com/asweigart/pyautogui/issues/281)/[#33](https://github.com/asweigart/pyautogui/issues/33) |
| 12 | `pixel` | `(x, y) -> Rgb` | cheapest possible observation |
| 13 | `find_image` | `(needle, region, confidence) -> Option<Rect>` | 3,272 + 2,100 |
| 14 | **`wait_for`** | `(Condition, timeout, poll) -> Result<Match>` | **the missing primitive in every coordinate tool surveyed** |

Plus the non-verb that matters most: **an explicit coordinate space on every coordinate.** AutoHotkey's [`CoordMode`](https://www.autohotkey.com/docs/v2/lib/CoordMode.htm) keys the space *per target type* — `Pixel` affects `PixelGetColor`/`PixelSearch`/`ImageSearch`, `Mouse` affects `Click`/`MouseMove` — with `RelativeTo` ∈ `Screen`/`Window`/`Client`. AHK learned that pixel-space and pointer-space are not the same space. PyAutoGUI has one implicit global space and pays for it in the Retina bug and the #1 SO question. You already have `px` / `global_px` / `window_px` / per-monitor `scale` — keep that, and make it an explicit enum parameter, not an inference.

### Bloat — looks necessary, isn't

| Cut | Why |
|---|---|
| Dialog boxes (`alert`/`confirm`/`prompt`) | 1,472 / 774 files vs `click`'s 14,904. PyAutoGUI itself outsources them to `pymsgbox`. A GUI toolkit inside an input library. |
| Easing/tween curves | `easeInQuad`, `easeOutElastic` via `pytweening`. Humanization theater. `duration` + linear is the 99% case. |
| Per-button/per-count click functions | `leftClick`/`rightClick`/`middleClick`/`doubleClick`/`tripleClick` = 5 functions for `click(button, count)`. |
| `hscroll`/`vscroll` | `scroll(dx, dy)`. |
| Separate relative variants | `moveRel`/`dragRel`, aliased *again* to `move`/`drag` — pure API churn debt. |
| Re-screenshotting per locate | pyscreeze takes a fresh full-screen capture inside every `locateOnScreen` → 1–2 s each on 1920×1080 (its own docs) and a [memory leak under polling (#806)](https://github.com/asweigart/pyautogui/issues/806). **Capture one frame, run N queries against it.** |
| A string mini-DSL | PyAutoGUI's `run("ccc")`. Essentially unused. |
| Background observer machinery | SikuliX `onAppear`/`onVanish`/`onChange`/`observe`/`observeInBackground` — a thread, a handler registry, and an event type to express what `wait_for` expresses synchronously. |
| Global exception-mode switches | SikuliX `setThrowException` / `setFindFailedResponse(SKIP\|RETRY\|PROMPT\|ABORT)` / `setFindFailedHandler`. Two well-named functions replace all four. |
| **A hidden global sleep on every call** | `pyautogui.PAUSE = 0.1` injects 100 ms into *every* call. This is exactly the fixed sleep auto-waiting exists to delete, and it silently caps your throughput. |
| OCR as a core primitive | SikuliX's single biggest quality complaint. Feature-gate it; never the default matcher. |
| Input listening / hotkey registration | 55k-view SO question wants it, but it's a different tool (event source, not action sink) with a different threading model. |

### The `wait_for` / `assert` primitive — the design that matters

**Shape: one engine, two return types.** SikuliX's best idea, hardened:

```rust
wait_for(cond, timeout, poll) -> Result<Match, WaitTimeout>   // errors on timeout
exists  (cond, timeout)       -> Option<Match>                // never errors
```

Never a global "should this throw" switch. And per Playwright's own [best-practices warning](https://playwright.dev/docs/best-practices) — use `await expect(locator).toBeVisible()`, **not** `expect(await locator.isVisible()).toBe(true)` because the latter "won't wait a single second" — **make the retrying form and the instant-boolean form look different**, or users will accidentally write the flaky one.

**Conditions: a small closed enum** (no DOM, so no open extension story):

| Condition | Params | Precedent |
|---|---|---|
| `PixelEquals { x, y, rgb, tolerance }` | tolerance | AHK `PixelSearch` Variation 0–255 (default 0); pyscreeze `tolerance=0` |
| `ImageAppears { needle, region, confidence }` | confidence ~0.7–0.9 | SikuliX `MinSimilarity = 0.7` |
| `ImageVanishes { … }` | same | SikuliX `waitVanish` → `True/False`, never raises |
| **`RegionStable { region, samples: 2 }`** | N identical consecutive hashes | **Direct port of Playwright's "same bounding box for at least two consecutive animation frames."** This is the sleep-killer for animations, and nobody in the coordinate-tool space implements it |
| `RegionChanged { region, baseline, min_changed_px }` | default ~50 px | SikuliX `ObserveMinChangedPixels = 50` ("about 7×7 pixels") |
| `TextPresent { region, pattern }` | OCR — **feature-gated** | SikuliX OCR complaints |

**Semantics to copy exactly:**

1. **Always evaluate at least once, even at `timeout = 0`.** SikuliX guarantees this, which makes `exists(cond, 0)` a clean non-blocking probe.
2. **Return the moment the condition holds.** AHK [`WinWait`](https://www.autohotkey.com/docs/v2/lib/WinWait.htm): "If a matching window comes into existence, the function will not wait for Timeout to expire." You pay actual latency, not worst-case.
3. **Poll rates graded by cost.** SikuliX runs ~3 scans/sec; Selenium polls at 500 ms. Pixel probe → 20–50 ms. Full-screen template match is 1–2 s on 1080p → **require or strongly default a `region`**, and never poll faster than the check takes.
4. **One frame per poll iteration; evaluate all conditions against it.**
5. **Graded default timeouts.** Playwright: assertions **5 s**, actions **no default**. Maestro: `assertVisible` auto-retries **up to 7 s**, `scrollUntilVisible` 20 s, `extendedWaitUntil` explicit. Put the default on the *assertion*; make the long wait explicit and named.
6. **The timeout error must carry evidence** — elapsed, poll count, region searched, **best score achieved**, last frame. pyscreeze already does this internally (`'Could not locate the image (highest confidence = %.3f)'`); PyAutoGUI's ["locateOnScreen returning none is a valid state (need help handling)"](https://github.com/asweigart/pyautogui/issues/303) exists because the surface doesn't.
7. **Re-evaluate the description, never a cached handle.** Playwright calls locators "the central piece of auto-waiting and retry-ability" precisely because they re-resolve each attempt.
8. **Compose actions on top.** `click_when(cond, timeout)` = `wait_for` → `click(match.center())`. That's `scrollUntilVisible` and `tapOn` in one line, and it makes the auto-waiting path the *shortest* path.
9. **Be strict about ambiguity.** Playwright throws when a locator matches multiple elements rather than guessing. `find_image` with 4 candidates above threshold should error or require `nth`, not silently take the top-left scan hit.

### Why users love auto-waiting — the evidence

The single strongest datum: **28,400 Python files import PyAutoGUI *and* call `time.sleep`; only ~272 use `minSearchTime`** — the built-in retry-until-found that ships *inside* `locateOnScreen`. It's undocumented in the PyAutoGUI docs, so users hand-roll `while not found: sleep(1)`. [Issue #625](https://github.com/asweigart/pyautogui/issues/625) is a user requesting a feature that already existed. **An auto-wait that isn't the default path does not exist.**

Supporting:
- Vendors lead with it. Playwright: ["Playwright waits for elements to be actionable… Assertions automatically retry until conditions are met. No artificial timeouts, no flaky tests."](https://playwright.dev/) Maestro: ["No more manual `sleep()` calls."](https://docs.maestro.dev/get-started/what-is-maestro)
- The framework that doesn't admits the cost. [Selenium's own docs](https://www.selenium.dev/documentation/webdriver/waits/): "the most common challenge for browser automation is ensuring that the web application is in a state to execute a particular Selenium command"; "**Do not mix implicit and explicit waits. Doing so can cause unpredictable wait times**"; `Thread.sleep` "can fail when insufficient or prohibitively extend session duration."
- **A sleep is neither a floor nor a ceiling.** AHK's [`Sleep`](https://www.autohotkey.com/docs/v2/lib/Sleep.htm) docs: "typically rounded up to the nearest multiple of 10 or 15.6 milliseconds" and "the actual delay time might wind up being longer than what was requested if the CPU is under load."
- It's a **speed** argument too — Maestro's "but never longer than necessary." A 100-step flow with 500 ms defensive sleeps burns 50 s that poll-until-true reclaims.
- The structural difference isn't the algorithm — both poll. It's that **Selenium's waiting is opt-in and lives in user code; Playwright's is opt-out and lives in the library.**

For reference, Playwright's [actionability checks](https://playwright.dev/docs/actionability) are per-action, not blanket: click needs Visible+Stable+ReceivesEvents+Enabled; `screenshot` needs Visible+Stable only; `press`/`focus` need nothing. Don't apply one precondition to everything.

---

## 4. Language bindings — subprocess wins, and it isn't close

### The verdict for a solo maintainer

**Ship one binary. Ship a documented stdio protocol. Write thin, pure-source wrapper packages (a few hundred lines each) that spawn it.** Do not ship PyO3 wheels or napi-rs modules.

### Why

**The distribution matrix is the tax, and the packaging community says so directly.** [pypackaging-native](https://pypackaging-native.github.io/meta-topics/user_expectations_wheels/), verbatim: *"The `cibuildwheel` matrix adds up to **70 combinations**"*; *"Most projects use **3-4 different CI systems**"*; *"That maintenance cost can be shared much more easily for packaging systems with centralized builds; **for building wheels they have to be paid by every project**"*; *"There are usually one a couple of maintainers (**perhaps even a single person**) who are responsible for or have expertise in wheel building."* Wheels per release in the wild: Kivy 20, NumPy 27, asyncpg 35, PyGame 57. Ruff — a binary-only tool with no Python API — still ships 17.

A binary-shipping wrapper doesn't escape the OS/arch axis, but it **escapes the Python-version axis and the interpreter-ABI coupling entirely**, which is where most of the 70 comes from.

**The performance argument for native modules is weaker than advertised.** At 1 MB payloads on Linux, serialization costs ~5× the spawn (§1.3); eliminating the spawn buys ~10% and you still pay the JSON tax. The honest case for a native module was never "avoid process startup" — it's zero-copy structured handoff and long-lived in-process state, and **a persistent stdio child gives you the long-lived state without the ABI**. Val Town's fix for Node's spawn ceiling was a [child-process pool, not a native module](https://blog.val.town/blog/node-spawn-performance) (3.4×). Bazel's answer was [persistent workers](https://bazel.build/remote/persistent) (2–6×).

**The macOS TCC argument is decisive for *this* tool.** A PyO3 extension makes `python3` the entity holding the Accessibility/Screen-Recording grant — which means the grant follows whichever Python the user happens to run, breaks on Homebrew upgrades, and is exactly the wrapper-binary failure documented in [claude-code#36832](https://github.com/anthropics/claude-code/issues/36832). A single signed helper binary is a *stable TCC identity*. That's a correctness argument, not a convenience one.

**Precedent runs your way.**
- **BurntSushi on ripgrep**: [shell out, like VS Code does](https://github.com/BurntSushi/ripgrep/discussions/2067); the crate is [explicitly "not ready for wide use."](https://raw.githubusercontent.com/BurntSushi/ripgrep/master/crates/grep/README.md) The [`libripgrep` issue](https://github.com/BurntSushi/ripgrep/issues/162) closes with "This is predominantly a design task… I don't really have the bandwidth to do the design work required." That's a full-time-equivalent maintainer declining the work you'd be signing up for.
- **esbuild** ships a Go binary and talks to it over [stdin/stdout from JS](https://github.com/evanw/esbuild/blob/main/lib/shared/stdio_protocol.ts) — not a native module.
- **Playwright** ships a driver subprocess for every non-JS language rather than N native bindings.
- **ffmpeg**: the libav* API is hard enough that most language "bindings" [just invoke the binary as a subprocess](https://cloudinary.com/guides/front-end-development/ffmpeg-python).
- **jq**: people write [whole reimplementations](https://github.com/itchyny/gojq) rather than link libjq, citing portability and build complexity.
- **terminator** (mediar-ai) is the counterexample — [Rust core + napi TS + PyO3 Python + CLI + 35-tool MCP](https://github.com/mediar-ai/terminator) — and it's Windows-only, VC-backed, and not solo-maintained. That's the resource level that surface requires.

**Caveats I'd hold you to.** The "robotjs died because native rebuilds" and "GitHub Desktop ripped out nodegit for dugite to escape native modules" narratives are widely believed and **underdocumented** — [dugite](https://github.com/desktop/dugite)'s README states no rationale. I'd stop repeating them as established fact. Likewise the nut.js commercial-licensing change: I could not verify it from a primary source in this pass, so don't cite it. The *structural* argument (wheel matrix, ABI coupling, TCC identity) stands on its own without them.

### If you publish wrapper packages

Use the **platform-package + optionalDependencies** pattern for npm (esbuild's model: `@scope/tool-darwin-arm64` etc., with the main package resolving at runtime) and a **binary-in-wheel with a `console_scripts` entry point** for PyPI. Both are pure packaging — no compiler on the user's machine, no `node-gyp`, no `manylinux` toolchain. Document the [macOS first-run Gatekeeper tax (~200–300 ms)](https://eclecticlight.co/2025/05/04/last-week-on-my-mac-checking-code-can-take-longer-now/) so nobody files a "your library is slow to start" issue.

But honestly: **for v1, publish nothing.** A 40-line `pixelactions.py` in your README that spawns the binary and speaks NDJSON is a better first deliverable than a package you have to keep releasing in lockstep with the binary. Playwright's version-lockstep pain ([client/server version must match exactly](https://github.com/microsoft/playwright-python/issues/2696)) is the thing you're avoiding.

---

## 5. MCP and agent consumption

*Confidence note: the MCP `2026-07-28` specifics below were fetched from the spec site during this research. The revision reportedly made large breaking changes (removal of `initialize`, removal of protocol sessions, mandatory `server/discover`). Verify against the spec before implementing.*

### Transport: stdio, unambiguously — and it's the same framing you're already building

The [stdio binding](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/stdio) is newline-delimited JSON-RPC with these MUSTs: no embedded newlines; stdout carries only protocol messages; **stderr is for logging and the client MUST NOT assume stderr output indicates errors**; closing stdin is the graceful shutdown signal. Notably the spec says custom transports over any reliable bidirectional byte stream "SHOULD reuse the stdio framing."

HTTP+SSE is dead: deprecated since `2025-03-26` and formally reclassified as Deprecated (eligible for removal) in `2026-07-28`. Streamable HTTP is the remote option, and it requires `Origin` validation against DNS rebinding plus 127.0.0.1 binding — all the complexity §1.4 says to avoid.

### What makes a tool good vs frustrating for an LLM

[Anthropic's "Writing effective tools for agents"](https://www.anthropic.com/engineering/writing-tools-for-agents):
- **Consolidate.** One `schedule_event` beats `list_users` + `list_events` + `create_event`.
- **Namespace** by service and resource; A/B prefix vs suffix — it produces measurable eval differences.
- **Token budget.** Claude Code caps tool responses at **25,000 tokens** (warns at 10,000). Paginate, filter, truncate with sensible defaults.
- **`response_format` enum** (`concise`/`detailed`) — their worked example is **72 vs 206 tokens**.
- **High-signal returns:** semantic names, not UUIDs.
- **Errors that teach** — "specific and actionable improvements," not tracebacks.
- **Descriptions as onboarding** — write for "a new hire on your team."

The bloat numbers ([tool search docs](https://platform.claude.com/docs/en/agents-and-tools/tool-use/tool-search-tool), [Advanced tool use](https://www.anthropic.com/engineering/advanced-tool-use)): a five-server setup burns **~55K tokens** of definitions before any work; worst observed **134K**. **Selection accuracy degrades once you exceed 30–50 available tools.** Tool search cuts definitions >85% and lifted an MCP eval from 79.5% → 88.1% on Opus 4.5. Anthropic's thresholds: standard calling below **10 tools / 100 tokens**; tool search at ≥10 tools or >10k tokens.

Error-handling design is the one thing to get right: **tool execution failures go in the result with `isError: true`, not as JSON-RPC errors** — the schema's rationale is "otherwise, the LLM would not be able to see that an error occurred and self-correct."

Annotation defaults surprise people: **`destructiveHint` and `openWorldHint` default to `true`.** Omit annotations and every tool reads as destructive and open-world, and hosts gate it.

### The "code execution / tools-as-code" shift

[Code execution with MCP](https://www.anthropic.com/engineering/code-execution-with-mcp) presents servers as a filesystem of importable modules rather than tool schemas, reporting **150,000 → 2,000 tokens (98.7%)** on their example. Armin Ronacher's [Your MCP Doesn't Need 30 Tools: It Needs Code](https://lucumr.pocoo.org/2025/8/18/code-mcps/) argues the same from the server side. Simon Willison, [commenting](https://simonwillison.net/2025/Nov/4/code-execution-with-mcp/): *"I don't use MCP at all any more when working with coding agents — I find CLI utilities and libraries like Playwright Python to be a more effective way of achieving the same goals."*

### The recommendation: don't ship an MCP server yet

**Your CLI is already the thing the CLI camp is arguing for**: exit codes as a contract, versioned JSON on stdout, a documented `session.json` schema with jq recipes, and `emit` already normalizing coordinate conventions per target tool. That's Anthropic's "return semantically meaningful output" advice, already implemented.

Two hard constraints settle it:

1. **OpenAI's `mcp` tool cannot reach a local stdio server** — it requires `server_url` (remote Streamable HTTP/SSE), `connector_id`, or `tunnel_id` ([MCP and Connectors guide](https://developers.openai.com/api/docs/guides/tools-connectors-mcp)). A stdio MCP server would be visible only to subprocess-spawning hosts (Claude Code, Claude Desktop, Cursor). Meanwhile OpenAI now ships `local_shell`, `shell`, and `apply_patch` as **first-class tool types** — a CLI with `--json` and exit codes is reachable from every agent with a shell. **The CLI surface has strictly wider reach than a stdio MCP server.**
2. **An MCP server means JSON-RPC plumbing and, realistically, an async runtime** for progress notifications — which collides with `AGENTS.md`'s "do not add dependencies, async runtimes, single-implementation traits, or architectural layers."

**Do instead:** a `SKILL.md`. ~30 tokens idle, ~800 on match. An [Arize eval](https://arize.com/blog/mcp-vs-cli-skills-for-agents-what-our-eval-found-and-which-you-should-use/) found an 800-token skills file beat 28,000 tokens of MCP schemas with fewer tool calls. Contents: when to reach for it, the action loop, the exit-code table, the JSON shapes, and **the coordinate-space distinction** — which is the one thing an agent will otherwise get wrong.

### The coordinate convention every agent will get wrong

Both vendors converged on: **do not let the API downscale; downscale client-side and own the remap.**

- Anthropic ([computer use tool](https://platform.claude.com/docs/en/agents-and-tools/tool-use/computer-use-tool), current `computer_20251124`): `display_width_px`/`display_height_px` **must exactly match the image you send**; "clicks consistently offset in one direction" is diagnosed as a dimension mismatch; "relying on the server-side downscale leaves you without the scale factor you need." macOS Retina at DPR 2: "either downscale the screenshot by 2x before sending, or halve the coordinates Claude returns."
- OpenAI ([computer use](https://developers.openai.com/api/docs/guides/tools-computer-use)): "prefer `detail: "original"`… **make sure you remap model-generated coordinates from the downscaled coordinate space to the original image's coordinate space.**" Recommends 1440×900 / 1600×900 when downscaling.

Action enums for reference — Anthropic: `screenshot`, `left_click`, `type`, `key`, `mouse_move`, `scroll`, `left_click_drag`, `right_click`, `middle_click`, `double_click`, `triple_click`, `left_mouse_down`, `left_mouse_up`, `hold_key`, `wait`, `zoom`. OpenAI GA: `click`, `double_click`, `scroll`, `type`, `wait`, `keypress`, `drag`, `move`, `screenshot`, batched as an `actions[]` array per call. Both use a `keys`/`text` field on the *mouse* action for held modifiers rather than splitting into separate keyboard steps. **Neither has any assertion or wait-for-condition primitive — only a dumb `wait`.**

That gap is your positioning. The only verification either vendor offers is "take another screenshot and ask the model whether it worked." A deterministic `assert` with an exit code is strictly better, and it works in a shell loop, in CI, and inside a code-execution sandbox — none of which can call an MCP tool.

**A worked remap example (your coordinate space → downscaled screenshot space → back) is the single highest-value doc addition for agent consumers.**

---

## 6. Presentation

### The gold standard is esbuild, and it's directly copyable

[esbuild's API page](https://esbuild.github.io/api/) states the doc architecture *and its justification* in one paragraph:

> *"The API can be accessed in one of three languages: on the command line, in JavaScript, and in Go. The concepts and parameters are largely identical between the three languages so they will be presented together here instead of having separate documentation for each language."*
>
> *"You can switch between languages using the `CLI`, `JS`, and `Go` tabs in the top-right corner of each code example."*

Every option documented once, language-neutral prose, one code block that re-renders per tab. From the page source: two switcher groups — `mode3` (CLI/JS/Go, **defaulting to `cli`**) and `mode2` (JS/Go, defaulting to `js`) for features with no CLI equivalent — persisted in `localStorage` and synced across browser tabs via a `storage` listener. Two consequences worth stealing:

1. **CLI is the default tab** — a first-time visitor sees shell commands.
2. **The 2-way switcher makes capability asymmetry visible** — when a feature has no CLI form, the tab isn't offered rather than showing a fake equivalent.

Its escalation is justified by a felt symptom, not a feature list: *"However, using the command-line interface can become unwieldy if you need to pass many options to esbuild."*

### The other patterns

**Playwright** — route the big axis, tab the small one. Language is a full docs-tree fork (`/python/docs/intro`, `/java/…`) reached via a dropdown labeled with your current selection, so prose, install, and idioms are all correct for that language. Package manager (npm/yarn/pnpm) and TS/JS are in-page tabs. It **states a default** for the non-default option's own page: *"Under most circumstances, for end-to-end testing, you'll want to use `@playwright/test`… and not `playwright` directly."* And it **scopes the parity promise**: *"All core features for automating the browser are supported in all languages, while testing ecosystem integration is different."*

**k6** — the closest analogue to a compiled binary running user scripts. It discloses the runtime *exactly where the wrong assumption forms*, inline in the first-test walkthrough: **"Note that k6 is not built upon Node.js, and instead uses its own JavaScript runtime."** Its gap to avoid: an exhaustively indexed JS API and **no consolidated CLI command reference**.

**Terraform** — three doc cards (Configuration Language / CLI / Cloud), each tree staying in its lane with explicit hand-offs. Its CLI sections are **task-named** (Initializing Working Directories, Provisioning Infrastructure, Inspecting Infrastructure) with *Alphabetical List of Commands* last. That's the correct inversion of FFmpeg.

**ripgrep** — CLI-first, library deliberately de-emphasized. First code block in the README is `brew install ripgrep`. The library isn't in the README's structure at all; the honest support tier lives on the crate: *"This crate isn't ready for wide use yet."*

**jq** — analogy tagline (*"akin to `sed`, `awk`, `grep`, and friends for JSON data"*), CLI invocation documented **before** the language reference, and a rigid **Command / Input / Output** template on every example plus a `[Run]` link to a playground.

**Maestro** — three solution cards (Studio / CLI / Cloud) mirrored by URL trees, and JS framed as *overflow, not an alternative surface*. Also publishes `/llms-full.txt` and `/sitemap.md` for agents.

**FFmpeg — the anti-pattern, measured.** [`ffmpeg-all.html`](https://ffmpeg.org/ffmpeg-all.html) is 2,590,994 bytes / 68,367 lines; [`ffmpeg-filters.html`](https://ffmpeg.org/ffmpeg-filters.html) is 1.4 MB / 40,000 lines. No task-first layer anywhere. The rule that breaks every newcomer — option meaning depends on position relative to `-i` — is prose buried in a wall. CLI docs and libav* docs never cross-reference. **Diagnostic: when third parties re-publish your reference in a better shape ([ffmpeg-filters-docs](https://github.com/ayosec/ffmpeg-filters-docs)) and your own docs page defers task guidance to a community wiki, your reference has failed.**

**ImageMagick** — "you can use this from 25 languages" as an unranked prose list (four competing Ruby bindings, three Python) reads as *"you're on your own."*

**Docker** — the mild silo: the [Engine API page](https://docs.docker.com/reference/api/engine/) explains the relationship, but the [CLI reference](https://docs.docker.com/reference/cli/docker/) never mentions the API. Cross-link both directions.

### The README skeleton for `pixelactions`

1. **One-sentence definition with an analogy**, tool-shaped. No mention of "library" yet.
2. **Proof asset above the fold** — a GIF or a real transcript.
3. **Install.**
4. **The hero example: one CLI invocation and its real output.** Not a library snippet. It must teach the syntax while doing something useful.
5. **"Three ways to use this" — ranked, with a stated default.** *"Most people want (1)."* Label any unstable route in the same sentence, ripgrep-style.
6. **The escalation path as a narrative**, each step justified by a symptom: single command → chained commands → persistent protocol *("when you need retries, waits, and cancellation, one invocation per action stops being enough")*.
7. **The programmatic hero example — same task, same output** as the CLI hero. Same-task-across-surfaces is what makes the mapping legible.
8. **Architecture disclosure, one sentence, right here.** *"pixelactions is a single binary. There is no embedded interpreter — your bot is written in your language and drives the binary over a line protocol."*
9. **Protocol reference as a top-level section, not an appendix**, versioned, with a raw `echo '{...}' | pixelactions serve` transcript before any wrapper. **Cross-link both directions**: every CLI verb names its protocol message, every message names the equivalent CLI invocation.
10. **CLI reference — task-grouped first, alphabetical dump last.**
11. **Wrapper snippets, ranked, with support tier stated.**
12. **Cookbook / FAQ.**

### Failure modes to avoid

Burying the CLI under the library · leading with the library when the CLI is the product · **no stated default** · unlabeled code blocks · docs that assume you already picked a language · the monolith · semantics hidden in reference order · siloed surfaces with no bridge · reference for one surface only · overpromising parity · **committing to N language SDKs you can't staff** (CDKTF deprecated 2025-12-10; k6 forced to fork its own JS engine) · GUI-first quickstart for a CI tool.

---

## 7. Blunt list: what would be overengineering

| Overengineering | Instead |
|---|---|
| A daemon with a socket, PID file, auto-start | Child-of-caller stdio only. No listener, no lifetime you have to manage. |
| gRPC / protobuf / WebSocket | NDJSON over a pipe. It's 3.7 GiB/s and every language has it. |
| An embedded Lua/Rhai/JS interpreter | You may end up owning the interpreter (k6 → Sobek). The whole point is that the bot lives in the user's language. |
| PyO3 wheels + napi modules + cdylib | 70-combination cibuildwheel matrix, 3–4 CI systems, per-project cost that "has to be paid by every project" — and on macOS it makes `python3` the TCC grantee. |
| A 35-tool MCP server | Selection accuracy degrades past 30–50 tools; OpenAI can't reach stdio MCP anyway. A `SKILL.md` + `--json` reaches every agent with a shell. |
| A DSL/expression language in the flow files | Maestro's line is the right one: *"not everything can (or should) be written in YAML"* — which is the argument for an escape hatch to a real language, not for a bigger DSL. |
| Auto-waiting as an optional parameter | Make it the default execution mode of every observation, or it doesn't exist (272 vs 28,400). |
| An async runtime for the protocol | Blocking request/response over a pipe, one in flight at a time, like esbuild: *"You must send a response after receiving a request because the other end is blocking."* Add IDs so you *can* go concurrent later; don't build it now. |

**One thing that looks like overengineering but isn't:** a version handshake on the first protocol exchange (git's welcome-message-plus-capability-subset, ~30 lines). Without it, every protocol change breaks every bot forever — and ripgrep's `--json` shipping without a version field is the cautionary example.
