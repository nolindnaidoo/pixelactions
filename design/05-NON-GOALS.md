# Non-goals — what this refuses to become

Written early on purpose. pixelcoords' non-goals section ended every
"why don't you add…" debate before it started; this does the same.

- **A scripting language.** No loops, no conditionals, no variables, no
  expression evaluator. A flow is a list of steps. Anything needing
  branching should be a real program calling pixelactions step by step.
  This is the line between a tool and an RPA suite, and RPA suites are
  a solved, crowded, expensive market.
- **A recorder.** "Record my clicks and replay them" produces
  unreviewable, unmaintainable artifacts that break on the first UI
  change — the failure mode of every macro recorder ever shipped. Marks
  come from pixelcoords, deliberately, with a human choosing what
  matters.
- **An accessibility-tree automation tool.** The a11y-first tools
  (terminator, agent-desktop, Appium, dogtail) are good and we are not
  competing with them. pixelactions is for where trees don't exist:
  canvas apps, games, streamed pixels (Citrix/VDI/VNC), legacy apps,
  cross-app OS flows. Positioning it as a general replacement would be
  a regression and would read as one.
- **A browser automation tool.** Playwright and Selenium own that, and
  own it well.
- **A daemon / always-on agent.** It runs, it acts, it exits. (If a
  platform forces a helper process, it's scoped to the run.)
- **Cloud anything.** No accounts, no telemetry, no upload. Same as
  pixelcoords.
- **Driving elevated/secure surfaces.** Windows UAC prompts, the secure
  desktop, and the login screen are unreachable by design — we say so
  rather than pretending.
- **Silent operation on Wayland.** Consent dialogs are the platform's
  security model working correctly. We will make them once-per-session
  where the portal allows, and never route around them.
