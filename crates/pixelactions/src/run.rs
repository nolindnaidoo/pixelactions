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
use pixelactions_core::convert::{
    ResolvedPoint, Space, native_space, near_screen_corner, to_space,
};
use pixelactions_core::flow::{Flow, Step, Verify};
use pixelactions_core::plan::Plan;
use pixelactions_core::report::{RunReport, StepOutcome, StepReport};
use pixelcoords_core::session::MonitorRecord;

use pixelactions_core::audit::Event as AuditEvent;

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
    found: &dyn Fn(&str),
) -> Result<Corrections> {
    if !flow.settings.relocate {
        return Ok(Corrections::new());
    }
    // Only the regions this run will *act on*. A `wait_for` is waiting for
    // something that is not there yet, and a `wait_gone` succeeds precisely
    // when its region is absent — demanding either up front made both verbs
    // impossible to use from the command line. Matching their templates
    // anyway is pure cost: a whole-session sweep of three regions measured
    // 5.2s against 1.5s per region.
    let targets = flow.acting_targets();
    if targets.is_empty() {
        return Ok(Corrections::new());
    }

    match locate_each(&targets, session, monitors, space, verifier, found)? {
        Ok(corrections) => Ok(corrections),
        Err(unconfirmed) => bail!(
            "the screen no longer matches the session: {}. Re-mark with pixelcoords, or set \
             relocate = false in the flow to act on the saved coordinates anyway",
            unconfirmed.join("; ")
        ),
    }
}

/// Confirm each label on its own, and report where the ones that moved are
/// now.
///
/// One template at a time on purpose. Matching is the expensive half of a
/// find, and a caller that needs two regions should not pay to match every
/// other region in the session. The outer `Result` is a broken verifier;
/// the inner one is the answer — corrections, or the labels that could not
/// be confirmed, described.
fn locate_each(
    targets: &[&str],
    session: &Path,
    monitors: &[MonitorRecord],
    space: Space,
    verifier: &mut dyn FnMut(&Path, Option<&str>) -> Result<verify::FindReport>,
    found: &dyn Fn(&str),
) -> Result<std::result::Result<Corrections, Vec<String>>> {
    let mut located = Corrections::new();
    let mut unconfirmed = Vec::new();
    for label in targets {
        let report = verifier(session, Some(label))?;
        if !report.is_confirmed(label) {
            let reason = report
                .result_for(label)
                .map_or_else(|| "not in the report".to_string(), describe);
            unconfirmed.push(format!("{label} ({reason})"));
            continue;
        }
        located.extend(corrections(&report, &[label], monitors, space));
        found(label);
    }
    if unconfirmed.is_empty() {
        return Ok(Ok(located));
    }
    Ok(Err(unconfirmed))
}

/// Confirm, immediately before a step acts, that the regions it is about
/// to touch are still there — and refresh their coordinates.
///
/// This is a **precondition**, and that is the point. "Is the thing I am
/// about to click present and unambiguous?" has a stable answer. "Did the
/// thing survive being clicked?" does not: focusing a field adds a caret
/// and a border, so checking a region *after* acting on it reports failure
/// precisely when the action worked. That check was never an outcome
/// check, and pretending otherwise produced false failures on success —
/// and, worse, false successes when a click was swallowed by window
/// activation and the region sat untouched.
///
/// Checking here also keeps coordinates honest mid-run. Relocation used to
/// happen once, before the first step, so any step that reflowed the page
/// left every later step acting on stale positions.
///
/// To assert an *outcome*, name the thing that should have changed:
/// `wait_for` what appears, `wait_gone` what disappears, `verify` another
/// region.
fn precheck(
    flow: &Flow,
    planned: &pixelactions_core::plan::PlannedStep,
    session: &Path,
    monitors: &[MonitorRecord],
    known: &mut Corrections,
    verifier: &mut dyn FnMut(&Path, Option<&str>) -> Result<verify::FindReport>,
) -> Result<()> {
    if flow.settings.verify != Verify::Each || !planned.step.injects() {
        return Ok(());
    }
    let targets = planned.step.targets();
    if targets.is_empty() {
        return Ok(());
    }

    match locate_each(
        &targets,
        session,
        monitors,
        flow.settings.space,
        verifier,
        &|_| {},
    )? {
        Err(unconfirmed) => bail!(
            "cannot act on {}. Nothing was injected for this step",
            unconfirmed.join("; ")
        ),
        Ok(fresh) => {
            // Replace what is known about *these* labels only. A region can
            // also move back to where it started, so a label absent from
            // `fresh` must lose its stale correction rather than keep it.
            for label in &targets {
                match fresh.get(*label) {
                    Some(point) => known.insert((*label).to_string(), *point),
                    None => known.remove(*label),
                };
            }
            Ok(())
        }
    }
}

/// `Auto` resolved to what this platform's input API actually speaks —
/// the space the cursor is read in, and the space corners are compared in.
fn resolve_space(space: Space) -> Space {
    match space {
        Space::Auto => native_space(),
        other => other,
    }
}

