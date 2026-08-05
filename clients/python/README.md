# pixelactions (Python)

Drive desktop interactions from a session a human marked — click a
**label**, not a coordinate, and confirm it landed.

```bash
pip install pixelactions
```

Needs the `pixelactions` binary on PATH:

```bash
brew install nolindnaidoo/tap/pixelactions   # macOS
cargo install pixelactions                   # anywhere with Rust
```

## Use it

```python
from pixelactions import Session

with Session("~/captures/login") as ui:
    ui.click("email")
    ui.write("me@example.com")
    ui.key("tab")
    ui.write("hunter2")
    ui.click("submit")
    ui.wait("dashboard")
```

Mark `email`, `submit` and `dashboard` once, by hand, with
[pixelcoords](https://pixelcoords.dev). The script then survives the window
moving: every step re-locates its region before acting.

Compare the usual version, which is wrong the moment anything shifts:

```python
pyautogui.click(812, 440)
```

## Verbs

| Method | Does |
|---|---|
| `click(label)` · `double_click(label)` | click the region's point |
| `write(text)` | type text — `write`, because `type` is a keyword |
| `key(chord)` | a chord like `cmd+s`, arriving as keys not characters |
| `drag(start, end)` | press at one region, release at another |
| `scroll(label, n, horizontal=False)` | wheel over a region; negative reverses |
| `verify(label)` | is the region still what it was? |
| `wait(label)` · `gone(label)` | block until it matches, or until it disappears |
| `changed(label, tolerance=…)` | did it change? the strongest post-action check |
| `pause(ms)` | when there is genuinely no observable |
| `relocate()` | where is everything now, without acting |

## Failure is two different things

```python
from pixelactions import StepFailed, ProtocolError

try:
    ui.wait("dashboard")
except StepFailed as e:      # it ran, and the answer is no
    print(e.report.detail)   # "timed out after 30s (48 polls, best 0.71)"
except ProtocolError as e:   # the question was wrong
    print(e)                 # 'no selection labeled "dashbord"'
```

That is the same line the binary's exit codes draw, and it is worth
keeping: a timeout is a fact about the screen, a typo'd label is a fact
about your code.

Pass `raise_on_failure=False` to get the report back instead of an
exception.

## Settings

Anything the flow file's `[settings]` table takes:

```python
Session("~/captures/login", settings={"failsafe": False, "timeout_ms": 5000})
```

`failsafe` is the corner kill switch — park the pointer in a screen corner
and a run stops. It cannot work on Wayland, which will not report the
pointer, so a flow there must turn it off deliberately.

## How it works

A `pixelactions serve` child process and newline-delimited JSON over its
stdin and stdout — the
[line protocol](https://github.com/nolindnaidoo/pixelactions/blob/main/docs/PROTOCOL.md).
No FFI, no native module, **no dependencies**.

The version tracks the binary it drives. There is no separate client
version to reason about.

## Releasing

Manual, like the crates — PyPI will not let a version be replaced, only
yanked, so it is a button someone presses rather than a side effect of a
tag.

Run the **Publish the Python client** workflow from the Actions tab. It
defaults to TestPyPI; choose `pypi` for the real thing. The version comes
from `pyproject.toml`, which a test holds equal to the workspace — there
is no version input to get wrong.

Publishing uses [Trusted
Publishing](https://docs.pypi.org/trusted-publishers/), so there is no API
token stored anywhere. One-time setup on PyPI: add a publisher for
`nolindnaidoo/pixelactions`, workflow `publish-python.yml`, environment
`pypi`.

MIT.
