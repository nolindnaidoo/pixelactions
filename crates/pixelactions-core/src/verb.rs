//! `verb:argument` — the chained-argv form of a step.
//!
//! One invocation performing many actions is the cheapest possible
//! programmability: no protocol, no daemon, nothing to install. It is
//! what `cliclick` and `xdotool` have always done, and it collapses N
//! process spawns into one — which matters less for the spawn cost
//! (~3ms) than for doing a single relocation pass instead of N.
//!
//! The verbs deliberately mirror the flow file's actions one-for-one, so
//! learning either teaches the other. That rule is borrowed from tmux's
//! control mode: the protocol's verbs *are* the command set.

use crate::flow::Step;

/// Why a chained argument could not be read as a step.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum VerbError {
    #[error("{0:?} is not verb:argument — try click:submit, type:\"hello\", or wait:done")]
    Malformed(String),
    #[error(
        "unknown verb {0:?} — expected click, double, drag, type, key, verify, wait, gone, or pause"
    )]
    Unknown(String),
    #[error("drag needs from>to, e.g. drag:handle>dropzone (got {0:?})")]
    DragShape(String),
    #[error("pause needs milliseconds, e.g. pause:250 (got {0:?})")]
    PauseValue(String),
    #[error("{0} needs a label, e.g. {0}:submit")]
    EmptyLabel(String),
}

/// Parse one `verb:argument` argument into a step.
///
/// The argument half is taken verbatim after the first colon, so
/// `type:https://example.com` and `type:a:b` work without escaping — a
/// rule worth keeping, since text is the argument most likely to contain
/// a colon.
pub fn parse(argument: &str) -> Result<Step, VerbError> {
    let Some((verb, rest)) = argument.split_once(':') else {
        return Err(VerbError::Malformed(argument.to_string()));
    };
    let verb = verb.trim();

    let step = match verb {
        "click" => Step::Click {
            target: label(verb, rest)?,
        },
        "double" => Step::DoubleClick {
            target: label(verb, rest)?,
        },
        "verify" => Step::Verify {
            target: label(verb, rest)?,
        },
        "wait" => Step::WaitFor {
            target: label(verb, rest)?,
        },
        "gone" => Step::WaitGone {
            target: label(verb, rest)?,
        },
        "type" => Step::Type {
            text: rest.to_string(),
        },
        "key" => Step::Key {
            chord: label(verb, rest)?,
        },
        "drag" => {
            let Some((from, to)) = rest.split_once('>') else {
                return Err(VerbError::DragShape(rest.to_string()));
            };
            if from.trim().is_empty() || to.trim().is_empty() {
                return Err(VerbError::DragShape(rest.to_string()));
            }
            Step::Drag {
                from: from.trim().to_string(),
                to: to.trim().to_string(),
            }
        }
        "pause" => {
            let ms = rest
                .trim()
                .parse::<u64>()
                .map_err(|_| VerbError::PauseValue(rest.to_string()))?;
            Step::Pause { ms }
        }
        other => return Err(VerbError::Unknown(other.to_string())),
    };
    Ok(step)
}

/// Parse every argument, or fail on the first bad one.
///
/// All-or-nothing by design: a chain with a typo in step 7 must not
/// perform steps 1 through 6 first.
pub fn parse_all<I, S>(arguments: I) -> Result<Vec<Step>, VerbError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    arguments.into_iter().map(|a| parse(a.as_ref())).collect()
}

fn label(verb: &str, rest: &str) -> Result<String, VerbError> {
    let trimmed = rest.trim();
    if trimmed.is_empty() {
        return Err(VerbError::EmptyLabel(verb.to_string()));
    }
    Ok(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_verb_maps_to_its_flow_action() {
        assert_eq!(
            parse("click:submit"),
            Ok(Step::Click {
                target: "submit".into()
            })
        );
        assert_eq!(
            parse("double:icon"),
            Ok(Step::DoubleClick {
                target: "icon".into()
            })
        );
        assert_eq!(
            parse("verify:done"),
            Ok(Step::Verify {
                target: "done".into()
            })
        );
        assert_eq!(
            parse("wait:dialog"),
            Ok(Step::WaitFor {
                target: "dialog".into()
            })
        );
        assert_eq!(
            parse("gone:spinner"),
            Ok(Step::WaitGone {
                target: "spinner".into()
            })
        );
        assert_eq!(
            parse("key:cmd+s"),
            Ok(Step::Key {
                chord: "cmd+s".into()
            })
        );
        assert_eq!(parse("pause:250"), Ok(Step::Pause { ms: 250 }));
        assert_eq!(
            parse("drag:handle>zone"),
            Ok(Step::Drag {
                from: "handle".into(),
                to: "zone".into()
            })
        );
    }

    #[test]
    fn type_keeps_everything_after_the_first_colon() {
        // Text is the argument most likely to contain a colon, so it must
        // not need escaping.
        assert_eq!(
            parse("type:https://example.com"),
            Ok(Step::Type {
                text: "https://example.com".into()
            })
        );
        assert_eq!(
            parse("type:a:b:c"),
            Ok(Step::Type {
                text: "a:b:c".into()
            })
        );
    }

    #[test]
    fn type_may_be_deliberately_empty() {
        // Clearing a field by typing nothing is meaningless, but so is
        // rejecting it — an empty string is a valid thing to type.
        assert_eq!(
            parse("type:"),
            Ok(Step::Type {
                text: String::new()
            })
        );
    }

    #[test]
    fn an_argument_without_a_colon_is_malformed() {
        assert!(matches!(parse("click"), Err(VerbError::Malformed(_))));
    }

    #[test]
    fn an_unknown_verb_names_the_real_ones() {
        let error = parse("teleport:home").expect_err("unknown");
        let message = error.to_string();
        assert!(message.contains("click"), "lists the options: {message}");
    }

    #[test]
    fn a_label_verb_without_a_label_is_refused() {
        assert!(matches!(parse("click:"), Err(VerbError::EmptyLabel(_))));
        assert!(matches!(parse("wait:   "), Err(VerbError::EmptyLabel(_))));
    }

    #[test]
    fn drag_requires_both_ends() {
        assert!(matches!(parse("drag:handle"), Err(VerbError::DragShape(_))));
        assert!(matches!(parse("drag:>zone"), Err(VerbError::DragShape(_))));
        assert!(matches!(
            parse("drag:handle>"),
            Err(VerbError::DragShape(_))
        ));
    }

    #[test]
    fn pause_needs_a_number() {
        assert!(matches!(parse("pause:soon"), Err(VerbError::PauseValue(_))));
        assert!(matches!(parse("pause:-5"), Err(VerbError::PauseValue(_))));
    }

    #[test]
    fn whitespace_around_labels_is_tolerated() {
        assert_eq!(
            parse("click: submit "),
            Ok(Step::Click {
                target: "submit".into()
            })
        );
    }

    #[test]
    fn a_chain_fails_whole_rather_than_running_the_good_part() {
        // The important property: a typo in the last argument must not
        // perform the first three actions.
        let result = parse_all(["click:a", "type:hi", "key:cmd+s", "clik:b"]);
        assert!(matches!(result, Err(VerbError::Unknown(_))));
    }

    #[test]
    fn a_whole_chain_parses_in_order() {
        let steps = parse_all(["click:submit", "type:hello", "wait:done"]).expect("valid");
        assert_eq!(steps.len(), 3);
        assert_eq!(
            steps[2],
            Step::WaitFor {
                target: "done".into()
            }
        );
    }
}
