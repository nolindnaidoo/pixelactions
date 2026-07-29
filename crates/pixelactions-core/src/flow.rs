//! The flow file: a list of steps referencing a pixelcoords session by
//! **label**, never by raw coordinate.
//!
//! That indirection is the point. A label survives the UI moving; a
//! coordinate does not. It also keeps a flow reviewable in git — a diff
//! shows intent ("click submit") rather than arithmetic.
//!
//! Parsing is strict: unknown keys are errors, not silent no-ops, so a
//! typo fails loudly at parse time rather than skipping a step at run
//! time. (Session parsing, by contrast, is deliberately tolerant — see
//! AGENTS.md on the compatibility contract.)

use serde::{Deserialize, Serialize};

use crate::convert::Space;

/// How thoroughly a run verifies itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Verify {
    /// Confirm every acted-on region after its step.
    #[default]
    Each,
    /// Confirm once, after the last step.
    End,
    /// Do not verify. The run reports success only in the sense that
    /// nothing errored — say so in the report.
    None,
}

/// Which way a scroll goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Axis {
    /// Up and down. Positive amounts scroll **down**, matching every
    /// platform's own wheel convention.
    #[default]
    Vertical,
    /// Left and right, for side-scrolling panes. Positive scrolls right.
    Horizontal,
}

/// What a step does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Step {
    /// Click the resolved point of a labeled region.
    Click { target: String },
    /// Double-click the resolved point of a labeled region.
    DoubleClick { target: String },
    /// Type literal text through the platform's Unicode path — layout
    /// independent, and unable to express shortcuts (use `key`).
    Type { text: String },
    /// Press a chord of physical keys, e.g. `cmd+s`.
    Key { chord: String },
    /// Press at one region's point, move, release at another's.
    Drag { from: String, to: String },
    /// Hover a region and turn the wheel over it.
    ///
    /// `target` picks *what* to scroll — a wheel event goes to whatever
    /// is under the cursor — and resolves exactly like a click's does.
    /// `amount` is the one quantity in this tool that is **not**
    /// exact: it counts 15° wheel clicks, and how far that moves depends
    /// on the reader's own OS scroll-speed setting. Scroll until
    /// something is visible (`wait_for`), never a fixed distance.
    Scroll {
        target: String,
        amount: i32,
        #[serde(default)]
        axis: Axis,
    },
    /// Confirm a labeled region still matches its saved crop.
    Verify { target: String },
    /// Poll until a labeled region matches its saved crop again, or the
    /// timeout expires. The honest alternative to guessing with a sleep:
    /// the OS accepts an event long before an app finishes reacting.
    WaitFor { target: String },
    /// Poll until a labeled region STOPS matching — "wait until this
    /// spinner goes away", "wait until the button changes state".
    WaitGone { target: String },
    /// Wait a fixed duration. Present because some waits genuinely have
    /// no observable, and pretending otherwise would push people to
    /// sleep-and-hope outside the tool.
    Pause { ms: u64 },
}

impl Step {
    /// Every session label this step needs. Resolution fails before any
    /// action runs when one is missing.
    pub fn targets(&self) -> Vec<&str> {
        match self {
            Self::Click { target }
            | Self::DoubleClick { target }
            | Self::Scroll { target, .. }
            | Self::Verify { target }
            | Self::WaitFor { target }
            | Self::WaitGone { target } => vec![target.as_str()],
            Self::Drag { from, to } => vec![from.as_str(), to.as_str()],
            Self::Type { .. } | Self::Key { .. } | Self::Pause { .. } => Vec::new(),
        }
    }

    /// Whether this step posts input, as opposed to only looking at the
    /// screen.
    ///
    /// The distinction decides which regions must be *present* before a
    /// run starts. Acting on a region whose position cannot be trusted
    /// clicks an unknown thing; looking for one that is absent is the
    /// entire job of `wait_for`.
    pub fn injects(&self) -> bool {
        match self {
            Self::Click { .. }
            | Self::DoubleClick { .. }
            | Self::Drag { .. }
            | Self::Scroll { .. }
            | Self::Type { .. }
            | Self::Key { .. } => true,
            Self::Verify { .. }
            | Self::WaitFor { .. }
            | Self::WaitGone { .. }
            | Self::Pause { .. } => false,
        }
    }

    /// A short human label for reports and dry-run output.
    pub fn summary(&self) -> String {
        match self {
            Self::Click { target } => format!("click {target}"),
            Self::DoubleClick { target } => format!("double-click {target}"),
            Self::Type { text } => format!("type {} chars", text.chars().count()),
            Self::Key { chord } => format!("key {chord}"),
            Self::Drag { from, to } => format!("drag {from} -> {to}"),
            Self::Scroll {
                target,
                amount,
                axis,
            } => {
                let way = match (axis, amount.is_negative()) {
                    (Axis::Vertical, false) => "down",
                    (Axis::Vertical, true) => "up",
                    (Axis::Horizontal, false) => "right",
                    (Axis::Horizontal, true) => "left",
                };
                format!("scroll {target} {way} {}", amount.abs())
            }
            Self::Verify { target } => format!("verify {target}"),
            Self::WaitFor { target } => format!("wait for {target}"),
            Self::WaitGone { target } => format!("wait until {target} is gone"),
            Self::Pause { ms } => format!("pause {ms}ms"),
        }
    }
}