/// The kill switch, checked before every step.
///
/// A person who wants a run to stop grabs the mouse — that reflex is the
/// only interface that works while the automation holds the keyboard and
/// the terminal is not focused. Slamming the cursor into a screen corner
/// aborts before the next step is performed.
///
/// A cursor that cannot be read fails the step rather than being waved
/// through: a safety check that silently stops evaluating is worse than
/// one that was never claimed. Turning `failsafe` off is the supported
/// way to opt out.
fn failsafe_gate(
    flow: &Flow,
    injector: &mut dyn Injector,
    monitors: &[MonitorRecord],
) -> Result<()> {
    if !flow.settings.failsafe {
        return Ok(());
    }
    let (x, y) = injector.cursor().map_err(|error| {
        anyhow::anyhow!("failsafe is on but the cursor position could not be read: {error}")
    })?;
    if !near_screen_corner(
        monitors,
        resolve_space(flow.settings.space),
        x,
        y,
        flow.settings.failsafe_margin,
    ) {
        return Ok(());
    }
    bail!(
        "kill switch: the cursor is in a screen corner ({x:.0}, {y:.0}), so the run stopped \
         before this step. Move it away and run again, or set failsafe = false"
    )
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
    waiter: &Waiter<'_>,
    verifier: &mut dyn FnMut(&Path, Option<&str>) -> Result<verify::FindReport>,
) -> Result<StepOutcome> {
    let report = waiter(
        session,
        target,
        want_present,
        Duration::from_millis(flow.settings.timeout_ms),
        Duration::from_millis(flow.settings.poll_ms.max(1)),
    )?;
    if report.ok {
        return Ok(StepOutcome::Verified);
    }

    // Out of budget. A timeout without evidence is the complaint that
    // fills pyautogui's issue tracker: "not found" tells you nothing about
    // whether you were one pixel off or looking at the wrong screen.
    //
    // So spend one full-frame search — the expensive kind, once, when the
    // answer is already bad — to say *which* of those it was. `wait` scores
    // each region where the session left it, so it cannot see a region
    // that moved; `find` can, and "it is there, it moved by (dx, dy)" is
    // the difference between a user guessing and a user fixing.
    let wanted = if want_present { "appear" } else { "disappear" };
    let last_look = drift_note(session, target, verifier);
    bail!(
        "timed out after {}ms waiting for {target:?} to {wanted} \
         ({} polls, best match score {:.3}) — {last_look}",
        report.elapsed_ms,
        report.polls,
        report.best_score(),
    );
}

/// One `find` after a timeout, purely to describe what was there.
///
/// A broken verifier must not replace the timeout with its own error: the
/// timeout is the real answer and the caller needs it, so a failure to
/// elaborate degrades to saying nothing extra.
fn drift_note(
    session: &Path,
    target: &str,
    verifier: &mut dyn FnMut(&Path, Option<&str>) -> Result<verify::FindReport>,
) -> String {
    let Ok(report) = verifier(session, Some(target)) else {
        return "a follow-up look failed too".to_string();
    };
    let Some(result) = report.result_for(target) else {
        return "the report never mentioned it".to_string();
    };
    match result.delta {
        Some(delta) if result.found && !result.ambiguous && (delta.dx != 0 || delta.dy != 0) => {
            // `describe` already names the offset, so do not repeat it —
            // say what it *means*, which is the part a reader is missing.
            format!(
                "last look: it is on screen, {} physical px from where it was marked, \
                 so `wait` was scoring the old position",
                format_args!("({}, {})", delta.dx, delta.dy),
            )
        }
        _ => format!("last look: {}", describe(result)),
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
        // A region that did not move needs no correction. Recording one
        // anyway is harmless arithmetic but makes the run *report* that
        // it relocated things when nothing changed, which is exactly the
        // kind of small dishonesty this tool exists to avoid.
        let moved = report
            .result_for(label)
            .and_then(|result| result.delta)
            .is_some_and(|delta| delta.dx != 0 || delta.dy != 0);
        if !moved {
            continue;
        }
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
        let Some(reason) = result.reason.clone() else {
            return format!("no match above the floor, best score {:.3}", result.score);
        };
        return format!("{reason} (score {:.3})", result.score);
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
    /// Whether a relocation pass has *just* confirmed these regions with
    /// nothing injected since — i.e. whether the first step's own check
    /// would be looking at the same screen for the second time.
    ///
    /// The caller knows this and the settings do not: a flow run preflights
    /// once and then steps through, while the protocol server calls this
    /// per request with no preflight at all.
    pub checked: bool,
    /// Called as each step finishes, before the next one starts.
    ///
    /// A run takes seconds — each check is a real screen capture — and a
    /// tool that prints nothing until it is done looks hung. The report is
    /// still returned whole; this is how a caller shows it arriving.
    pub progress: &'a dyn Fn(&StepReport),
    /// Blocks until a region matches again, or stops. The second seam
    /// beside the verifier, and it lives here rather than as another
    /// argument because `execute` is already at its argument limit.
    ///
    /// `Fn` rather than `FnMut`: a wait is now a single call that blocks,
    /// so nothing on this side counts iterations any more —
    /// `pixelcoords wait` owns the loop.
    pub waiter: &'a Waiter<'a>,
    /// Compares a region against the screen now — the RGB comparison the
    /// correlation-based seams above cannot make.
    pub differ: &'a Differ<'a>,
    /// Receives every event this run produces, as it happens.
    ///
    /// Incremental on purpose: writing at the end would lose exactly the
    /// runs worth having a record of — the one the watchdog stopped, the
    /// one someone killed.
    pub auditor: &'a Auditor<'a>,
}

/// Receives run events. See `pixelactions_core::audit`.
pub type Auditor<'a> = dyn Fn(&AuditEvent) + 'a;

/// An auditor that records nothing, for callers that do not want a log.
pub fn no_audit() -> &'static Auditor<'static> {
    &|_: &AuditEvent| {}
}

/// Compares `label` against the screen, allowing `tolerance` percent of
/// its masked pixels to differ before it counts as changed.
pub type Differ<'a> = dyn Fn(&Path, &str, f64) -> Result<verify::DiffReport> + 'a;

/// The differ every real caller wants: `pixelcoords diff` itself.
pub fn real_differ() -> &'static Differ<'static> {
    &|session, label, tolerance| verify::diff(session, label, tolerance)
}

/// Blocks until `label` matches its saved crop again (`true`) or stops
/// matching (`false`), within a timeout, polling at an interval.
pub type Waiter<'a> =
    dyn Fn(&Path, &str, bool, Duration, Duration) -> Result<verify::WaitReport> + 'a;

/// The waiter every real caller wants: `pixelcoords wait` itself.
pub fn real_waiter() -> &'static Waiter<'static> {
    &|session, label, want_present, timeout, interval| {
        verify::wait(session, label, want_present, timeout, interval)
    }
}

/// A `progress` that reports nowhere, for callers that only want the
/// finished report.
pub fn silent() -> &'static dyn Fn(&StepReport) {
    &|_: &StepReport| {}
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
        checked,
        progress,
        waiter,
        differ,
        auditor,
    } = *context;
    auditor(&AuditEvent::run(
        crate::audit::now(),
        session.display().to_string(),
        true,
    ));
    // One place every finished step passes through, so a step added later
    // cannot quietly skip the log the way it could if each site called
    // `progress` and `push` on its own.
    let emit = |report: StepReport| -> StepReport {
        progress(&report);
        auditor(&AuditEvent::step(crate::audit::now(), &report));
        report
    };
    let started = Instant::now();
    let settle = Duration::from_millis(flow.settings.settle_ms);
    let mut steps: Vec<StepReport> = Vec::with_capacity(plan.steps.len());
    let mut failed = false;
    // Owned, because the pre-step check refreshes it: a step that reflows
    // the page moves every other region, and later steps must not act on
    // coordinates measured before that happened.
    let mut corrections = corrections.clone();
    // Any step at all makes the picture stale again.
    let mut fresh = checked;

    for planned in &plan.steps {
        if failed {
            let skipped = record(planned, StepOutcome::Skipped, None, 0);
            steps.push(emit(skipped));
            continue;
        }
        if started.elapsed() > WATCHDOG {
            let timed_out = record(
                planned,
                StepOutcome::Failed,
                Some("watchdog: the run exceeded its time budget".into()),
                0,
            );
            steps.push(emit(timed_out));
            failed = true;
            continue;
        }

        let step_started = Instant::now();

        // Guards run first and are reported apart from failures: nothing
        // was attempted, and the run earns exit 3 rather than 1.
        let gate = failsafe_gate(flow, injector, monitors).and_then(|()| {
            if fresh {
                return Ok(());
            }
            precheck(flow, planned, session, monitors, &mut corrections, verifier)
        });
        if let Err(refusal) = gate {
            let refused = record_with(
                planned,
                StepOutcome::Refused,
                Some(format!("{refusal:#}")),
                step_started.elapsed().as_millis() as u64,
                corrected_points(planned, &corrections),
            );
            steps.push(emit(refused));
            failed = true;
            continue;
        }

        // Act on where the regions are *now* — the check above may have
        // moved them.
        fresh = false;
        let points = corrected_points(planned, &corrections);
        let outcome = perform(injector, &planned.step, planned, &points, settle)
            .and_then(|()| confirm(flow, &planned.step, session, waiter, differ, verifier));
        let elapsed = step_started.elapsed().as_millis() as u64;

        let done = match outcome {
            Ok(outcome) => record_with(planned, outcome, None, elapsed, points),
            Err(error) => {
                failed = true;
                record_with(
                    planned,
                    StepOutcome::Failed,
                    Some(format!("{error:#}")),
                    elapsed,
                    points,
                )
            }
        };
        steps.push(emit(done));
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
        Step::Scroll { amount, axis, .. } => {
            // Hover first: a wheel event goes to whatever is under the
            // cursor, so the label picks the container to scroll exactly
            // the way it picks the thing to click.
            let point = first_point(planned, &points_for_step)?;
            injector.move_to(point.0, point.1)?;
            std::thread::sleep(settle);
            injector.scroll(*amount, *axis)?;
        }
        Step::Type { text } => injector.text(text)?,
        Step::Key { chord } => injector.chord(chord)?,
        Step::Pause { ms } => std::thread::sleep(Duration::from_millis(*ms)),
        // Verification and waiting steps inject nothing; the confirm
        // phase does their work.
        Step::Verify { .. }
        | Step::WaitFor { .. }
        | Step::WaitGone { .. }
        | Step::Changed { .. } => {}
    }
    std::thread::sleep(settle);
    Ok(())
}

