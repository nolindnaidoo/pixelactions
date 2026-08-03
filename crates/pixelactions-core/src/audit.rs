//! The record a run leaves behind.
//!
//! Observable polling, the watchdog and the corner kill switch all make a
//! run safe to *watch*. This makes one safe to **not** watch: a flow that
//! ran at 3am, or one a model drove, otherwise answers "what did it
//! actually do" with nothing at all.
//!
//! One line per event, NDJSON — the format the line protocol already
//! speaks, appendable and greppable. Everything here is pure: the caller
//! supplies the clock and owns the file, because neither belongs in a
//! crate with no platform dependencies.
//!
//! # What a record can never contain
//!
//! **Typed text.** A `type` step carries whatever was typed, which is how
//! passwords end up in log files. Nothing here strips it, because nothing
//! here ever sees it: [`Step::summary`](crate::flow::Step::summary)
//! renders `Type` as `"type N chars"`, and these records are built from
//! [`StepReport`], which carries that summary and never the step. The
//! property holds by construction rather than by remembering, and there
//! is a test pinning it.

use serde::{Deserialize, Serialize};

use crate::convert::ResolvedPoint;
use crate::report::{StepOutcome, StepReport};

/// One line of the log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    /// Opens a run. Everything after it, until the next `run`, belongs to
    /// this one.
    Run {
        /// RFC 3339, supplied by the caller — this crate holds no clock.
        utc: String,
        session: String,
        /// False for a resolved-but-not-performed run, so a reader can
        /// never mistake a plan for something that happened.
        executed: bool,
    },
    /// One step, after it finished.
    Step {
        utc: String,
        index: usize,
        /// The redacted label — never the step itself. See the module
        /// docs.
        summary: String,
        outcome: StepOutcome,
        /// Where the input actually went, **after** space conversion.
        /// The saved coordinate is in the session; this is the one that
        /// was sent, and the only one worth having when a run went wrong.
        points: Vec<ResolvedPoint>,
        elapsed_ms: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
}

impl Event {
    /// Open a run.
    #[must_use]
    pub fn run(utc: String, session: String, executed: bool) -> Self {
        Self::Run {
            utc,
            session,
            executed,
        }
    }

    /// Record a finished step.
    ///
    /// Takes the report rather than the step, which is what makes typed
    /// text unreachable from here.
    #[must_use]
    pub fn step(utc: String, report: &StepReport) -> Self {
        Self::Step {
            utc,
            index: report.index,
            summary: report.summary.clone(),
            outcome: report.outcome,
            points: report.points.clone(),
            elapsed_ms: report.elapsed_ms,
            detail: report.detail.clone(),
        }
    }

    /// The line to append, newline included.
    ///
    /// Serialization of these types cannot fail — every field is a
    /// string, a number, a bool or a `Vec` of those — so a failure here
    /// would be a bug in this module rather than bad input, and the
    /// record says so instead of vanishing.
    #[must_use]
    pub fn line(&self) -> String {
        match serde_json::to_string(self) {
            Ok(json) => format!("{json}\n"),
            Err(error) => format!("{{\"event\":\"broken\",\"detail\":\"{error}\"}}\n"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::convert::Space;
    use crate::flow::Step;

    fn point() -> ResolvedPoint {
        ResolvedPoint {
            x: 76.0,
            y: 15.0,
            space: Space::Logical,
            monitor: 0,
            scale: 2.0,
        }
    }

    fn report(summary: &str) -> StepReport {
        StepReport {
            index: 0,
            summary: summary.to_string(),
            outcome: StepOutcome::Executed,
            points: vec![point()],
            detail: None,
            elapsed_ms: 262,
        }
    }

    #[test]
    fn a_run_line_names_the_session_and_says_it_executed() {
        let line = Event::run("2026-08-03T21:00:00Z".into(), "/tmp/s".into(), true).line();
        assert!(line.contains(r#""event":"run""#), "{line}");
        assert!(line.contains("/tmp/s"), "{line}");
        assert!(line.contains(r#""executed":true"#), "{line}");
        assert!(line.ends_with('\n'), "one line, newline included");
    }

    #[test]
    fn a_step_line_carries_the_point_that_was_actually_sent() {
        let line = Event::step("2026-08-03T21:00:00Z".into(), &report("click submit")).line();
        // The coordinate after conversion is the forensic datum: the saved
        // one is already in the session, this is the one that was posted.
        assert!(line.contains("76"), "{line}");
        assert!(line.contains("15"), "{line}");
        assert!(line.contains(r#""outcome":"executed""#), "{line}");
        assert!(line.contains("262"), "{line}");
    }

    /// The property the module doc claims, pinned.
    ///
    /// A `type` step's text is never in a `StepReport` to begin with —
    /// `Step::summary` renders it as a character count — so the log cannot
    /// leak it even though nothing here strips anything.
    #[test]
    fn typed_text_cannot_reach_the_log() {
        let secret = "hunter2-correct-horse-battery-staple";
        let step = Step::Type {
            text: secret.to_string(),
        };
        let summary = step.summary();
        assert!(!summary.contains(secret), "summary leaked it: {summary}");
        assert_eq!(summary, "type 36 chars");

        let line = Event::step("2026-08-03T21:00:00Z".into(), &report(&summary)).line();
        assert!(!line.contains(secret), "the log leaked it: {line}");
        assert!(
            !line.contains("hunter2"),
            "the log leaked part of it: {line}"
        );
    }

    #[test]
    fn a_failure_carries_its_detail_and_a_success_omits_it() {
        let mut failed = report("changed panel");
        failed.outcome = StepOutcome::Failed;
        failed.detail = Some("did not change".into());
        assert!(
            Event::step("t".into(), &failed)
                .line()
                .contains("did not change")
        );
        assert!(
            !Event::step("t".into(), &report("click x"))
                .line()
                .contains("detail")
        );
    }

    #[test]
    fn every_line_is_one_line() {
        let line = Event::step("t".into(), &report("click submit")).line();
        assert_eq!(
            line.matches('\n').count(),
            1,
            "NDJSON is one record per line"
        );
    }
}
