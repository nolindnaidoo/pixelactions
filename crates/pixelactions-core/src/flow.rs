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
    /// Confirm a labeled region still matches its saved crop.
    Verify { target: String },
}

impl Step {
    /// Every session label this step needs. Resolution fails before any
    /// action runs when one is missing.
    pub fn targets(&self) -> Vec<&str> {
        match self {
            Self::Click { target } | Self::DoubleClick { target } | Self::Verify { target } => {
                vec![target.as_str()]
            }
            Self::Drag { from, to } => vec![from.as_str(), to.as_str()],
            Self::Type { .. } | Self::Key { .. } => Vec::new(),
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
            Self::Verify { target } => format!("verify {target}"),
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
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            relocate: true,
            verify: Verify::Each,
            space: Space::Auto,
            settle_ms: 120,
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
        let mut seen = Vec::new();
        for step in &self.steps {
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
