//! The run loop: act on a resolved plan, verify, report.
//!
//! Ordering rules that exist because a half-executed flow is the worst
//! outcome this tool can produce:
//!
//! - Planning is total. Every label resolved before this module runs.
//! - A failed step stops the run; the rest are recorded as skipped, not
//!   silently dropped.
//! - Verification failure is a step failure. "The click was posted" is
//!   not "the click worked", and the report distinguishes them.
//! - A watchdog bounds the whole run, so a flow can never sit there
//!   holding your mouse forever.

use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use pixelactions_core::flow::{Flow, Step, Verify};
use pixelactions_core::plan::Plan;
use pixelactions_core::report::{RunReport, StepOutcome, StepReport};

use crate::inject::{Button, Injector};
use crate::verify;

/// How long a whole run may take before it is abandoned. Generous for a
/// human flow, short enough that a wedged run does not own the machine.
pub const WATCHDOG: Duration = Duration::from_secs(120);

/// Before acting: confirm every region the flow will touch is still on
/// screen, unambiguously.
///
/// This is the difference between automation and vandalism. A flow whose
/// targets have moved would otherwise click whatever now occupies those
/// coordinates.
pub fn preflight(
    flow: &Flow,
    session: &Path,
    verifier: &mut dyn FnMut(&Path, Option<&str>) -> Result<verify::FindReport>,
) -> Result<()> {
    if !flow.settings.relocate {
        return Ok(());
    }
    let targets = flow.targets();
    if targets.is_empty() {
        return Ok(());
    }

    let report = verifier(session, None)?;
    let unconfirmed: Vec<String> = targets
        .iter()
        .filter(|label| !report.is_confirmed(label))
        .map(|label| {
            let reason = report
                .result_for(label)
                .map_or_else(|| "not in the report".to_string(), describe);
            format!("{label} ({reason})")
        })
        .collect();

    if unconfirmed.is_empty() {
        return Ok(());
    }
    bail!(
        "the screen no longer matches the session: {}. Re-mark with pixelcoords, or set \
         relocate = false in the flow to act on the saved coordinates anyway",
        unconfirmed.join("; ")
    )
}

/// A human sentence for one region's state, using the score and drift
/// pixelcoords reports.
fn describe(result: &verify::FindResult) -> String {
    if result.ambiguous {
        return format!("matched in more than one place, score {:.3}", result.score);
    }
    if !result.found {
        return result.reason.clone().unwrap_or_else(|| {
            format!("no match above the floor, best score {:.3}", result.score)
        });
    }
    match result.delta {
        Some(delta) if delta.dx != 0 || delta.dy != 0 => {
            format!("moved by ({}, {})", delta.dx, delta.dy)
        }
        _ => "unchanged".to_string(),
    }
}

/// Execute a plan.
///
/// Always returns a report: a run that failed is a run whose report says
/// which step failed and why. Errors are outcomes here, not exceptions.
/// `session` is the resolved session directory — needed to ask
/// pixelcoords for verification.
pub fn execute(
    injector: &mut dyn Injector,
    flow: &Flow,
    plan: &Plan,
    session: &Path,
    verifier: &mut dyn FnMut(&Path, Option<&str>) -> Result<verify::FindReport>,
) -> RunReport {
    let started = Instant::now();
    let settle = Duration::from_millis(flow.settings.settle_ms);
    let mut steps: Vec<StepReport> = Vec::with_capacity(plan.steps.len());
    let mut failed = false;

    for planned in &plan.steps {
        if failed {
            steps.push(record(planned, StepOutcome::Skipped, None, 0));
            continue;
        }
        if started.elapsed() > WATCHDOG {
            steps.push(record(
                planned,
                StepOutcome::Failed,
                Some("watchdog: the run exceeded its time budget".into()),
                0,
            ));
            failed = true;
            continue;
        }

        let step_started = Instant::now();
        let outcome = perform(injector, &planned.step, planned, settle)
            .and_then(|()| confirm(flow, &planned.step, session, verifier));
        let elapsed = step_started.elapsed().as_millis() as u64;

        match outcome {
            Ok(outcome) => steps.push(record(planned, outcome, None, elapsed)),
            Err(error) => {
                steps.push(record(
                    planned,
                    StepOutcome::Failed,
                    Some(format!("{error:#}")),
                    elapsed,
                ));
                failed = true;
            }
        }
    }

    RunReport {
        schema: RunReport::SCHEMA,
        session: session.display().to_string(),
        executed: true,
        steps,
    }
}

fn record(
    planned: &pixelactions_core::plan::PlannedStep,
    outcome: StepOutcome,
    detail: Option<String>,
    elapsed_ms: u64,
) -> StepReport {
    StepReport {
        index: planned.index,
        summary: planned.summary.clone(),
        outcome,
        points: planned.points.clone(),
        detail,
        elapsed_ms,
    }
}

