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
  - `verified` — an observation step whose condition held: a `verify`,
    `changed`, `wait_for`, or `wait_gone`. These are the only steps that
    assert anything about the screen.
  - `executed` — the input was posted. **"Nothing errored" is not "it
    worked"**, and this is where that distinction lives. Acting steps
    always report this, never `verified`: a click cannot confirm its own
    outcome, because acting on a region changes it. Assert the outcome by
    naming what should have changed.
  - `skipped` — an earlier step failed
  - `failed` — the step or its verification failed; `detail` says why
  - `refused` — a guard declined *before* anything was attempted; today
    that means the kill switch, a cursor found in a screen corner.
    Distinct from `failed` because a failure may be worth retrying and a
    refusal never is. A run containing one exits 3.

## Plan report (`plan --json`)

The same shape minus outcomes, with `"executed": false`. Useful for
diffing what a flow *would* do after a session is re-captured.

## Protocol messages (`serve`)

The line protocol has its own message shapes — one JSON object per line,
in and out — documented in [PROTOCOL.md](PROTOCOL.md). They reuse this
file's vocabulary: a `done` response carries the same `outcome` values
and the same resolved `points` as a run report's step.

## Doctor report (`doctor --json`)

Platform, native coordinate space, supported session schema, the
pixelcoords binary and its version, capability flags, macOS
Accessibility state, and the probe result when `--probe` was passed.
