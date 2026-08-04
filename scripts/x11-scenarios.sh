#!/usr/bin/env bash
# End-to-end scenarios against a real X server.
#
# What this proves that nothing else in CI can: a session marked from a
# real screen resolves to a coordinate, that coordinate survives space
# conversion, XTEST accepts it, and **the pointer actually lands on the
# region a human would have marked**. Every step of that is a real
# capture, a real conversion and a real synthetic event — the parts
# `AGENTS.md` says cannot be verified headless.
#
# Run it under a display:
#   xvfb-run -a --server-args="-screen 0 1280x1024x24" scripts/x11-scenarios.sh
#
# Needs: xdotool (to read back where the pointer went), ImageMagick (to
# cut a crop the way the overlay would), and pixelcoords on PATH.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
bin="$root/target/debug/pixelactions"
[ -x "$bin" ] || bin="$root/target/release/pixelactions"
command -v pixelcoords >/dev/null || {
  echo "pixelcoords is not on PATH — the scenarios drive the real pairing" >&2
  exit 2
}
command -v xdotool >/dev/null || { echo "xdotool is needed to read the pointer back" >&2; exit 2; }
command -v convert >/dev/null || { echo "ImageMagick is needed to cut a crop" >&2; exit 2; }

work="$(mktemp -d)"
trap 'rm -rf "$work"; kill %1 2>/dev/null || true' EXIT

export XDG_SESSION_TYPE=x11
export XDG_STATE_HOME="$work/state"

pass=0
fail=0
check() { # check <name> <expected> <actual>
  if [ "$2" = "$3" ]; then
    echo "  ok    $1"
    pass=$((pass + 1))
  else
    echo "  FAIL  $1: expected [$2], got [$3]"
    fail=$((fail + 1))
  fi
}

echo "== putting something detailed on screen"
# A flat screen is degenerate for normalized cross-correlation — a crop of
# one colour correlates with everything and `find` reports nothing. Text
# gives the matcher something to lock onto, which is what a real desktop
# has and an empty X root does not.
xmessage -geometry 600x400+100+100 \
  "pixelactions scenario target
the quick brown fox jumps over the lazy dog
0123456789 ABCDEFGHIJ klmnopqrst
$(date +%s) deterministic-enough for one run" &
sleep 2

echo "== capturing it the way pixelcoords would"
pixelcoords shoot --out "$work" >/dev/null 2>&1
shot="$work/screenshot-0.png"
[ -f "$shot" ] || { echo "  FAIL  no capture written"; exit 1; }

# A region inside the xmessage window, well away from its edges.
X=200; Y=200; W=240; H=90
convert "$shot" -crop "${W}x${H}+${X}+${Y}" +repage "$work/crop-0-target.png"

python3 - "$work" "$X" "$Y" "$W" "$H" <<'PY'
import json, sys, subprocess
work, x, y, w, h = sys.argv[1], *map(int, sys.argv[2:6])
# The screen is 1280x1024 at scale 1 — X11 has no per-monitor scaling to
# describe here, which is why this job is the X11 path and not a stand-in
# for macOS.
px = {"x": x, "y": y, "w": w, "h": h}
json.dump({
    "schema": 1,
    "app": {"name": "pixelcoords", "version": "0.7.0"},
    "created_utc": "2026-01-01T00:00:00Z",
    "platform": "linux", "capture": None, "name": "x11 scenarios",
    "monitors": [{"index": 0, "name": "screen", "primary": True,
                  "origin_px": {"x": 0, "y": 0},
                  "size_px": {"w": 1280, "h": 1024}, "scale": 1.0}],
    "target": None, "measures": [],
    "selections": [{"shape": "rect", "label": "target", "monitor": 0,
                    "px": px, "global_px": px, "rot_deg": None,
                    "window_px": None, "crop": "crop-0-target.png",
                    "color": None}],
}, open(f"{work}/session.json", "w"))
PY

echo "== scenario: the region is locatable in a fresh capture"
found=$(pixelcoords find --session "$work" 2>/dev/null \
  | python3 -c 'import json,sys; print(json.load(sys.stdin)["results"][0]["found"])')
check "find locates the marked region" "True" "$found"

echo "== scenario: plan resolves without touching anything"
planned=$("$bin" plan --session "$work" click:target --json 2>/dev/null \
  | python3 -c 'import json,sys; p=json.load(sys.stdin)["steps"][0]["points"][0]; print(f"{p[\"x\"]:.0f},{p[\"y\"]:.0f}")')
# The click point of a rect is its centre, in physical pixels, because
# XTEST's space is the session's space and `Space::Auto` resolves to
# physical on Linux.
check "plan resolves to the region's centre" "$((X + W / 2)),$((Y + H / 2))" "$planned"

echo "== scenario: a click puts the pointer exactly there"
xdotool mousemove 0 0
"$bin" run --session "$work" click:target --yes >/dev/null 2>&1 || true
landed=$(xdotool getmouselocation --shell | awk -F= '/^X=/{x=$2} /^Y=/{y=$2} END{print x","y}')
check "the pointer landed on the marked region" "$planned" "$landed"

echo "== scenario: the audit log records it"
log="$XDG_STATE_HOME/pixelactions/audit.ndjson"
[ -f "$log" ] && recorded=yes || recorded=no
check "a record was written" "yes" "$recorded"
if [ -f "$log" ]; then
  steps=$(grep -c '"event":"step"' "$log" || true)
  check "the record has a step line" "1" "$steps"
fi

echo "== scenario: exit codes are the API"
"$bin" run --session "$work" click:nosuchlabel --yes >/dev/null 2>&1 && code=0 || code=$?
check "an unknown label exits 2" "2" "$code"
"$bin" run --session "$work" click:target >/dev/null 2>&1 && code=0 || code=$?
check "acting without --yes exits 3" "3" "$code"

echo
echo "== $pass passed, $fail failed"
[ "$fail" -eq 0 ]