/// Run-wide settings. Every field has a defensible default so a minimal
/// flow file is three lines.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Settings {
    /// Re-locate regions against a fresh capture before acting, and act
    /// on the corrected coordinates.
    pub relocate: bool,
    /// How thoroughly to verify.
    pub verify: Verify,
    /// Coordinate space to resolve into. `Auto` is what the platform's
    /// input API wants and is almost always right.
    pub space: Space,
    /// Milliseconds to settle between steps. Not a substitute for
    /// verification — the OS accepts an event long before an app has
    /// finished reacting to it.
    pub settle_ms: u64,
    /// How long a `wait_for` / `wait_gone` step may poll before it fails.
    pub timeout_ms: u64,
    /// Milliseconds between polls while waiting. Each poll is a screen
    /// capture, so this is a real cost, not a formality.
    pub poll_ms: u64,
    /// Abort the run if the cursor is found in a screen corner before a
    /// step. The kill switch: grabbing the mouse is what a person does
    /// when automation goes wrong, and a corner needs no aim. On by
    /// default — turning it off means nothing but the watchdog can stop
    /// a run from the outside.
    pub failsafe: bool,
    /// How close to a corner counts, in the input space's own units.
    pub failsafe_margin: f64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            relocate: true,
            verify: Verify::Each,
            space: Space::Auto,
            settle_ms: 120,
            timeout_ms: 10_000,
            poll_ms: 400,
            failsafe: true,
            failsafe_margin: 10.0,
        }
    }
}

/// A parsed flow file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Flow {
    /// Path to the pixelcoords session directory (or its session.json).
    pub session: String,
    #[serde(default)]
    pub settings: Settings,
    // Defaulted so a step-less flow reaches the Empty check and gets a
    // sentence a human can act on, rather than serde's "missing field".
    #[serde(rename = "step", default)]
    pub steps: Vec<Step>,
}

/// Parse errors, actionable by construction — the message names what to
/// fix, in the tradition of the sister tool's strict config parsing.
#[derive(Debug, thiserror::Error)]
pub enum FlowError {
    #[error("flow file is not valid TOML: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("flow has no steps — nothing to run")]
    Empty,
}

impl Flow {
    /// Parse a flow from TOML source.
    pub fn parse(source: &str) -> Result<Self, FlowError> {
        let flow: Self = toml::from_str(source)?;
        if flow.steps.is_empty() {
            return Err(FlowError::Empty);
        }
        Ok(flow)
    }

    /// Every distinct label the flow references, in first-use order.
    pub fn targets(&self) -> Vec<&str> {
        self.labels(|_| true)
    }

    /// The labels a run will actually *act on*, in first-use order.
    ///
    /// These are the ones that must be found before anything is
    /// injected. The rest — a `wait_for` waiting for a dialog, a
    /// `wait_gone` waiting for a spinner to clear — are by definition
    /// allowed to be absent, and demanding them up front would make
    /// those verbs impossible to use.
    pub fn acting_targets(&self) -> Vec<&str> {
        self.labels(Step::injects)
    }

