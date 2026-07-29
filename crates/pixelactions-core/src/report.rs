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

    /// The exit code this run earns: 0 when every step is verified or
    /// executed, 1 when any failed.
    pub fn exit_code(&self) -> i32 {
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
    fn a_report_round_trips() {
        let run = report(vec![step(0, StepOutcome::Verified)]);
        let text = serde_json::to_string(&run).expect("serialize");
        let back: RunReport = serde_json::from_str(&text).expect("deserialize");
        assert_eq!(run, back);
    }
}
