"""A `pixelactions serve` stand-in, on the same wire.

The client's job is framing, ids, and turning answers into exceptions --
none of which needs a real binary or a real screen. Testing against a fake
keeps the suite runnable anywhere, including a CI runner with no display,
and lets the awkward answers (a failure, a wrong id, a dead pipe) be
produced on demand rather than waited for.
"""

import json
import sys

VERBS = [
    "click", "double_click", "drag", "scroll", "type", "key",
    "verify", "wait_for", "wait_gone", "changed", "pause",
]


def main() -> None:
    mode = sys.argv[1] if len(sys.argv) > 1 else "ok"
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        request = json.loads(line)
        verb, rid = request.get("do"), request.get("id")

        if verb == "hello":
            version = 99 if mode == "wrong-version" else 1
            reply = {"id": rid, "result": "welcome", "version": version,
                     "verbs": VERBS, "session": "/fake"}
        elif verb == "bye":
            reply = {"id": rid, "result": "closed"}
        elif verb == "relocate":
            reply = {"id": rid, "result": "located",
                     "moved": ["submit"], "missing": ["gone-label"]}
        elif mode == "error":
            reply = {"id": rid, "result": "error",
                     "detail": 'no selection labeled "nope"'}
        elif mode == "failed":
            reply = {"id": rid, "result": "done", "outcome": "failed",
                     "detail": "timed out after 30s", "elapsed_ms": 30000}
        elif mode == "wrong-id":
            reply = {"id": 999, "result": "done", "outcome": "executed",
                     "elapsed_ms": 1}
        else:
            reply = {"id": rid, "result": "done", "outcome": "verified",
                     "points": [{"x": 812.0, "y": 440.0, "space": "logical",
                                 "monitor": 0, "scale": 2.0}],
                     "elapsed_ms": 12}

        sys.stdout.write(json.dumps(reply) + "\n")
        sys.stdout.flush()


if __name__ == "__main__":
    main()