const DRAG_STEPS: u32 = 12;

/// What a step asserts once it has run.
///
/// Only observation steps assert anything. An acting step reports
/// `executed` — the OS accepted the input — because the region it touched
/// cannot confirm its own outcome; see [`precheck`]. Outcomes are asserted
/// by naming what should have changed.
fn confirm(
    flow: &Flow,
    step: &Step,
    session: &Path,
    waiter: &Waiter<'_>,
    differ: &Differ<'_>,
    verifier: &mut dyn FnMut(&Path, Option<&str>) -> Result<verify::FindReport>,
) -> Result<StepOutcome> {
    match step {
        Step::Changed { target, tolerance } => check_changed(session, target, *tolerance, differ),
        Step::WaitFor { target } => poll_until(flow, session, target, true, waiter, verifier),
        Step::WaitGone { target } => poll_until(flow, session, target, false, waiter, verifier),
        Step::Verify { target } => check_once(session, target, verifier),
        _ => Ok(StepOutcome::Executed),
    }
}

/// A `changed` step: prove the region is no longer what it was.
///
/// `pixelcoords diff` answers the opposite question — `ok` means every
/// region stayed *within* tolerance — so this succeeds precisely when that
/// is false. A step that finds nothing changed fails the run, the same way
/// a `verify` that finds the region gone does: both are assertions about
/// the screen, and an assertion that quietly passes is worse than none.
fn check_changed(
    session: &Path,
    target: &str,
    tolerance: f64,
    differ: &Differ<'_>,
) -> Result<StepOutcome> {
    let report = differ(session, target, tolerance)?;
    if !report.ok {
        return Ok(StepOutcome::Verified);
    }
    let detail = report.first().map_or_else(
        || format!("region {target:?} was not in the report"),
        |r| {
            format!(
                "{} of {} pixels differ ({:.3}%)",
                r.changed_px, r.masked_px, r.changed_pct
            )
        },
    );
    bail!(
        "region {target:?} did not change{} — {detail}",
        if tolerance > 0.0 {
            format!(" by more than {tolerance}%")
        } else {
            String::new()
        }
    )
}

