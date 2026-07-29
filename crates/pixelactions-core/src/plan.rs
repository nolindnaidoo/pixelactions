//! Resolution: turn a flow plus a session into a concrete plan, or
//! refuse.
//!
//! Everything that can be known before touching the screen is decided
//! here — which labels exist, which point each step aims at, what space
//! that point is in. A flow that references a missing label fails during
//! planning, before a single event is injected. Half-executed flows are
//! the worst failure mode this tool could have.

use pixelcoords_core::session::SessionFile;

use crate::convert::{ResolvedPoint, Space, to_space};
use crate::flow::{Flow, Step};

/// A resolved point plus the region that produced it, so a caller can
/// check a corrected point still lands inside what the human marked.
#[derive(Debug, Clone, PartialEq)]
pub struct Bounds {
    pub label: String,
    /// The region in global physical pixels — the space corrections
    /// arrive in, so no conversion is needed to compare.
    pub shape: pixelcoords_core::geometry::Shape,
    pub monitor: usize,
}

/// One step, resolved to the points it will act on.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedStep {
    pub index: usize,
    pub summary: String,
    pub step: Step,
    /// Resolved points, in the order the step's targets appear. Empty for
    /// keyboard steps.
    pub points: Vec<ResolvedPoint>,
    /// The regions those points came from, positionally paired.
    pub bounds: Vec<Bounds>,
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
        let mut bounds = Vec::new();
        for label in step.targets() {
            let (point, region) = resolve_label(session, label, space)?;
            points.push(point);
            bounds.push(region);
        }
        steps.push(PlannedStep {
            index,
            summary: step.summary(),
            step: step.clone(),
            points,
            bounds,
        });
    }
    Ok(Plan { steps })
}

/// The point a labeled region will be acted on: its click point —
/// `pixelcoords-core`'s own interior-point logic, never reimplemented
/// here — translated to global coordinates and converted to `space`.
fn resolve_label(
    session: &SessionFile,
    label: &str,
    space: Space,
) -> Result<(ResolvedPoint, Bounds), PlanError> {
    let selection = session
        .selections
        .iter()
        .find(|s| s.label.eq_ignore_ascii_case(label))
        .ok_or_else(|| PlanError::UnknownLabel {
            label: label.to_string(),
            available: available_labels(session),
        })?;

    let monitor = session
        .monitors
        .iter()
        .find(|m| m.index == selection.monitor)
        .ok_or_else(|| PlanError::UnknownMonitor {
            label: label.to_string(),
            monitor: selection.monitor,
        })?;

    // click_point works in the shape's own (monitor-local) space; add the
    // monitor origin to reach the global desktop grid the conversion and
    // the input APIs both use.
    let local = selection.px.click_point();
    let global_x = monitor.origin_px.x + local.x;
    let global_y = monitor.origin_px.y + local.y;

    let point = to_space(&session.monitors, global_x, global_y, space).ok_or(
        PlanError::PointOffscreen {
            label: label.to_string(),
            x: global_x,
            y: global_y,
        },
    )?;
    let bounds = Bounds {
        label: label.to_string(),
        shape: selection.global_px.clone(),
        monitor: selection.monitor,
    };
    Ok((point, bounds))
}

/// Whether a global physical point lies inside the region it belongs to.
///
/// This is the guardrail: a corrected coordinate that has wandered
/// outside its own marked region means the relocation found something
/// else, and acting on it would click an unknown thing.
pub fn within_bounds(bounds: &Bounds, global_x: i32, global_y: i32) -> bool {
    bounds
        .shape
        .hit_test(pixelcoords_core::geometry::Point::new(global_x, global_y))
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
                },
            ],
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
        assert_eq!(plan.steps[0].bounds[0].label, "submit");
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
    fn a_point_inside_its_region_passes_the_bounds_check() {
        let plan = plan(
            &flow("[[step]]\naction = \"click\"\ntarget = \"submit\"\n"),
            &session(),
            Space::Physical,
        )
        .expect("planned");
        let bounds = &plan.steps[0].bounds[0];
        // The region's own center, in global physical pixels.
        assert!(within_bounds(bounds, 850, 440));
    }

    #[test]
    fn a_point_outside_its_region_fails_the_bounds_check() {
        let plan = plan(
            &flow("[[step]]\naction = \"click\"\ntarget = \"submit\"\n"),
            &session(),
            Space::Physical,
        )
        .expect("planned");
        let bounds = &plan.steps[0].bounds[0];
        // Far outside the 800,400 100x80 rect: a relocation that landed
        // here found something else entirely.
        assert!(!within_bounds(bounds, 2000, 1200));
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

    #[test]
    fn a_point_outside_every_monitor_is_refused_rather_than_guessed() {
        let mut broken = session();
        // Move the region past the right edge of every described monitor.
        broken.selections[0].px = Shape::Rect(Rect::new(99_000, 400, 10, 10));
        let error = plan(
            &flow("[[step]]\naction = \"click\"\ntarget = \"submit\"\n"),
            &broken,
            Space::Auto,
        )
        .expect_err("should refuse");
        assert!(matches!(error, PlanError::PointOffscreen { .. }));
    }
}
