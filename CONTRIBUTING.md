# Contributing

Thanks for your interest. pixelactions runs verified on **macOS** and on
**Linux**, both X11 (XTEST) and Wayland (GNOME and KDE, via the portal +
EIS path) right now — Windows is the next milestone — so the most valuable
contributions are bug reports from real machines, small focused fixes,
and the platform work itself.

This tool moves a real mouse and keyboard. That raises the bar on
verification: see the definition of done in
[AGENTS.md](AGENTS.md).

## Bug reports

File through the issue form — it asks for what a report needs to be
actionable: OS and version, `pixelactions --version`, the full
`pixelactions doctor` output, and what you did / expected / got.

Two things make a report far easier to act on:

- **The `plan` output** for the same arguments. It resolves every
  coordinate without touching anything, which separates "the wrong place
  was computed" from "the right place was computed and the click did not
  land".
- **`pixelcoords find --session DIR`** at the moment of the failure. Most
  surprises turn out to be the screen no longer matching the session.

## Pull requests

- For anything larger than a bug fix, open an issue first so we agree on
  the approach before you invest time. Check
  [design/05-NON-GOALS.md](design/05-NON-GOALS.md) first — those are
  settled.
- Read [AGENTS.md](AGENTS.md) — the engineering-standards document
  (layout, control-flow style, coordinate conventions, testing bar,
  definition of done). CI enforces the mechanical parts. PRs that follow
  it get reviewed fast.
- Every change needs tests where tests are possible; every bug fix
  includes a regression test. Input synthesis and permission behavior are
  exempt — don't mock the window system. Verify those by running on real
  hardware and say so plainly.
- **Never claim injection behavior from a green test.** A passing suite
  proves the coordinates were computed, not that anything moved.
- Keep commits focused and describe the why, not just the what.

If you code with an AI assistant, point it at [CLAUDE.md](CLAUDE.md) /
[AGENTS.md](AGENTS.md) — they encode the same standards, and CI will
reject output that ignores them. [SKILL.md](SKILL.md) is the operator's
guide for an agent *driving* the tool, which is a different document.

## Building, testing, CI, releases

All in [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md): builds, the workspace
tour, the CI gates, coverage measurement, and the release process.
