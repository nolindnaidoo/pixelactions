//! Resolution: turn a flow plus a session into a concrete plan, or
//! refuse.
//!
//! Everything that can be known before touching the screen is decided
//! here — which labels exist, which point each step aims at, what space
//! that point is in. A flow that references a missing label fails during
//! planning, before a single event is injected. Half-executed flows are
//! the worst failure mode this tool could have.

use pixelcoords_core::locate::Delta;
use pixelcoords_core::resolve::{ResolveError, resolve};
use pixelcoords_core::session::SessionFile;
use pixelcoords_core::space::{Origin, Resolved};

use crate::convert::{ResolvedPoint, Space, to_space};
use crate::flow::{Flow, Step};

/// One step, resolved to the points it will act on.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedStep {
    pub index: usize,
    pub summary: String,
    pub step: Step,
    /// Resolved points, in the order the step's targets appear. Empty for
    /// keyboard steps.
    pub points: Vec<ResolvedPoint>,
}

/// A whole flow, resolved. Holding one means every label existed and
/// every point converted.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub steps: Vec<PlannedStep>,
}

/// Why a flow could not be planned. Each variant names the fix.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum PlanError {
    #[error("no selection labeled {label:?} in this session — it has: {available}")]
    UnknownLabel { label: String, available: String },
    #[error("selection {label:?} sits on monitor {monitor}, which this session does not describe")]
    UnknownMonitor { label: String, monitor: usize },
    #[error(
        "selection {label:?} resolves to ({x}, {y}), which is outside every monitor in this session"
    )]
    PointOffscreen { label: String, x: i32, y: i32 },
}

/// Resolve a flow against a session.
///
/// `space` overrides the flow's own setting when a caller needs a
/// specific space (dry-run reporting, tests).
pub fn plan(flow: &Flow, session: &SessionFile, space: Space) -> Result<Plan, PlanError> {
    let mut steps = Vec::with_capacity(flow.steps.len());
    for (index, step) in flow.steps.iter().enumerate() {
        let mut points = Vec::new();
        for label in step.targets() {
            points.push(resolve_label(session, label, space)?);
        }
        steps.push(PlannedStep {
            index,
            summary: step.summary(),
            step: step.clone(),
            points,
        });
    }
    Ok(Plan { steps })
}

/// Nothing has relocated when a plan is built — `plan` never captures.
/// `run` corrects for drift afterwards, against a fresh `find`.
const NO_DRIFT: &dyn Fn(usize) -> Option<(f64, Delta)> = &|_| None;

/// The point a labeled region will be acted on.
///
/// The label lookup, the monitor lookup, the interior click point and the
/// hop from monitor-local to global coordinates are all
/// `pixelcoords_core::resolve`'s. `design/08` calls that the seam, and
/// says why: reassembling it here means this tool can get DPI wrong in a
/// way the crate that owns the geometry cannot.
///
/// **What stays ours is the refusal.** `resolve` answers in the monitor a
/// selection *claims*; a point that lands in a gap between monitors, or
/// past the edge of every one, is still something to refuse rather than
/// guess at — so the containing-monitor check runs on the physical answer
/// before any conversion, and `to_space` converts against the monitor
/// that actually holds the point.
fn resolve_label(
    session: &SessionFile,
    label: &str,
    space: Space,
) -> Result<ResolvedPoint, PlanError> {
    let resolved = resolve(
        session,
        Some(label),
        Origin::Global,
        Resolved::Physical,
        NO_DRIFT,
    )
    .map_err(|error| match error {
        ResolveError::UnknownMonitor { monitor, .. } => PlanError::UnknownMonitor {
            label: label.to_string(),
            monitor,
        },
        // NoSelections, UnknownLabel, and the two window-space errors all
        // mean the same thing to a caller here: that label is not
        // actionable. Ours names the alternatives; `Origin::Global` never
        // reaches the window-space pair.
        _ => PlanError::UnknownLabel {
            label: label.to_string(),
            available: available_labels(session),
        },
    })?;

    let point = resolved
        .first()
        .ok_or_else(|| PlanError::UnknownLabel {
            label: label.to_string(),
            available: available_labels(session),
        })?
        .point;

    to_space(&session.monitors, point.x, point.y, space).ok_or(PlanError::PointOffscreen {
        label: label.to_string(),
        x: point.x,
        y: point.y,
    })
}