    fn labels(&self, keep: impl Fn(&Step) -> bool) -> Vec<&str> {
        let mut seen = Vec::new();
        for step in self.steps.iter().filter(|step| keep(step)) {
            for target in step.targets() {
                if !seen.contains(&target) {
                    seen.push(target);
                }
            }
        }
        seen
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_injecting_steps_must_be_present_before_a_run() {
        assert!(Step::Click { target: "a".into() }.injects());
        assert!(Step::Type { text: "hi".into() }.injects());
        assert!(
            Step::Key {
                chord: "cmd+s".into()
            }
            .injects()
        );
        assert!(
            Step::Scroll {
                target: "a".into(),
                amount: 1,
                axis: Axis::Vertical
            }
            .injects()
        );
        // Observation only — these read the screen and never move it.
        assert!(!Step::Verify { target: "a".into() }.injects());
        assert!(!Step::WaitFor { target: "a".into() }.injects());
        assert!(!Step::WaitGone { target: "a".into() }.injects());
        assert!(!Step::Pause { ms: 10 }.injects());
    }

    #[test]
    fn a_wait_for_target_is_not_required_to_exist_up_front() {
        // The regression: a flow that clicks one region and then waits
        // for another used to demand *both* before starting, which made
        // wait_for — the whole point of the verb — impossible to use.
        let flow = Flow::parse(
            "session = \"s\"\n\n\
             [[step]]\naction = \"click\"\ntarget = \"submit\"\n\n\
             [[step]]\naction = \"wait_for\"\ntarget = \"confirmation\"\n\n\
             [[step]]\naction = \"wait_gone\"\ntarget = \"spinner\"\n",
        )
        .expect("valid");

        assert_eq!(flow.targets(), vec!["submit", "confirmation", "spinner"]);
        assert_eq!(flow.acting_targets(), vec!["submit"]);
    }

    #[test]
    fn a_scroll_step_reads_its_amount_and_defaults_to_vertical() {
        let flow = Flow::parse(
            "session = \"s\"\n\n[[step]]\naction = \"scroll\"\ntarget = \"results\"\namount = -3\n",
        )
        .expect("valid");
        assert_eq!(
            flow.steps[0],
            Step::Scroll {
                target: "results".into(),
                amount: -3,
                axis: Axis::Vertical,
            }
        );
    }

    #[test]
    fn a_scroll_needs_an_amount_rather_than_guessing_one() {
        // The least predictable value in the tool is the one place a
        // silent default would hurt most.
        let flow =
            Flow::parse("session = \"s\"\n\n[[step]]\naction = \"scroll\"\ntarget = \"x\"\n");
        assert!(flow.is_err(), "amount is required");
    }

    #[test]
    fn a_scroll_names_the_direction_a_human_would_say() {
        let down = Step::Scroll {
            target: "list".into(),
            amount: 3,
            axis: Axis::Vertical,
        };
        let left = Step::Scroll {
            target: "list".into(),
            amount: -2,
            axis: Axis::Horizontal,
        };
        assert_eq!(down.summary(), "scroll list down 3");
        assert_eq!(left.summary(), "scroll list left 2");
    }

    #[test]
    fn a_scroll_targets_the_region_it_hovers() {
        let step = Step::Scroll {
            target: "pane".into(),
            amount: 1,
            axis: Axis::Vertical,
        };
        assert_eq!(step.targets(), vec!["pane"]);
    }

    const MINIMAL: &str = r#"
session = "~/captures/20260728"

[[step]]
action = "click"
target = "submit"
"#;

    #[test]
    fn a_minimal_flow_parses_with_defensible_defaults() {
        let flow = Flow::parse(MINIMAL).expect("valid");
        assert_eq!(flow.steps.len(), 1);
        assert!(flow.settings.relocate, "relocation defaults on");
        assert_eq!(flow.settings.verify, Verify::Each);
        assert_eq!(flow.settings.space, Space::Auto);
    }

    #[test]
    fn every_action_kind_round_trips() {
        let source = r#"
session = "s"

[[step]]
action = "click"
target = "a"

[[step]]
action = "double_click"
target = "b"

[[step]]
action = "type"
text = "hello"

[[step]]
action = "key"
chord = "cmd+s"

[[step]]
action = "drag"
from = "handle"
to = "zone"

[[step]]
action = "verify"
target = "done"
"#;
        let flow = Flow::parse(source).expect("valid");
        assert_eq!(flow.steps.len(), 6);
        assert_eq!(flow.targets(), vec!["a", "b", "handle", "zone", "done"]);
    }

    #[test]
    fn an_unknown_key_is_an_error_not_a_silent_skip() {
        let source = r#"
session = "s"

[[step]]
action = "click"
targt = "typo"
"#;
        assert!(matches!(Flow::parse(source), Err(FlowError::Toml(_))));
    }

    #[test]
    fn an_unknown_action_is_an_error() {
        let source = r#"
session = "s"

[[step]]
action = "teleport"
target = "a"
"#;
        assert!(matches!(Flow::parse(source), Err(FlowError::Toml(_))));
    }

    #[test]
    fn an_empty_flow_is_refused() {
        assert!(matches!(
            Flow::parse(r#"session = "s""#),
            Err(FlowError::Empty)
        ));
    }

    #[test]
    fn targets_are_deduplicated_in_first_use_order() {
        let source = r#"
session = "s"

[[step]]
action = "click"
target = "b"

[[step]]
action = "click"
target = "a"

[[step]]
action = "verify"
target = "b"
"#;
        assert_eq!(
            Flow::parse(source).expect("valid").targets(),
            vec!["b", "a"]
        );
    }

    #[test]
    fn waiting_and_pausing_parse() {
        let source = r#"
session = "s"

[settings]
timeout_ms = 3000
poll_ms = 250

[[step]]
action = "wait_for"
target = "dialog"

[[step]]
action = "wait_gone"
target = "spinner"

[[step]]
action = "pause"
ms = 500
"#;
        let flow = Flow::parse(source).expect("valid");
        assert_eq!(flow.settings.timeout_ms, 3000);
        assert_eq!(flow.settings.poll_ms, 250);
        assert_eq!(flow.targets(), vec!["dialog", "spinner"]);
        assert_eq!(flow.steps[2].summary(), "pause 500ms");
    }

    #[test]
    fn keyboard_steps_need_no_targets() {
        let step = Step::Type { text: "hi".into() };
        assert!(step.targets().is_empty());
        assert_eq!(step.summary(), "type 2 chars");
    }

    #[test]
    fn settings_reject_unknown_keys_too() {
        let source = r#"
session = "s"

[settings]
reloacte = true

[[step]]
action = "click"
target = "a"
"#;
        assert!(Flow::parse(source).is_err());
    }
}
