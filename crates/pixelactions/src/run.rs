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

use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use pixelactions_core::convert::{ResolvedPoint, Space, to_space};
use pixelactions_core::flow::{Flow, Step, Verify};
use pixelactions_core::plan::Plan;
use pixelactions_core::report::{RunReport, StepOutcome, StepReport};
use pixelcoords_core::session::MonitorRecord;

use crate::inject::{Button, Injector};
use crate::verify;

/// How long a whole run may take before it is abandoned. Generous for a
/// human flow, short enough that a wedged run does not own the machine.
pub const WATCHDOG: Duration = Duration::from_secs(120);

/// What relocation learned: where each region is *now*.
pub type Corrections = HashMap<String, ResolvedPoint>;

/// Before acting: re-locate every region the flow will touch, and hand
/// back their current points.
///
/// Two properties, in order of importance. A region that cannot be found
/// unambiguously **stops the run** — clicking coordinates whose regions
/// have moved is vandalism, not automation. A region that moved but is
/// still identifiable yields a corrected point, so the flow heals
/// instead of failing. That second half is the reason a session written
/// last month still works today.
pub fn preflight(
    flow: &Flow,
    session: &Path,
    monitors: &[MonitorRecord],
    space: Space,
    verifier: &mut dyn FnMut(&Path, Option<&str>) -> Result<verify::FindReport>,
) -> Result<Corrections> {
    if !flow.settings.relocate {
        return Ok(Corrections::new());
    }
    let targets = flow.targets();
    if targets.is_empty() {
        return Ok(Corrections::new());
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
        return Ok(corrections(&report, &targets, monitors, space));
    }
    bail!(
        "the screen no longer matches the session: {}. Re-mark with pixelcoords, or set \
         relocate = false in the flow to act on the saved coordinates anyway",
        unconfirmed.join("; ")
    )
}

/// Bounds enforcement, honoring the flow's setting.
fn bounds_gate(
    flow: &Flow,
    planned: &pixelactions_core::plan::PlannedStep,
    corrections: &Corrections,
    monitors: &[MonitorRecord],
) -> Result<()> {
    if !flow.settings.bounds {
        return Ok(());
    }
    check_bounds(planned, corrections, monitors)
}

/// Poll until a region is present (`want_present`) or absent, or the
/// flow's timeout expires.
///
/// This is the honest alternative to a sleep: each poll is a real screen
/// capture through pixelcoords, so waiting costs something and returns
/// the truth rather than a guess about how long an app needs.
fn poll_until(
    flow: &Flow,
    session: &Path,
    target: &str,
    want_present: bool,
    verifier: &mut dyn FnMut(&Path, Option<&str>) -> Result<verify::FindReport>,
) -> Result<StepOutcome> {
    let deadline = Instant::now() + Duration::from_millis(flow.settings.timeout_ms);
    let interval = Duration::from_millis(flow.settings.poll_ms.max(1));
    loop {
        let report = verifier(session, Some(target))?;
        if report.is_confirmed(target) == want_present {
            return Ok(StepOutcome::Verified);
        }
        if Instant::now() >= deadline {
            let wanted = if want_present { "appear" } else { "disappear" };
            bail!(
                "timed out after {}ms waiting for {target:?} to {wanted}",
                flow.settings.timeout_ms
            );
        }
        std::thread::sleep(interval);
    }
}

/// Translate each found region's current position into an actable point.
///
/// A region whose new geometry is missing, or whose corrected point falls
/// outside every described monitor, simply gets no correction — the flow
/// then acts on the saved coordinate, which preflight already confirmed
/// still matches.
pub fn corrections(
    report: &verify::FindReport,
    targets: &[&str],
    monitors: &[MonitorRecord],
    space: Space,
) -> Corrections {
    let mut corrections = Corrections::new();
    for label in targets {
        let Some((monitor_index, local)) = report.corrected_point(label) else {
            continue;
        };
        let Some(monitor) = monitors.iter().find(|m| m.index == monitor_index) else {
            continue;
        };
        let global_x = monitor.origin_px.x + local.x;
        let global_y = monitor.origin_px.y + local.y;
        if let Some(point) = to_space(monitors, global_x, global_y, space) {
            corrections.insert((*label).to_string(), point);
        }
    }
    corrections
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
/// Everything a run needs that isn't the injector or the verifier.
/// Grouped because the alternative is a seven-argument function, and the
/// house rule forbids silencing that lint inline.
pub struct Context<'a> {
    pub flow: &'a Flow,
    pub plan: &'a Plan,
    pub session: &'a Path,
    pub monitors: &'a [MonitorRecord],
    pub corrections: &'a Corrections,
}

