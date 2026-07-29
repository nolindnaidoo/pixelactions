# Changelog

All notable changes are recorded here, written as they land. Versions
follow [Semantic Versioning](https://semver.org). Pre-1.0 policy:
**minor** (0.x.0) for new features and for any breaking change to the
CLI, the flow file, or the line protocol; **patch** (0.x.y) for fixes.
1.0.0 comes when those three are declared stable.

## Unreleased

Requires **pixelcoords 0.1.2 or newer**, enforced before any run: older
captures composite the mouse pointer into the image, which makes
relocation unreliable in a way that presents as flakiness.

Runs report as they go, rather than printing nothing for ten seconds and
then a wall of text. Each region confirmed before the run, and each step
as it finishes, appears immediately.

The pre-run check also stopped matching regions it will never act on: it
swept every template in the session, including ones only ever waited for.
Measured on a three-region session, that alone was 5.2s against 2.9s.

Nothing is published yet. The loop works end to end on macOS — resolve a
label to its verified point, re-locate it against a fresh capture, act,
and confirm — driven three ways: chained argv, flow files, and the
`serve` line protocol. Windows and X11 are the next milestone.
