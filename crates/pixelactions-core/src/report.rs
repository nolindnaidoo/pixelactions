//! Run reports — what happened, in a shape a machine can read.
//!
//! A report is written whether the run succeeded or not: the failure
//! case is the one worth reading. Every step records the point it acted
//! on *after conversion*, so a wrong click is diagnosable from the
//! artifact rather than by rerunning with a camera pointed at the screen.

use serde::{Deserialize, Serialize};

use crate::convert::ResolvedPoint;

/// What became of one step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepOutcome {
    /// Executed, and verification confirmed it.
    Verified,
    /// Executed; verification was not requested. Reported distinctly from
    /// `Verified` on purpose — "nothing errored" is not "it worked".
    Executed,
    /// Not executed: an earlier step failed.
    Skipped,
    /// Executed but verification failed, or the step itself failed.
    Failed,
    /// Not executed, on purpose: a guard said no. The kill switch, or a
    /// point that wandered outside its own marked region. Reported apart
    /// from `Failed` because "it did not work" and "I declined to try"
    /// call for different responses — one may be worth retrying, the
    /// other never is.
    Refused,
}

impl StepOutcome {
    /// The wire name, for printing and for the line protocol. Kept beside
    /// the serde attribute so the two can't drift apart — a client that
    /// matches on the JSON sees exactly what a human reading the terminal
    /// sees.
    pub fn name(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Executed => "executed",
            Self::Skipped => "skipped",
            Self::Failed => "failed",
            Self::Refused => "refused",
        }
    }
}

/// One step's record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepReport {
    pub index: usize,
    pub summary: String,
    pub outcome: StepOutcome,
    pub points: Vec<ResolvedPoint>,
    /// Present when the outcome is `Failed` — what went wrong, in words.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub elapsed_ms: u64,
}

/// The whole run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunReport {
    pub schema: u32,
    pub session: String,
    /// False for a plan-only run, so a consumer can never mistake a
    /// resolved plan for a performed one.
    pub executed: bool,
    pub steps: Vec<StepReport>,
}

impl RunReport {
    pub const SCHEMA: u32 = 1;

    /// The exit code this run earns: 3 when a guard refused, 1 when a
    /// step failed, 0 otherwise.
    ///
    /// A run stops at its first non-success, so at most one of these is
    /// ever present — the ordering states the precedence rather than
    /// resolving a real conflict.
    pub fn exit_code(&self) -> i32 {
        if self.steps.iter().any(|s| s.outcome == StepOutcome::Refused) {
            return 3;
        }
        if self.steps.iter().any(|s| s.outcome == StepOutcome::Failed) {
            return 1;
        }
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(index: usize, outcome: StepOutcome) -> StepReport {
        StepReport {
            index,
            summary: format!("step {index}"),
            outcome,
            points: Vec::new(),
            detail: None,
            elapsed_ms: 1,
        }
    }

    fn report(steps: Vec<StepReport>) -> RunReport {
        RunReport {
            schema: RunReport::SCHEMA,
            session: "s".into(),
            executed: true,
            steps,
        }
    }

    #[test]
    fn a_clean_run_exits_zero() {
        let run = report(vec![
            step(0, StepOutcome::Verified),
            step(1, StepOutcome::Executed),
        ]);
        assert_eq!(run.exit_code(), 0);
    }

    #[test]
    fn any_failure_exits_one() {
        let run = report(vec![
            step(0, StepOutcome::Verified),
            step(1, StepOutcome::Failed),
        ]);
        assert_eq!(run.exit_code(), 1);
    }

    #[test]
    fn skipped_steps_do_not_themselves_fail_the_run() {
        // Skipped means "an earlier step failed" — that earlier failure is
        // what sets the code, so this stays 0 when nothing actually failed.
        let run = report(vec![
            step(0, StepOutcome::Executed),
            step(1, StepOutcome::Skipped),
        ]);
        assert_eq!(run.exit_code(), 0);
    }

    #[test]
    fn executed_and_verified_are_distinct_in_the_wire_format() {
        let executed = serde_json::to_string(&StepOutcome::Executed).expect("serialize");
        let verified = serde_json::to_string(&StepOutcome::Verified).expect("serialize");
        assert_eq!(executed, "\"executed\"");
        assert_eq!(verified, "\"verified\"");
    }

    #[test]
    fn a_refusal_exits_three_not_one() {
        // "I declined to act" is operationally different from "it did not
        // work" — a CI job and an agent both need to tell them apart.
        let run = report(vec![step(0, StepOutcome::Refused)]);
        assert_eq!(run.exit_code(), 3);
    }

    #[test]
    fn the_printed_name_is_the_wire_name() {
        for outcome in [
            StepOutcome::Verified,
            StepOutcome::Executed,
            StepOutcome::Skipped,
            StepOutcome::Failed,
            StepOutcome::Refused,
        ] {
            let json = serde_json::to_string(&outcome).expect("serialize");
            assert_eq!(json, format!("\"{}\"", outcome.name()));
        }
    }

    #[test]
    fn a_report_round_trips() {
        let run = report(vec![step(0, StepOutcome::Verified)]);
        let text = serde_json::to_string(&run).expect("serialize");
        let back: RunReport = serde_json::from_str(&text).expect("deserialize");
        assert_eq!(run, back);
    }
}