pub fn execute(
    injector: &mut dyn Injector,
    context: &Context<'_>,
    verifier: &mut dyn FnMut(&Path, Option<&str>) -> Result<verify::FindReport>,
) -> RunReport {
    let Context {
        flow,
        plan,
        session,
        monitors,
        corrections,
    } = *context;
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
        // Act on where the regions are now, not where they were when the
        // session was captured.
        let points = corrected_points(planned, corrections);
        let outcome = bounds_gate(flow, planned, corrections, monitors)
            .and_then(|()| perform(injector, &planned.step, planned, &points, settle))
            .and_then(|()| confirm(flow, &planned.step, session, verifier));
        let elapsed = step_started.elapsed().as_millis() as u64;

        match outcome {
            Ok(outcome) => steps.push(record_with(planned, outcome, None, elapsed, points)),
            Err(error) => {
                steps.push(record_with(
                    planned,
                    StepOutcome::Failed,
                    Some(format!("{error:#}")),
                    elapsed,
                    points,
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
    let points = planned.points.clone();
    record_with(planned, outcome, detail, elapsed_ms, points)
}

/// The report records the points that were **actually used**, corrections
/// included — so a reader can see where the click went, not where the
/// session said it would.
fn record_with(
    planned: &pixelactions_core::plan::PlannedStep,
    outcome: StepOutcome,
    detail: Option<String>,
    elapsed_ms: u64,
    points: Vec<ResolvedPoint>,
) -> StepReport {
    StepReport {
        index: planned.index,
        summary: planned.summary.clone(),
        outcome,
        points,
        detail,
        elapsed_ms,
    }
}

/// A step's points, with any relocation applied. Targets and points are
/// positionally paired by the planner, so the same index selects both.
fn corrected_points(
    planned: &pixelactions_core::plan::PlannedStep,
    corrections: &Corrections,
) -> Vec<ResolvedPoint> {
    planned
        .step
        .targets()
        .iter()
        .zip(planned.points.iter())
        .map(|(label, planned_point)| corrections.get(*label).copied().unwrap_or(*planned_point))
        .collect()
}

/// Refuse a step whose corrected point has wandered outside the region a
/// human actually marked.
///
/// Relocation moves a point to wherever the crop matched. If that is no
/// longer inside the marked region, the match found something else —
/// and clicking it would be acting on an unknown thing.
fn check_bounds(
    planned: &pixelactions_core::plan::PlannedStep,
    corrections: &Corrections,
    monitors: &[MonitorRecord],
) -> Result<()> {
    for (index, bounds) in planned.bounds.iter().enumerate() {
        let Some(corrected) = corrections.get(&bounds.label) else {
            continue; // not relocated: the planner already proved it fits
        };
        let Some(monitor) = monitors.iter().find(|m| m.index == corrected.monitor) else {
            continue;
        };
        // Corrections carry converted coordinates; bounds are global
        // physical, so undo the scale before comparing.
        let global_x = (corrected.x * scale_for(corrected.space, monitor.scale)).round() as i32;
        let global_y = (corrected.y * scale_for(corrected.space, monitor.scale)).round() as i32;
        if !pixelactions_core::plan::within_bounds(bounds, global_x, global_y) {
            bail!(
                "refusing step {}: {:?} relocated to ({global_x}, {global_y}), outside the \
                 region you marked — the match found something else",
                planned.index + 1,
                bounds.label
            );
        }
        let _ = index;
    }
    Ok(())
}

/// The multiplier that returns a converted coordinate to physical pixels.
fn scale_for(space: Space, monitor_scale: f64) -> f64 {
    match space {
        Space::Logical => monitor_scale,
        _ => 1.0,
    }
}

/// Perform one step's input.
fn perform(
    injector: &mut dyn Injector,
    step: &Step,
    planned: &pixelactions_core::plan::PlannedStep,
    points: &[ResolvedPoint],
    settle: Duration,
) -> Result<()> {
    let points_for_step = points.to_vec();
    match step {
        Step::Click { .. } => {
            let point = first_point(planned, &points_for_step)?;
            injector.move_to(point.0, point.1)?;
            std::thread::sleep(settle);
            injector.click(Button::Left)?;
        }
        Step::DoubleClick { .. } => {
            let point = first_point(planned, &points_for_step)?;
            injector.move_to(point.0, point.1)?;
            std::thread::sleep(settle);
            injector.double_click(Button::Left)?;
        }
        Step::Drag { .. } => {
            let [from, to] = two_points(&points_for_step)?;
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
        Step::Pause { ms } => std::thread::sleep(Duration::from_millis(*ms)),
        // Verification and waiting steps inject nothing; the confirm
        // phase does their work.
        Step::Verify { .. } | Step::WaitFor { .. } | Step::WaitGone { .. } => {}
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
    // Waiting steps poll for a condition rather than checking once.
    if let Step::WaitFor { target } = step {
        return poll_until(flow, session, target, true, verifier);
    }
    if let Step::WaitGone { target } = step {
        return poll_until(flow, session, target, false, verifier);
    }

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

fn first_point(
    planned: &pixelactions_core::plan::PlannedStep,
    points: &[ResolvedPoint],
) -> Result<(f64, f64)> {
    let Some(point) = points.first() else {
        bail!("step {} resolved to no point", planned.index + 1);
    };
    Ok((point.x, point.y))
}

fn two_points(points: &[ResolvedPoint]) -> Result<[(f64, f64); 2]> {
    let [from, to] = points else {
        bail!("drag needs two resolved points, got {}", points.len());
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
            bounds: Vec::new(),
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
            &Context {
                flow: &flow_with(Verify::None),
                plan: &plan,
                session: Path::new("/tmp/session"),
                monitors: &monitors(),
                corrections: &Corrections::new(),
            },
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
            &Context {
                flow: &flow_with(Verify::None),
                plan: &plan,
                session: Path::new("/tmp/session"),
                monitors: &monitors(),
                corrections: &Corrections::new(),
            },
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
            &Context {
                flow: &flow_with(Verify::Each),
                plan: &plan,
                session: Path::new("/tmp/session"),
                monitors: &monitors(),
                corrections: &Corrections::new(),
            },
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
            &Context {
                flow: &flow_with(Verify::None),
                plan: &plan,
                session: Path::new("/tmp/session"),
                monitors: &monitors(),
                corrections: &Corrections::new(),
            },
            &mut always_found(),
        );
        assert_eq!(unverified.steps[0].outcome, StepOutcome::Executed);

        let verified = execute(
            &mut Recording::default(),
            &Context {
                flow: &flow_with(Verify::Each),
                plan: &plan,
                session: Path::new("/tmp/session"),
                monitors: &monitors(),
                corrections: &Corrections::new(),
            },
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
            &Context {
                flow: &flow_with(Verify::None),
                plan: &plan,
                session: Path::new("/tmp/session"),
                monitors: &monitors(),
                corrections: &Corrections::new(),
            },
            &mut never_found(),
        );
        assert_eq!(report.steps[0].outcome, StepOutcome::Failed);
    }

    fn monitors() -> Vec<MonitorRecord> {
        vec![MonitorRecord {
            index: 0,
            name: "retina".into(),
            primary: true,
            origin_px: pixelcoords_core::geometry::Point::new(0, 0),
            size_px: pixelcoords_core::geometry::Size::new(3024, 1964),
            scale: 2.0,
        }]
    }

    /// A verifier reporting that `submit` moved up 120 physical pixels.
    fn moved_up() -> impl FnMut(&Path, Option<&str>) -> Result<verify::FindReport> {
        |_, _| {
            Ok(serde_json::from_str(
                r#"{"all_relocated":true,"results":[
                    {"label":"submit","found":true,"ambiguous":false,"score":0.99,
                     "monitor":0,"new_px":{"x":812,"y":320,"w":96,"h":40},
                     "delta":{"dx":0,"dy":-120}}]}"#,
            )
            .expect("fixture"))
        }
    }

    #[test]
    fn a_moved_region_is_clicked_where_it_is_now_not_where_it_was() {
        // The whole differentiator in one test. The session says the
        // region is at one place; the screen says another; the click must
        // follow the screen.
        let flow =
            Flow::parse("session = \"s\"\n\n[[step]]\naction = \"click\"\ntarget = \"submit\"\n")
                .expect("valid");
        let corrections = preflight(
            &flow,
            Path::new("/tmp/session"),
            &monitors(),
            Space::Logical,
            &mut moved_up(),
        )
        .expect("regions found");

        // New bbox 812,320 96x40 → click point 860,340 physical → /2 on a
        // Retina display → 430,170 logical.
        let corrected = corrections.get("submit").expect("corrected");
        assert!((corrected.x - 430.0).abs() < f64::EPSILON);
        assert!((corrected.y - 170.0).abs() < f64::EPSILON);

        let mut injector = Recording::default();
        let plan = Plan {
            steps: vec![planned(
                0,
                Step::Click {
                    target: "submit".into(),
                },
                // What the session recorded — deliberately stale.
                vec![point(406.0, 230.0)],
            )],
        };
        let report = execute(
            &mut injector,
            &Context {
                flow: &flow_with(Verify::None),
                plan: &plan,
                session: Path::new("/tmp/session"),
                monitors: &monitors(),
                corrections: &corrections,
            },
            &mut moved_up(),
        );
        assert_eq!(injector.events, vec!["move 430,170", "click"]);
        // And the report records where it actually clicked, not the plan.
        assert!((report.steps[0].points[0].x - 430.0).abs() < f64::EPSILON);
    }

    #[test]
    fn preflight_refuses_when_a_target_cannot_be_found() {
        let flow =
            Flow::parse("session = \"s\"\n\n[[step]]\naction = \"click\"\ntarget = \"submit\"\n")
                .expect("valid");
        let error = preflight(
            &flow,
            Path::new("/tmp/session"),
            &monitors(),
            Space::Logical,
            &mut never_found(),
        )
        .expect_err("must refuse");
        let message = format!("{error:#}");
        assert!(message.contains("no longer matches"), "message: {message}");
        assert!(message.contains("submit"), "names the region: {message}");
    }

    #[test]
    fn preflight_is_skipped_entirely_when_relocation_is_off() {
        let mut flow =
            Flow::parse("session = \"s\"\n\n[[step]]\naction = \"click\"\ntarget = \"submit\"\n")
                .expect("valid");
        flow.settings.relocate = false;
        // never_found() would refuse — but relocation is off, so the
        // verifier is never consulted.
        let corrections = preflight(
            &flow,
            Path::new("/tmp/session"),
            &monitors(),
            Space::Logical,
            &mut never_found(),
        )
        .expect("no preflight");
        assert!(corrections.is_empty());
    }

    /// A verifier that reports absent the first N times, then present.
    fn appears_after(
        polls: usize,
    ) -> impl FnMut(&Path, Option<&str>) -> Result<verify::FindReport> {
        let mut seen = 0usize;
        move |_, label| {
            seen += 1;
            let found = seen > polls;
            Ok(serde_json::from_str(&format!(
                r#"{{"all_relocated":{found},"results":[{{"label":"{}","found":{found}}}]}}"#,
                label.unwrap_or("x")
            ))
            .expect("fixture"))
        }
    }

    fn waiting_flow(step: &str, timeout_ms: u64) -> Flow {
        let mut flow = Flow::parse(&format!("session = \"s\"\n\n{step}")).expect("valid");
        flow.settings.settle_ms = 0;
        flow.settings.poll_ms = 1;
        flow.settings.timeout_ms = timeout_ms;
        flow
    }

    #[test]
    fn wait_for_polls_until_the_region_appears() {
        let flow = waiting_flow(
            "[[step]]\naction = \"wait_for\"\ntarget = \"dialog\"\n",
            5_000,
        );
        let plan = Plan {
            steps: vec![planned(
                0,
                Step::WaitFor {
                    target: "dialog".into(),
                },
                vec![point(1.0, 1.0)],
            )],
        };
        let report = execute(
            &mut Recording::default(),
            &Context {
                flow: &flow,
                plan: &plan,
                session: Path::new("/tmp/session"),
                monitors: &monitors(),
                corrections: &Corrections::new(),
            },
            &mut appears_after(3),
        );
        assert_eq!(report.steps[0].outcome, StepOutcome::Verified);
    }

    #[test]
    fn wait_for_times_out_honestly_rather_than_hanging() {
        let flow = waiting_flow("[[step]]\naction = \"wait_for\"\ntarget = \"dialog\"\n", 5);
        let plan = Plan {
            steps: vec![planned(
                0,
                Step::WaitFor {
                    target: "dialog".into(),
                },
                vec![point(1.0, 1.0)],
            )],
        };
        let report = execute(
            &mut Recording::default(),
            &Context {
                flow: &flow,
                plan: &plan,
                session: Path::new("/tmp/session"),
                monitors: &monitors(),
                corrections: &Corrections::new(),
            },
            &mut never_found(),
        );
        assert_eq!(report.steps[0].outcome, StepOutcome::Failed);
        let detail = report.steps[0].detail.clone().expect("explained");
        assert!(detail.contains("timed out"), "detail: {detail}");
        assert!(
            detail.contains("appear"),
            "says what it waited for: {detail}"
        );
    }

    #[test]
    fn wait_gone_succeeds_when_the_region_is_absent() {
        let flow = waiting_flow(
            "[[step]]\naction = \"wait_gone\"\ntarget = \"spinner\"\n",
            5_000,
        );
        let plan = Plan {
            steps: vec![planned(
                0,
                Step::WaitGone {
                    target: "spinner".into(),
                },
                vec![point(1.0, 1.0)],
            )],
        };
        let report = execute(
            &mut Recording::default(),
            &Context {
                flow: &flow,
                plan: &plan,
                session: Path::new("/tmp/session"),
                monitors: &monitors(),
                corrections: &Corrections::new(),
            },
            &mut never_found(),
        );
        assert_eq!(report.steps[0].outcome, StepOutcome::Verified);
    }

    #[test]
    fn a_correction_outside_its_own_region_is_refused() {
        // The guardrail: relocation found a match, but it landed outside
        // the region a human marked — so it matched something else.
        use pixelcoords_core::geometry::{Rect, Shape};
        let flow = flow_with(Verify::None);
        let mut plan = Plan {
            steps: vec![planned(
                0,
                Step::Click {
                    target: "submit".into(),
                },
                vec![point(10.0, 10.0)],
            )],
        };
        plan.steps[0].bounds = vec![pixelactions_core::plan::Bounds {
            label: "submit".into(),
            shape: Shape::Rect(Rect::new(800, 400, 100, 80)),
            monitor: 0,
        }];
        let mut corrections = Corrections::new();
        // Logical (10, 10) on a 2x monitor is global physical (20, 20) —
        // nowhere near the marked rect.
        corrections.insert("submit".into(), point(10.0, 10.0));

        let mut injector = Recording::default();
        let report = execute(
            &mut injector,
            &Context {
                flow: &flow,
                plan: &plan,
                session: Path::new("/tmp/session"),
                monitors: &monitors(),
                corrections: &corrections,
            },
            &mut always_found(),
        );
        assert_eq!(report.steps[0].outcome, StepOutcome::Failed);
        assert!(injector.events.is_empty(), "nothing was injected");
        let detail = report.steps[0].detail.clone().expect("explained");
        assert!(detail.contains("outside the region"), "detail: {detail}");
    }

    #[test]
    fn a_pause_step_injects_nothing_and_succeeds() {
        let mut flow = flow_with(Verify::Each);
        flow.settings.settle_ms = 0;
        let plan = Plan {
            steps: vec![planned(0, Step::Pause { ms: 1 }, Vec::new())],
        };
        let mut injector = Recording::default();
        let report = execute(
            &mut injector,
            &Context {
                flow: &flow,
                plan: &plan,
                session: Path::new("/tmp/session"),
                monitors: &monitors(),
                corrections: &Corrections::new(),
            },
            &mut never_found(),
        );
        assert_eq!(report.steps[0].outcome, StepOutcome::Executed);
        assert!(injector.events.is_empty());
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
            &Context {
                flow: &flow_with(Verify::Each),
                plan: &plan,
                session: Path::new("/tmp/session"),
                monitors: &monitors(),
                corrections: &Corrections::new(),
            },
            &mut always_found(),
        );
        assert_eq!(report.steps[0].outcome, StepOutcome::Executed);
    }
}