/// Perform one step's input.
fn perform(
    injector: &mut dyn Injector,
    step: &Step,
    planned: &pixelactions_core::plan::PlannedStep,
    settle: Duration,
) -> Result<()> {
    match step {
        Step::Click { .. } => {
            let point = first_point(planned)?;
            injector.move_to(point.0, point.1)?;
            std::thread::sleep(settle);
            injector.click(Button::Left)?;
        }
        Step::DoubleClick { .. } => {
            let point = first_point(planned)?;
            injector.move_to(point.0, point.1)?;
            std::thread::sleep(settle);
            injector.double_click(Button::Left)?;
        }
        Step::Drag { .. } => {
            let [from, to] = two_points(planned)?;
            injector.move_to(from.0, from.1)?;
            std::thread::sleep(settle);
            injector.press(Button::Left)?;
            // Intermediate motion: apps distinguish a drag from a click by
            // the moves between press and release, so a single jump often
            // registers as neither.
            for step_index in 1..=DRAG_STEPS {
                let progress = f64::from(step_index) / f64::from(DRAG_STEPS);
                injector.move_to(
                    from.0 + (to.0 - from.0) * progress,
                    from.1 + (to.1 - from.1) * progress,
                )?;
            }
            injector.release(Button::Left)?;
        }
        Step::Type { text } => injector.text(text)?,
        Step::Key { chord } => injector.chord(chord)?,
        // Verification-only steps inject nothing; the confirm phase does
        // the work.
        Step::Verify { .. } => {}
    }
    std::thread::sleep(settle);
    Ok(())
}

const DRAG_STEPS: u32 = 12;

/// Ask pixelcoords whether the region a step touched is still there.
fn confirm(
    flow: &Flow,
    step: &Step,
    session: &Path,
    verifier: &mut dyn FnMut(&Path, Option<&str>) -> Result<verify::FindReport>,
) -> Result<StepOutcome> {
    // A `verify` step always checks, whatever the run-wide setting — it
    // exists for no other reason.
    let explicit = matches!(step, Step::Verify { .. });
    if !explicit && flow.settings.verify != Verify::Each {
        return Ok(StepOutcome::Executed);
    }
    let Some(target) = step.targets().first().copied() else {
        // Keyboard steps have no region to check. Say "executed", never
        // "verified" — the distinction is the point.
        return Ok(StepOutcome::Executed);
    };

    let report = verifier(session, Some(target))?;
    if report.is_confirmed(target) {
        return Ok(StepOutcome::Verified);
    }
    let _ = report.all_relocated;
    let detail = report
        .result_for(target)
        .and_then(|r| r.reason.clone())
        .unwrap_or_else(|| {
            format!("region {target:?} did not match its saved crop after the step")
        });
    bail!(detail)
}

fn first_point(planned: &pixelactions_core::plan::PlannedStep) -> Result<(f64, f64)> {
    let Some(point) = planned.points.first() else {
        bail!("step {} resolved to no point", planned.index + 1);
    };
    Ok((point.x, point.y))
}

fn two_points(planned: &pixelactions_core::plan::PlannedStep) -> Result<[(f64, f64); 2]> {
    let [from, to] = planned.points.as_slice() else {
        bail!(
            "drag needs two resolved points, got {}",
            planned.points.len()
        );
    };
    Ok([(from.x, from.y), (to.x, to.y)])
}

#[cfg(test)]
mod tests {
    use pixelactions_core::convert::{ResolvedPoint, Space};
    use pixelactions_core::plan::PlannedStep;

    use super::*;
    use crate::inject::Recording;

    fn point(x: f64, y: f64) -> ResolvedPoint {
        ResolvedPoint {
            x,
            y,
            space: Space::Logical,
            monitor: 0,
            scale: 2.0,
        }
    }

    fn planned(index: usize, step: Step, points: Vec<ResolvedPoint>) -> PlannedStep {
        PlannedStep {
            index,
            summary: step.summary(),
            step,
            points,
        }
    }

    fn flow_with(verify: Verify) -> Flow {
        let mut flow =
            Flow::parse("session = \"s\"\n\n[[step]]\naction = \"type\"\ntext = \"x\"\n")
                .expect("valid");
        flow.settings.verify = verify;
        flow.settings.settle_ms = 0;
        flow
    }

    /// A verifier that confirms everything.
    fn always_found() -> impl FnMut(&Path, Option<&str>) -> Result<verify::FindReport> {
        |_, label| {
            Ok(serde_json::from_str(&format!(
                r#"{{"all_relocated":true,"results":[{{"label":"{}","found":true}}]}}"#,
                label.unwrap_or("x")
            ))
            .expect("fixture"))
        }
    }