fn available_labels(session: &SessionFile) -> String {
    let labels: Vec<&str> = session
        .selections
        .iter()
        .map(|s| s.label.as_str())
        .filter(|l| !l.is_empty())
        .collect();
    if labels.is_empty() {
        return "no labeled selections".to_string();
    }
    labels.join(", ")
}

#[cfg(test)]
mod tests {
    use pixelcoords_core::geometry::{Point, Size};
    use pixelcoords_core::geometry::{Rect, Shape, ToolKind};
    use pixelcoords_core::session::{MonitorRecord, SelectionRecord};

    use super::*;
    use crate::flow::Flow;

    fn session() -> SessionFile {
        SessionFile {
            schema: 1,
            app: pixelcoords_core::session::AppInfo {
                name: "pixelcoords".into(),
                version: "0.1.1".into(),
            },
            created_utc: "2026-07-28T00:00:00Z".into(),
            platform: Some("macos".into()),
            capture: None,
            name: None,
            monitors: vec![
                MonitorRecord {
                    index: 0,
                    name: "built-in".into(),
                    primary: true,
                    origin_px: Point::new(0, 0),
                    size_px: Size::new(3024, 1964),
                    scale: 2.0,
                },
                MonitorRecord {
                    index: 1,
                    name: "external".into(),
                    primary: false,
                    origin_px: Point::new(3024, 0),
                    size_px: Size::new(1920, 1080),
                    scale: 1.0,
                },
            ],
            target: None,
            selections: vec![
                SelectionRecord {
                    shape: ToolKind::Rect,
                    label: "submit".into(),
                    monitor: 0,
                    px: Shape::Rect(Rect::new(800, 400, 100, 80)),
                    global_px: Shape::Rect(Rect::new(800, 400, 100, 80)),
                    rot_deg: None,
                    window_px: None,
                    crop: "crop-0-submit.png".into(),
                    color: None,
                },
                SelectionRecord {
                    shape: ToolKind::Rect,
                    label: "far".into(),
                    monitor: 1,
                    px: Shape::Rect(Rect::new(100, 100, 40, 40)),
                    global_px: Shape::Rect(Rect::new(3124, 100, 40, 40)),
                    rot_deg: None,
                    window_px: None,
                    crop: "crop-1-far.png".into(),
                    color: None,
                },
            ],
            // Rulers are pixelcoords 0.5.0's; nothing here acts on one.
            measures: Vec::new(),
        }
    }

    fn flow(body: &str) -> Flow {
        Flow::parse(&format!("session = \"s\"\n{body}")).expect("valid flow")
    }

    #[test]
    fn a_click_resolves_to_the_regions_click_point_in_logical_points() {
        let plan = plan(
            &flow("[[step]]\naction = \"click\"\ntarget = \"submit\"\n"),
            &session(),
            Space::Logical,
        )
        .expect("planned");
        let point = plan.steps[0].points[0];
        // Rect 800,400 100x80 centers at 850,440 physical; /2 on a Retina
        // monitor.
        assert!((point.x - 425.0).abs() < f64::EPSILON);
        assert!((point.y - 220.0).abs() < f64::EPSILON);
        assert_eq!(point.monitor, 0);
    }

    #[test]
    fn a_selection_on_a_second_monitor_uses_that_monitors_scale() {
        let plan = plan(
            &flow("[[step]]\naction = \"click\"\ntarget = \"far\"\n"),
            &session(),
            Space::Logical,
        )
        .expect("planned");
        let point = plan.steps[0].points[0];
        // Monitor 1 is 1x: global 3144,120 stays put.
        assert_eq!(point.monitor, 1);
        assert!((point.x - 3144.0).abs() < f64::EPSILON);
    }

    #[test]
    fn labels_match_case_insensitively_like_the_sister_tool() {
        assert!(
            plan(
                &flow("[[step]]\naction = \"click\"\ntarget = \"SUBMIT\"\n"),
                &session(),
                Space::Physical,
            )
            .is_ok()
        );
    }

    #[test]
    fn an_unknown_label_fails_planning_and_lists_the_real_ones() {
        let error = plan(
            &flow("[[step]]\naction = \"click\"\ntarget = \"nope\"\n"),
            &session(),
            Space::Auto,
        )
        .expect_err("should refuse");
        let PlanError::UnknownLabel { available, .. } = &error else {
            panic!("wrong error: {error}");
        };
        assert!(
            available.contains("submit"),
            "names the options: {available}"
        );
    }

