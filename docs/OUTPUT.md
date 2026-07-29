# Output reference

## Run report (`run --json`)

```json
{
  "schema": 1,
  "session": "/Users/you/Downloads/pixelcoords-captures/20260728-182121-117",
  "executed": true,
  "steps": [
    {
      "index": 0,
      "summary": "click submit",
      "outcome": "verified",
      "points": [
        { "x": 430.0, "y": 170.0, "space": "logical", "monitor": 0, "scale": 2.0 }
      ],
      "elapsed_ms": 412
    }
  ]
}
```

- **`executed`** is `false` for a plan-only run, so a consumer can never
  mistake a resolved plan for a performed one.
- **`points`** are the coordinates *actually used* — relocation
  corrections included. A reader sees where the click went, not where
  the session said it would.
- **`outcome`** distinguishes:
  - `verified` — executed, and a fresh capture confirmed the region
  - `executed` — executed; verification was not requested or not
    applicable (keyboard steps have no region). **"Nothing errored" is
    not "it worked"**, and this is where that distinction lives.
  - `skipped` — an earlier step failed
  - `failed` — the step or its verification failed; `detail` says why

## Plan report (`plan --json`)

The same shape minus outcomes, with `"executed": false`. Useful for
diffing what a flow *would* do after a session is re-captured.

## Doctor report (`doctor --json`)

Platform, native coordinate space, supported session schema, the
pixelcoords binary and its version, capability flags, macOS
Accessibility state, and the probe result when `--probe` was passed.