/// A `verify` step: look once, and say what was seen when it is not there.
fn check_once(
    session: &Path,
    target: &str,
    verifier: &mut dyn FnMut(&Path, Option<&str>) -> Result<verify::FindReport>,
) -> Result<StepOutcome> {
    let report = verifier(session, Some(target))?;
    if report.is_confirmed(target) {
        return Ok(StepOutcome::Verified);
    }
    let detail = report.result_for(target).map_or_else(
        || format!("region {target:?} was not in the report"),
        |result| format!("region {target:?}: {}", describe(result)),
    );
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

    /// Every label a test might name. The pre-step check sweeps the whole
    /// session (`label: None`), so a fake has to answer for all of them,
    /// not just the one being asked about.
    const TEST_LABELS: &[&str] = &[
        "submit", "next", "email", "results", "a", "b", "x", "dialog",
    ];

    fn report_of(label: Option<&str>, found: bool) -> verify::FindReport {
        let labels: Vec<&str> = label.map_or_else(|| TEST_LABELS.to_vec(), |one| vec![one]);
        let results: Vec<String> = labels
            .iter()
            .map(|l| format!(r#"{{"label":"{l}","found":{found}}}"#))
            .collect();
        serde_json::from_str(&format!(
            r#"{{"all_relocated":{found},"results":[{}]}}"#,
            results.join(",")
        ))
        .expect("fixture")
    }

    /// A verifier that confirms everything.
    fn always_found() -> impl FnMut(&Path, Option<&str>) -> Result<verify::FindReport> {
        |_, label| Ok(report_of(label, true))
    }

    /// A verifier that finds nothing.
    fn never_found() -> impl FnMut(&Path, Option<&str>) -> Result<verify::FindReport> {
        |_, label| Ok(report_of(label, false))
    }

    /// A waiter whose condition holds immediately — the default for every
    /// test that is not about waiting.
    fn waits_ok() -> &'static Waiter<'static> {
        &|_, _, _, _, _| {
            Ok(verify::WaitReport {
                ok: true,
                polls: 1,
                elapsed_ms: 0,
                results: vec![verify::WaitResult { score: 1.0 }],
            })
        }
    }

    /// A differ reporting the region is unchanged — `pixelcoords diff`
    /// answers "did it stay the same", so `ok: true` is *no change*.
    fn sees_no_change() -> &'static Differ<'static> {
        &|_, _, _| {
            Ok(verify::DiffReport {
                ok: true,
                results: vec![verify::DiffResult {
                    changed_px: 0,
                    masked_px: 7503,
                    changed_pct: 0.0,
                }],
            })
        }
    }

    /// A differ reporting the region changed past its tolerance.
    fn sees_change() -> &'static Differ<'static> {
        &|_, _, _| {
            Ok(verify::DiffReport {
                ok: false,
                results: vec![verify::DiffResult {
                    changed_px: 1156,
                    masked_px: 7503,
                    changed_pct: 15.407,
                }],
            })
        }
    }

    /// A waiter that never sees its condition hold: `pixelcoords wait`
    /// spent its whole budget and reported `ok: false`.
    ///
    /// `polls` is derived from the budget the same way `wait` derives it,
    /// so the number in a timeout message is the one a real run would
    /// carry rather than a constant that happens to look plausible.
    fn waits_out(score: f64) -> &'static Waiter<'static> {
        Box::leak(Box::new(
            move |_: &Path, _: &str, _: bool, timeout: Duration, interval: Duration| {
                let polls = u32::try_from(timeout.as_millis() / interval.as_millis().max(1))
                    .unwrap_or(u32::MAX);
                Ok(verify::WaitReport {
                    ok: false,
                    polls,
                    elapsed_ms: u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX),
                    results: vec![verify::WaitResult { score }],
                })
            },
        ))
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
                checked: false,
                progress: silent(),
                waiter: waits_ok(),
                differ: sees_no_change(),
                auditor: no_audit(),
            },
            &mut always_found(),
        );
        assert_eq!(injector.events, vec!["move 100,200", "click"]);
        assert_eq!(report.exit_code(), 0);
    }

    #[test]
    fn a_cursor_in_a_corner_stops_the_run_before_the_step() {
        // The single test monitor is 3024x1964 at scale 2, so its
        // bottom-right corner is (1511.5, 981.5) in logical points —
        // where a person slamming the mouse would leave it.
        let mut injector = Recording {
            cursor: (1511.0, 981.0),
            ..Default::default()
        };
        let plan = Plan {
            steps: vec![planned(
                0,
                Step::Click {
                    target: "submit".into(),
                },
                vec![point(100.0, 200.0)],
            )],
        };
        let mut flow = flow_with(Verify::None);
        flow.settings.space = Space::Logical;
        let report = execute(
            &mut injector,
            &Context {
                flow: &flow,
                plan: &plan,
                session: Path::new("/tmp/session"),
                monitors: &monitors(),
                corrections: &Corrections::new(),
                checked: false,
                progress: silent(),
                waiter: waits_ok(),
                differ: sees_no_change(),
                auditor: no_audit(),
            },
            &mut always_found(),
        );
        assert_eq!(
            injector.events,
            Vec::<String>::new(),
            "nothing may be injected once the kill switch trips"
        );
        assert_eq!(
            report.exit_code(),
            3,
            "a person stopping the run is a refusal, not a failure"
        );
        assert_eq!(report.steps[0].outcome, StepOutcome::Refused);
        let detail = report.steps[0].detail.as_deref().unwrap_or_default();
        assert!(detail.contains("kill switch"), "says why: {detail}");
    }

    #[test]
    fn the_kill_switch_can_be_turned_off_deliberately() {
        let mut injector = Recording {
            cursor: (1511.0, 981.0),
            ..Default::default()
        };
        let plan = Plan {
            steps: vec![planned(
                0,
                Step::Click {
                    target: "submit".into(),
                },
                vec![point(100.0, 200.0)],
            )],
        };
        let mut flow = flow_with(Verify::None);
        flow.settings.space = Space::Logical;
        flow.settings.failsafe = false;
        let report = execute(
            &mut injector,
            &Context {
                flow: &flow,
                plan: &plan,
                session: Path::new("/tmp/session"),
                monitors: &monitors(),
                corrections: &Corrections::new(),
                checked: false,
                progress: silent(),
                waiter: waits_ok(),
                differ: sees_no_change(),
                auditor: no_audit(),
            },
            &mut always_found(),
        );
        assert_eq!(injector.events, vec!["move 100,200", "click"]);
        assert_eq!(report.exit_code(), 0);
    }

    #[test]
    fn an_unreadable_cursor_refuses_the_step_rather_than_waving_it_through() {
        /// An injector whose cursor cannot be read — the shape a missing
        /// permission takes.
        struct Blind;
        impl Injector for Blind {
            fn move_to(&mut self, _x: f64, _y: f64) -> Result<()> {
                Ok(())
            }
            fn click(&mut self, _button: Button) -> Result<()> {
                Ok(())
            }
            fn double_click(&mut self, _button: Button) -> Result<()> {
                Ok(())
            }
            fn press(&mut self, _button: Button) -> Result<()> {
                Ok(())
            }
            fn release(&mut self, _button: Button) -> Result<()> {
                Ok(())
            }
            fn text(&mut self, _text: &str) -> Result<()> {
                Ok(())
            }
            fn chord(&mut self, _chord: &str) -> Result<()> {
                Ok(())
            }
            fn scroll(&mut self, _amount: i32, _axis: pixelactions_core::flow::Axis) -> Result<()> {
                Ok(())
            }
            fn cursor(&mut self) -> Result<(f64, f64)> {
                bail!("no cursor here")
            }
            fn probe(&mut self) -> Result<()> {
                Ok(())
            }
        }

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
            &mut Blind,
            &Context {
                flow: &flow_with(Verify::None),
                plan: &plan,
                session: Path::new("/tmp/session"),
                monitors: &monitors(),
                corrections: &Corrections::new(),
                checked: false,
                progress: silent(),
                waiter: waits_ok(),
                differ: sees_no_change(),
                auditor: no_audit(),
            },
            &mut always_found(),
        );
        assert_eq!(report.exit_code(), 3);
        let detail = report.steps[0].detail.as_deref().unwrap_or_default();
        assert!(detail.contains("failsafe"), "names the setting: {detail}");
    }

    #[test]
    fn looking_at_a_moved_region_is_never_refused_on_bounds() {
        // The bounds guard stops the tool *acting* on a possible
        // mis-match. A wait_for injects nothing, and refusing it would
        // take away the one primitive that finds out where a region went
        // — which is exactly what you reach for after a scroll.
        let mut corrections = Corrections::new();
        corrections.insert("submit".to_string(), point(9_000.0, 9_000.0));
        let plan = Plan {
            steps: vec![planned(
                0,
                Step::WaitFor {
                    target: "submit".into(),
                },
                vec![point(9_000.0, 9_000.0)],
            )],
        };
        let report = execute(
            &mut Recording::default(),
            &Context {
                flow: &flow_with(Verify::Each),
                plan: &plan,
                session: Path::new("/tmp/session"),
                monitors: &monitors(),
                corrections: &corrections,
                checked: false,
                progress: silent(),
                waiter: waits_ok(),
                differ: sees_no_change(),
                auditor: no_audit(),
            },
            &mut always_found(),
        );
        assert_eq!(report.steps[0].outcome, StepOutcome::Verified);
    }

    #[test]
    fn a_scroll_hovers_the_region_first_then_turns_the_wheel() {
        let mut injector = Recording::default();
        let plan = Plan {
            steps: vec![planned(
                0,
                Step::Scroll {
                    target: "results".into(),
                    amount: -3,
                    axis: pixelactions_core::flow::Axis::Vertical,
                },
                vec![point(100.0, 200.0)],
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
                checked: false,
                progress: silent(),
                waiter: waits_ok(),
                differ: sees_no_change(),
                auditor: no_audit(),
            },
            &mut always_found(),
        );
        // Order matters: the wheel event goes wherever the cursor is, so
        // hovering has to happen before the scroll, not after.
        assert_eq!(injector.events, vec!["move 100,200", "scroll v-3"]);
    }

    #[test]
    fn a_scroll_is_never_checked_against_its_own_region_afterwards() {
        // Scrolling moves its own region on purpose. A verifier that
        // answers only the pre-step check proves nothing looks again after
        // the wheel turns.
        let mut looks = 0;
        let mut once = move |_: &Path, label: Option<&str>| {
            looks += 1;
            Ok(report_of(label, looks == 1))
        };
        let plan = Plan {
            steps: vec![planned(
                0,
                Step::Scroll {
                    target: "results".into(),
                    amount: 3,
                    axis: pixelactions_core::flow::Axis::Vertical,
                },
                vec![point(100.0, 200.0)],
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
                checked: false,
                progress: silent(),
                waiter: waits_ok(),
                differ: sees_no_change(),
                auditor: no_audit(),
            },
            &mut once,
        );
        assert_eq!(report.steps[0].outcome, StepOutcome::Executed);
        assert_eq!(report.exit_code(), 0);
    }

    #[test]
    fn a_scroll_still_obeys_the_kill_switch() {
        let mut injector = Recording {
            cursor: (1511.0, 981.0),
            ..Default::default()
        };
        let plan = Plan {
            steps: vec![planned(
                0,
                Step::Scroll {
                    target: "results".into(),
                    amount: 3,
                    axis: pixelactions_core::flow::Axis::Vertical,
                },
                vec![point(100.0, 200.0)],
            )],
        };
        let mut flow = flow_with(Verify::None);
        flow.settings.space = Space::Logical;
        let report = execute(
            &mut injector,
            &Context {
                flow: &flow,
                plan: &plan,
                session: Path::new("/tmp/session"),
                monitors: &monitors(),
                corrections: &Corrections::new(),
                checked: false,
                progress: silent(),
                waiter: waits_ok(),
                differ: sees_no_change(),
                auditor: no_audit(),
            },
            &mut always_found(),
        );
        assert_eq!(report.steps[0].outcome, StepOutcome::Refused);
        assert!(injector.events.is_empty(), "nothing was injected");
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
                checked: false,
                progress: silent(),
                waiter: waits_ok(),
                differ: sees_no_change(),
                auditor: no_audit(),
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
    fn a_region_that_cannot_be_found_refuses_before_injecting_anything() {
        // Under the old model this clicked first and failed the check
        // afterwards, so the input had already landed somewhere. The check
        // is a precondition now: if the region cannot be confirmed, nothing
        // is injected at all, and the run earns a refusal rather than a
        // failure.
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
                checked: false,
                progress: silent(),
                waiter: waits_ok(),
                differ: sees_no_change(),
                auditor: no_audit(),
            },
            &mut never_found(),
        );
        assert_eq!(report.steps[0].outcome, StepOutcome::Refused);
        assert_eq!(report.steps[1].outcome, StepOutcome::Skipped);
        assert_eq!(report.exit_code(), 3);
        assert!(
            injector.events.is_empty(),
            "nothing may be injected when the region could not be confirmed"
        );
    }

    #[test]
    fn an_acting_step_reports_executed_never_verified() {
        // A click cannot confirm its own outcome: acting on a region
        // changes it, so "the region still matches" means the click did
        // nothing. Acting steps report what is true — the input was posted
        // — and outcomes are asserted by naming what should have changed.
        let plan = Plan {
            steps: vec![planned(
                0,
                Step::Click {
                    target: "submit".into(),
                },
                vec![point(1.0, 1.0)],
            )],
        };
        for verify in [Verify::None, Verify::Each] {
            let report = execute(
                &mut Recording::default(),
                &Context {
                    flow: &flow_with(verify),
                    plan: &plan,
                    session: Path::new("/tmp/session"),
                    monitors: &monitors(),
                    corrections: &Corrections::new(),
                    checked: false,
                    progress: silent(),
                    waiter: waits_ok(),
                    differ: sees_no_change(),
                    auditor: no_audit(),
                },
                &mut always_found(),
            );
            assert_eq!(
                report.steps[0].outcome,
                StepOutcome::Executed,
                "verify = {verify:?}"
            );
        }
    }

    #[test]
    fn the_check_runs_before_the_step_not_after() {
        // A verifier that answers once and then reports nothing. If the
        // check happened after the input, the second call would fail the
        // step; it succeeds, so the only look happened up front.
        let mut looks = 0;
        let mut once = move |_: &Path, label: Option<&str>| {
            looks += 1;
            Ok(report_of(label, looks == 1))
        };
        let plan = Plan {
            steps: vec![planned(
                0,
                Step::Click {
                    target: "submit".into(),
                },
                vec![point(5.0, 6.0)],
            )],
        };
        let mut injector = Recording::default();
        let report = execute(
            &mut injector,
            &Context {
                flow: &flow_with(Verify::Each),
                plan: &plan,
                session: Path::new("/tmp/session"),
                monitors: &monitors(),
                corrections: &Corrections::new(),
                checked: false,
                progress: silent(),
                waiter: waits_ok(),
                differ: sees_no_change(),
                auditor: no_audit(),
            },
            &mut once,
        );
        assert_eq!(report.steps[0].outcome, StepOutcome::Executed);
        assert_eq!(injector.events, vec!["move 5,6", "click"]);
    }

    #[test]
    fn a_step_acts_on_where_a_region_moved_mid_run() {
        // The reflow bug: relocation used to happen once, before the first
        // step, so a step that shifted the page left every later step
        // clicking coordinates measured before the shift.
        let mut looks = 0;
        let mut drifting = move |_: &Path, _label: Option<&str>| {
            looks += 1;
            // Second look onward: "submit" has moved 120 physical pixels up.
            let shift = if looks >= 2 {
                r#","new_px":{"x":800,"y":280,"w":100,"h":80},"delta":{"dx":0,"dy":-120}"#
            } else {
                ""
            };
            Ok(serde_json::from_str(&format!(
                r#"{{"all_relocated":true,"results":[{{"label":"submit","found":true,"monitor":0{shift}}}]}}"#
            ))
            .expect("fixture"))
        };
        let step = || {
            planned(
                0,
                Step::Click {
                    target: "submit".into(),
                },
                vec![point(425.0, 220.0)],
            )
        };
        let plan = Plan {
            steps: vec![step(), step()],
        };
        // Pinned, because `Auto` resolves to logical on macOS and physical
        // everywhere else — an assertion on converted coordinates would
        // pass on one platform and fail on the others.
        let mut flow = flow_with(Verify::Each);
        flow.settings.space = Space::Logical;
        let mut injector = Recording::default();
        execute(
            &mut injector,
            &Context {
                flow: &flow,
                plan: &plan,
                session: Path::new("/tmp/session"),
                monitors: &monitors(),
                corrections: &Corrections::new(),
                checked: false,
                progress: silent(),
                waiter: waits_ok(),
                differ: sees_no_change(),
                auditor: no_audit(),
            },
            &mut drifting,
        );
        // Rect 800,280 100x80 centers at 850,320 physical; /2 on this 2x
        // monitor. The second click follows the region rather than
        // repeating the first click's coordinates.
        assert_eq!(
            injector.events,
            vec!["move 425,220", "click", "move 425,160", "click"]
        );
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
                checked: false,
                progress: silent(),
                waiter: waits_ok(),
                differ: sees_no_change(),
                auditor: no_audit(),
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
            &|_| {},
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
                checked: false,
                progress: silent(),
                waiter: waits_ok(),
                differ: sees_no_change(),
                auditor: no_audit(),
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
            &|_| {},
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
            &|_| {},
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
                checked: false,
                progress: silent(),
                waiter: waits_ok(),
                differ: sees_no_change(),
                auditor: no_audit(),
            },
            &mut appears_after(3),
        );
        assert_eq!(report.steps[0].outcome, StepOutcome::Verified);
    }

    #[test]
    fn a_timeout_reports_how_hard_it_looked() {
        // "Not found" without evidence is the complaint that fills
        // pyautogui's issue tracker.
        let mut flow = flow_with(Verify::Each);
        flow.settings.timeout_ms = 30;
        flow.settings.poll_ms = 10;
        let plan = Plan {
            steps: vec![planned(
                0,
                Step::WaitFor {
                    target: "dialog".into(),
                },
                vec![point(10.0, 10.0)],
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
                checked: false,
                progress: silent(),
                waiter: waits_out(0.42),
                differ: sees_no_change(),
                auditor: no_audit(),
            },
            &mut never_found(),
        );
        let detail = report.steps[0].detail.clone().expect("explained");
        assert!(detail.contains("polls"), "says how many times: {detail}");
        assert!(
            detail.contains("best match score"),
            "says how close: {detail}"
        );
        assert!(detail.contains("last look"), "says what it saw: {detail}");
    }

    #[test]
    fn an_ambiguous_match_is_never_reported_as_not_matching() {
        // A crop that matches perfectly in three places is refused — but
        // calling that "did not match its saved crop" is false, and
        // sends the reader looking for the wrong problem entirely.
        let mut ambiguous = |_: &Path, label: Option<&str>| {
            Ok(serde_json::from_str(&format!(
                r#"{{"all_relocated":false,"results":[{{"label":"{}","found":true,"ambiguous":true,"score":1.0}}]}}"#,
                label.unwrap_or("x")
            ))
            .expect("fixture"))
        };
        let plan = Plan {
            steps: vec![planned(
                0,
                Step::Verify {
                    target: "profile".into(),
                },
                vec![point(10.0, 10.0)],
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
                checked: false,
                progress: silent(),
                waiter: waits_ok(),
                differ: sees_no_change(),
                auditor: no_audit(),
            },
            &mut ambiguous,
        );
        let detail = report.steps[0].detail.clone().expect("explained");
        assert!(
            detail.contains("more than one place"),
            "names the real problem: {detail}"
        );
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
                checked: false,
                progress: silent(),
                waiter: waits_out(0.10),
                differ: sees_no_change(),
                auditor: no_audit(),
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

    /// The reason a timeout still spends one full-frame `find`.
    ///
    /// `pixelcoords wait` scores a region where the session left it, so a
    /// region that *moved* looks identical to one that never appeared —
    /// both are "did not match". `find` searches the whole frame and can
    /// tell those apart, and which one it was is the difference between a
    /// user guessing and a user fixing. Paid once, when the answer is
    /// already bad.
    #[test]
    fn a_timeout_on_a_region_that_moved_says_so() {
        let flow = waiting_flow("[[step]]\naction = \"wait_for\"\ntarget = \"submit\"\n", 5);
        let plan = Plan {
            steps: vec![planned(
                0,
                Step::WaitFor {
                    target: "submit".into(),
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
                checked: false,
                progress: silent(),
                waiter: waits_out(0.05),
                differ: sees_no_change(),
                auditor: no_audit(),
            },
            // `wait` never matched, but a full-frame look finds it 120 px up.
            &mut moved_up(),
        );
        assert_eq!(report.steps[0].outcome, StepOutcome::Failed);
        let detail = report.steps[0].detail.clone().expect("explained");
        assert!(detail.contains("timed out"), "detail: {detail}");
        assert!(
            detail.contains("it is on screen"),
            "distinguishes moved from absent: {detail}"
        );
        assert!(
            detail.contains("(0, -120)"),
            "names the drift so it can be fixed: {detail}"
        );
    }

    /// The record is written **as the run goes**, not at the end — so a
    /// run the watchdog stops, or one someone kills, still leaves one.
    #[test]
    fn every_step_is_recorded_as_it_finishes() {
        use std::cell::RefCell;
        let seen: RefCell<Vec<String>> = RefCell::new(Vec::new());
        let record = |event: &AuditEvent| seen.borrow_mut().push(event.line());

        let flow = flow_with(Verify::None);
        let plan = Plan {
            steps: vec![
                planned(
                    0,
                    Step::Click {
                        target: "submit".into(),
                    },
                    vec![point(430.0, 220.0)],
                ),
                planned(
                    1,
                    Step::Key {
                        chord: "esc".into(),
                    },
                    vec![],
                ),
            ],
        };
        execute(
            &mut Recording::default(),
            &Context {
                flow: &flow,
                plan: &plan,
                session: Path::new("/tmp/session"),
                monitors: &monitors(),
                corrections: &Corrections::new(),
                checked: true,
                progress: silent(),
                waiter: waits_ok(),
                differ: sees_no_change(),
                auditor: &record,
            },
            &mut always_found(),
        );

        let lines = seen.borrow();
        assert_eq!(lines.len(), 3, "one run line and two steps: {lines:?}");
        assert!(lines[0].contains(r#""event":"run""#), "{}", lines[0]);
        assert!(lines[0].contains("/tmp/session"), "{}", lines[0]);
        assert!(lines[1].contains("click submit"), "{}", lines[1]);
        // The coordinate actually sent, not the one the session saved.
        assert!(lines[1].contains("430"), "{}", lines[1]);
        assert!(lines[2].contains("key esc"), "{}", lines[2]);
    }

    /// A run stopped part-way still leaves a record of what it did before
    /// stopping — the case the log exists for.
    #[test]
    fn a_failed_run_records_the_failure_and_the_skips() {
        use std::cell::RefCell;
        let seen: RefCell<Vec<String>> = RefCell::new(Vec::new());
        let record = |event: &AuditEvent| seen.borrow_mut().push(event.line());

        let flow = waiting_flow(
            "[[step]]\naction = \"changed\"\ntarget = \"panel\"\n",
            5_000,
        );
        let plan = Plan {
            steps: vec![
                planned(
                    0,
                    Step::Changed {
                        target: "panel".into(),
                        tolerance: 0.0,
                    },
                    vec![point(1.0, 1.0)],
                ),
                planned(
                    1,
                    Step::Click {
                        target: "submit".into(),
                    },
                    vec![point(430.0, 220.0)],
                ),
            ],
        };
        execute(
            &mut Recording::default(),
            &Context {
                flow: &flow,
                plan: &plan,
                session: Path::new("/tmp/session"),
                monitors: &monitors(),
                corrections: &Corrections::new(),
                checked: true,
                progress: silent(),
                waiter: waits_ok(),
                differ: sees_no_change(),
                auditor: &record,
            },
            &mut always_found(),
        );

        let lines = seen.borrow();
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert!(lines[1].contains(r#""outcome":"failed""#), "{}", lines[1]);
        assert!(lines[1].contains("did not change"), "{}", lines[1]);
        assert!(lines[2].contains(r#""outcome":"skipped""#), "{}", lines[2]);
    }

    #[test]
    fn a_changed_step_passes_when_the_region_actually_changed() {
        let flow = waiting_flow(
            "[[step]]\naction = \"changed\"\ntarget = \"panel\"\n",
            5_000,
        );
        let plan = Plan {
            steps: vec![planned(
                0,
                Step::Changed {
                    target: "panel".into(),
                    tolerance: 0.0,
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
                checked: false,
                progress: silent(),
                waiter: waits_ok(),
                differ: sees_change(),
                auditor: no_audit(),
            },
            &mut always_found(),
        );
        assert_eq!(report.steps[0].outcome, StepOutcome::Verified);
    }

    /// The half that makes the verb worth having: an action that did
    /// nothing must not report success.
    #[test]
    fn a_changed_step_fails_when_nothing_moved_and_says_how_little() {
        let flow = waiting_flow(
            "[[step]]\naction = \"changed\"\ntarget = \"panel\"\n",
            5_000,
        );
        let plan = Plan {
            steps: vec![planned(
                0,
                Step::Changed {
                    target: "panel".into(),
                    tolerance: 0.0,
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
                checked: false,
                progress: silent(),
                waiter: waits_ok(),
                differ: sees_no_change(),
                auditor: no_audit(),
            },
            &mut always_found(),
        );
        assert_eq!(report.steps[0].outcome, StepOutcome::Failed);
        let detail = report.steps[0].detail.clone().expect("explained");
        assert!(detail.contains("did not change"), "detail: {detail}");
        assert!(
            detail.contains("0 of 7503 pixels"),
            "quantifies it rather than only refusing: {detail}"
        );
    }

    /// `changed` looks; it never injects. The same rule `verify`, `wait`
    /// and `gone` follow.
    #[test]
    fn a_changed_step_injects_nothing() {
        let flow = waiting_flow(
            "[[step]]\naction = \"changed\"\ntarget = \"panel\"\n",
            5_000,
        );
        let plan = Plan {
            steps: vec![planned(
                0,
                Step::Changed {
                    target: "panel".into(),
                    tolerance: 0.0,
                },
                vec![point(1.0, 1.0)],
            )],
        };
        let mut injector = Recording::default();
        execute(
            &mut injector,
            &Context {
                flow: &flow,
                plan: &plan,
                session: Path::new("/tmp/session"),
                monitors: &monitors(),
                corrections: &Corrections::new(),
                checked: false,
                progress: silent(),
                waiter: waits_ok(),
                differ: sees_change(),
                auditor: no_audit(),
            },
            &mut always_found(),
        );
        assert!(injector.events.is_empty(), "{:?}", injector.events);
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
                checked: false,
                progress: silent(),
                waiter: waits_ok(),
                differ: sees_no_change(),
                auditor: no_audit(),
            },
            &mut never_found(),
        );
        assert_eq!(report.steps[0].outcome, StepOutcome::Verified);
    }

    #[test]
    fn a_region_that_moved_a_long_way_is_still_acted_on() {
        // Distance is not evidence. A scrolled page moves a region far
        // outside the rect it was marked in, while the match stays
        // perfect — measured at 80 physical pixels per wheel click
        // against a 60px-tall region. Refusing on distance made
        // relocation useless on anything that scrolls, so the trust
        // decision belongs to pixelcoords: unambiguous and above the
        // score floor, or no correction at all.
        let mut corrections = Corrections::new();
        corrections.insert("submit".into(), point(10.0, 10.0));
        let plan = Plan {
            steps: vec![planned(
                0,
                Step::Click {
                    target: "submit".into(),
                },
                vec![point(900.0, 700.0)],
            )],
        };
        let mut injector = Recording::default();
        let report = execute(
            &mut injector,
            &Context {
                flow: &flow_with(Verify::None),
                plan: &plan,
                session: Path::new("/tmp/session"),
                monitors: &monitors(),
                corrections: &corrections,
                checked: false,
                progress: silent(),
                waiter: waits_ok(),
                differ: sees_no_change(),
                auditor: no_audit(),
            },
            &mut always_found(),
        );
        assert_eq!(report.steps[0].outcome, StepOutcome::Executed);
        assert_eq!(
            injector.events,
            vec!["move 10,10", "click"],
            "acts where the region is now, not where it was marked"
        );
    }

    #[test]
    fn an_ambiguous_region_never_produces_a_correction_to_act_on() {
        // What actually guards against acting on the wrong thing, now
        // that distance does not: a crop matching in more than one place
        // yields no correction, and preflight refuses the run outright.
        let report: verify::FindReport = serde_json::from_str(
            r#"{"all_relocated":false,"results":[{"label":"submit","found":true,"ambiguous":true,"score":1.0,"monitor":0,"new_px":{"x":10,"y":10,"w":4,"h":4}}]}"#,
        )
        .expect("fixture");
        assert!(!report.is_confirmed("submit"));
        assert!(
            report.corrected_point("submit").is_none(),
            "an ambiguous match must never become a point to act on"
        );
        let corrections = corrections(&report, &["submit"], &monitors(), Space::Logical);
        assert!(corrections.is_empty());
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
                checked: false,
                progress: silent(),
                waiter: waits_ok(),
                differ: sees_no_change(),
                auditor: no_audit(),
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
                checked: false,
                progress: silent(),
                waiter: waits_ok(),
                differ: sees_no_change(),
                auditor: no_audit(),
            },
            &mut always_found(),
        );
        assert_eq!(report.steps[0].outcome, StepOutcome::Executed);
    }
}