    /// A verifier that finds nothing.
    fn never_found() -> impl FnMut(&Path, Option<&str>) -> Result<verify::FindReport> {
        |_, label| {
            Ok(serde_json::from_str(&format!(
                r#"{{"all_relocated":false,"results":[{{"label":"{}","found":false}}]}}"#,
                label.unwrap_or("x")
            ))
            .expect("fixture"))
        }
    }

    #[test]
    fn a_click_moves_then_clicks_in_that_order() {
        let mut injector = Recording::default();
        let plan = Plan {
            steps: vec![planned(
                0,
                Step::Click {
                    target: "submit".into(),
                },
                vec![point(100.0, 200.0)],
            )],
        };
        let report = execute(
            &mut injector,
            &flow_with(Verify::None),
            &plan,
            Path::new("/tmp/session"),
            &mut always_found(),
        );
        assert_eq!(injector.events, vec!["move 100,200", "click"]);
        assert_eq!(report.exit_code(), 0);
    }

    #[test]
    fn a_drag_presses_moves_through_and_releases() {
        let mut injector = Recording::default();
        let plan = Plan {
            steps: vec![planned(
                0,
                Step::Drag {
                    from: "a".into(),
                    to: "b".into(),
                },
                vec![point(0.0, 0.0), point(120.0, 0.0)],
            )],
        };
        execute(
            &mut injector,
            &flow_with(Verify::None),
            &plan,
            Path::new("/tmp/session"),
            &mut always_found(),
        );
        assert_eq!(injector.events.first().expect("moved"), "move 0,0");
        assert_eq!(injector.events.get(1).expect("pressed"), "press");
        assert_eq!(injector.events.last().expect("released"), "release");
        // Intermediate motion is what makes it a drag rather than a click.
        let moves = injector
            .events
            .iter()
            .filter(|e| e.starts_with("move"))
            .count();
        assert!(moves > 2, "drag interpolates: {:?}", injector.events);
    }

    #[test]
    fn a_failed_verification_fails_the_step_and_skips_the_rest() {
        let mut injector = Recording::default();
        let plan = Plan {
            steps: vec![
                planned(
                    0,
                    Step::Click {
                        target: "submit".into(),
                    },
                    vec![point(10.0, 10.0)],
                ),
                planned(
                    1,
                    Step::Click {
                        target: "next".into(),
                    },
                    vec![point(20.0, 20.0)],
                ),
            ],
        };
        let report = execute(
            &mut injector,
            &flow_with(Verify::Each),
            &plan,
            Path::new("/tmp/session"),
            &mut never_found(),
        );
        assert_eq!(report.steps[0].outcome, StepOutcome::Failed);
        assert_eq!(report.steps[1].outcome, StepOutcome::Skipped);
        assert_eq!(report.exit_code(), 1);
        // The second step's input was never injected.
        assert!(!injector.events.iter().any(|e| e == "move 20,20"));
    }

    #[test]
    fn executed_and_verified_are_different_outcomes() {
        let mut injector = Recording::default();
        let plan = Plan {
            steps: vec![planned(
                0,
                Step::Click {
                    target: "submit".into(),
                },
                vec![point(1.0, 1.0)],
            )],
        };
        let unverified = execute(
            &mut injector,
            &flow_with(Verify::None),
            &plan,
            Path::new("/tmp/session"),
            &mut always_found(),
        );
        assert_eq!(unverified.steps[0].outcome, StepOutcome::Executed);

        let verified = execute(
            &mut Recording::default(),
            &flow_with(Verify::Each),
            &plan,
            Path::new("/tmp/session"),
            &mut always_found(),
        );
        assert_eq!(verified.steps[0].outcome, StepOutcome::Verified);
    }

    #[test]
    fn a_verify_step_checks_even_when_the_run_setting_is_none() {
        let plan = Plan {
            steps: vec![planned(
                0,
                Step::Verify {
                    target: "done".into(),
                },
                vec![point(1.0, 1.0)],
            )],
        };
        let report = execute(
            &mut Recording::default(),
            &flow_with(Verify::None),
            &plan,
            Path::new("/tmp/session"),
            &mut never_found(),
        );
        assert_eq!(report.steps[0].outcome, StepOutcome::Failed);
    }

    #[test]
    fn keyboard_steps_report_executed_never_verified() {
        let plan = Plan {
            steps: vec![planned(
                0,
                Step::Type {
                    text: "hello".into(),
                },
                Vec::new(),
            )],
        };
        let report = execute(
            &mut Recording::default(),
            &flow_with(Verify::Each),
            &plan,
            Path::new("/tmp/session"),
            &mut always_found(),
        );
        assert_eq!(report.steps[0].outcome, StepOutcome::Executed);
    }
}
