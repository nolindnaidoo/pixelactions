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
trap 'rm -rf "$work"' EXIT

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
# Random noise, painted onto the root window. Two properties the matcher
# needs and text does not have:
#
#   - **Detailed.** A flat crop correlates with everything; pixelcoords
#     refuses one by name rather than matching it anywhere.
#   - **Unique.** Text repeats — the same word at two places makes a crop
#     match in more than one, and pixelcoords refuses that too, correctly,
#     because an ambiguous match yields no point worth acting on.
#
# Both refusals are the tool behaving properly. A scenario has to give it
# something a human would actually mark.
convert -size 1280x1024 xc: +noise Random "$work/noise.png"
display -window root "$work/noise.png"
sleep 1

echo "== capturing it the way pixelcoords would"
pixelcoords shoot --out "$work" >/dev/null 2>&1
shot="$work/screenshot-0.png"
[ -f "$shot" ] || { echo "  FAIL  no capture written"; exit 1; }

X=400; Y=300; W=240; H=90
dev=$(convert "$shot" -crop "${W}x${H}+${X}+${Y}" +repage \
  -format "%[fx:standard_deviation]" info: 2>/dev/null || echo 0)
dev_i=$(printf '%.0f' "$(echo "$dev * 100000" | bc -l 2>/dev/null || echo 0)")
echo "  region ${X},${Y} deviation ${dev_i}"
[ "${dev_i:-0}" -gt 100 ] || {
  echo "  FAIL  the capture is flat — nothing to mark, so no scenario is meaningful" >&2
  exit 1
}
convert "$shot" -crop "${W}x${H}+${X}+${Y}" +repage "$work/crop-0-target.png"

echo "== scenario: the region is locatable in a fresh capture"
report="$work/find.json"
pixelcoords find --session "$work" >"$report" 2>/dev/null || true
found=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["results"][0]["found"])' "$report" 2>/dev/null || echo "unreadable")
check "find locates the marked region" "True" "$found"

echo "== scenario: plan resolves without touching anything"
"$bin" plan --session "$work" click:target --json >"$work/plan.json" 2>/dev/null || true
planned=$(python3 -c 'import json,sys; p=json.load(open(sys.argv[1]))["steps"][0]["points"][0]; print("%.0f,%.0f" % (p["x"], p["y"]))' "$work/plan.json" 2>/dev/null || echo "unreadable")
# The click point of a rect is its centre, in physical pixels, because
# XTEST's space is the session's space and `Space::Auto` resolves to
# physical on Linux.
check "plan resolves to the region's centre" "$((X + W / 2)),$((Y + H / 2))" "$planned"

echo "== scenario: a click puts the pointer exactly there"
xdotool mousemove 0 0
# Kept, not discarded: a refusal here is the interesting outcome, and a
# scenario that hides why it refused is worth less than no scenario.
"$bin" run --session "$work" click:target --yes >"$work/run.log" 2>&1 || true
sed 's/^/      | /' "$work/run.log"
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