    #[test]
    fn planning_fails_before_any_step_when_a_later_label_is_missing() {
        // The first step is fine; the second is not. Planning must refuse
        // the whole flow rather than half-execute it.
        let result = plan(
            &flow(
                "[[step]]\naction = \"click\"\ntarget = \"submit\"\n\n[[step]]\naction = \"click\"\ntarget = \"ghost\"\n",
            ),
            &session(),
            Space::Auto,
        );
        assert!(matches!(result, Err(PlanError::UnknownLabel { .. })));
    }

    #[test]
    fn a_drag_resolves_both_ends() {
        let plan = plan(
            &flow("[[step]]\naction = \"drag\"\nfrom = \"submit\"\nto = \"far\"\n"),
            &session(),
            Space::Physical,
        )
        .expect("planned");
        assert_eq!(plan.steps[0].points.len(), 2);
        assert_eq!(plan.steps[0].points[0].monitor, 0);
        assert_eq!(plan.steps[0].points[1].monitor, 1);
    }

    #[test]
    fn keyboard_steps_resolve_to_no_points() {
        let plan = plan(
            &flow("[[step]]\naction = \"type\"\ntext = \"hi\"\n"),
            &session(),
            Space::Auto,
        )
        .expect("planned");
        assert!(plan.steps[0].points.is_empty());
    }

    #[test]
    fn a_selection_on_an_undescribed_monitor_is_an_error() {
        let mut broken = session();
        broken.selections[0].monitor = 9;
        let error = plan(
            &flow("[[step]]\naction = \"click\"\ntarget = \"submit\"\n"),
            &broken,
            Space::Auto,
        )
        .expect_err("should refuse");
        assert!(matches!(
            error,
            PlanError::UnknownMonitor { monitor: 9, .. }
        ));
    }

    /// Both `px` and `global_px` move, because a session that pixelcoords
    /// wrote keeps them in step — `global_px` is the monitor-local shape
    /// already translated, not an independent field.
    ///
    /// That distinction is new. Global answers now come from `global_px`
    /// via `pixelcoords_core::resolve` instead of being re-derived here as
    /// `monitor.origin_px + px.click_point()`, so moving only `px` no
    /// longer moves the answer. Re-deriving what the session already
    /// states was exactly the reassembly `design/08` wanted gone.
    #[test]
    fn a_point_outside_every_monitor_is_refused_rather_than_guessed() {
        let mut broken = session();
        // Past the right edge of every described monitor.
        broken.selections[0].px = Shape::Rect(Rect::new(99_000, 400, 10, 10));
        broken.selections[0].global_px = Shape::Rect(Rect::new(99_000, 400, 10, 10));
        let error = plan(
            &flow("[[step]]\naction = \"click\"\ntarget = \"submit\"\n"),
            &broken,
            Space::Auto,
        )
        .expect_err("should refuse");
        assert!(matches!(error, PlanError::PointOffscreen { .. }));
    }

    /// The rounding rule, pinned. A physical coordinate that does not
    /// divide evenly rounds to the nearest logical point rather than
    /// truncating toward zero, which is both what
    /// `pixelcoords resolve --units auto` answers and the more accurate
    /// of the two once an injector converts to an integer anyway.
    #[test]
    fn an_odd_physical_coordinate_rounds_rather_than_truncating() {
        let mut odd = session();
        // Height 70 puts the click point at y = 400 + 35 = 435 physical —
        // odd, so scale 2.0 cannot divide it evenly. x stays even, so only
        // one axis is under test.
        odd.selections[0].px = Shape::Rect(Rect::new(800, 400, 100, 70));
        odd.selections[0].global_px = Shape::Rect(Rect::new(800, 400, 100, 70));
        let plan = plan(
            &flow("[[step]]\naction = \"click\"\ntarget = \"submit\"\n"),
            &odd,
            Space::Logical,
        )
        .expect("planned");
        let point = plan.steps[0].points[0];
        // 435 / 2.0 = 217.5. Rounds to 218; truncating gave 217.
        assert!((point.y - 218.0).abs() < f64::EPSILON, "{}", point.y);
        assert!((point.x - 425.0).abs() < f64::EPSILON, "{}", point.x);
    }
}
