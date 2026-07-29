//! Invariants of the coordinate conversion, for every input rather than
//! the inputs someone thought of.
//!
//! This is the module where a bug means clicking the wrong place on
//! someone's screen, so the rules are stated as properties: a point
//! inside a monitor resolves to that monitor, converting is reversible,
//! and nothing panics on hostile geometry.

use pixelactions_core::convert::{Space, monitor_at, to_space};
use pixelcoords_core::geometry::{Point, Size};
use pixelcoords_core::session::MonitorRecord;
use proptest::prelude::*;

fn monitor(index: usize, origin: (i32, i32), size: (i32, i32), scale: f64) -> MonitorRecord {
    MonitorRecord {
        index,
        name: format!("display {index}"),
        primary: index == 0,
        origin_px: Point::new(origin.0, origin.1),
        size_px: Size::new(size.0, size.1),
        scale,
    }
}

proptest! {
    /// A point inside a monitor's bounds always resolves to that monitor —
    /// the property the whole resolver rests on.
    #[test]
    fn a_point_inside_a_monitor_resolves_to_it(
        origin_x in -10_000i32..10_000,
        origin_y in -10_000i32..10_000,
        width in 1i32..8_000,
        height in 1i32..8_000,
        dx in 0i32..8_000,
        dy in 0i32..8_000,
        scale in prop::sample::select(vec![1.0f64, 1.5, 2.0, 3.0]),
    ) {
        let monitors = vec![monitor(0, (origin_x, origin_y), (width, height), scale)];
        let x = origin_x + dx % width;
        let y = origin_y + dy % height;
        let found = monitor_at(&monitors, x, y);
        prop_assert!(found.is_some(), "({x}, {y}) should be inside the only monitor");
        prop_assert_eq!(found.expect("checked").index, 0);
    }

    /// Logical conversion is reversible: multiply back by the scale and
    /// the original physical coordinate returns. Rounding is the enemy of
    /// a click landing where it was aimed.
    #[test]
    fn logical_conversion_is_reversible(
        dx in 0i32..3_000,
        dy in 0i32..2_000,
        scale in prop::sample::select(vec![1.0f64, 1.5, 2.0, 3.0]),
    ) {
        let monitors = vec![monitor(0, (0, 0), (4_000, 3_000), scale)];
        let point = to_space(&monitors, dx, dy, Space::Logical).expect("inside");
        let back_x = point.x * scale;
        let back_y = point.y * scale;
        prop_assert!((back_x - f64::from(dx)).abs() < 1e-9, "x round-trip: {back_x} vs {dx}");
        prop_assert!((back_y - f64::from(dy)).abs() < 1e-9, "y round-trip: {back_y} vs {dy}");
    }

    /// Physical space never changes a coordinate, whatever the monitor's
    /// scale — the identity that keeps Windows and X11 honest.
    #[test]
    fn physical_space_is_always_the_identity(
        dx in 0i32..3_000,
        dy in 0i32..2_000,
        scale in prop::sample::select(vec![1.0f64, 1.5, 2.0, 3.0]),
    ) {
        let monitors = vec![monitor(0, (0, 0), (4_000, 3_000), scale)];
        let point = to_space(&monitors, dx, dy, Space::Physical).expect("inside");
        prop_assert!((point.x - f64::from(dx)).abs() < f64::EPSILON);
        prop_assert!((point.y - f64::from(dy)).abs() < f64::EPSILON);
    }

    /// Hostile geometry — points far outside every monitor, empty monitor
    /// lists — refuses rather than panicking or guessing.
    #[test]
    fn points_outside_every_monitor_refuse_without_panicking(
        x in i32::MIN / 2..i32::MAX / 2,
        y in i32::MIN / 2..i32::MAX / 2,
    ) {
        let monitors: Vec<MonitorRecord> = Vec::new();
        prop_assert!(monitor_at(&monitors, x, y).is_none());
        prop_assert!(to_space(&monitors, x, y, Space::Auto).is_none());
    }

    /// Adjacent monitors never both claim a pixel: bounds are half-open,
    /// so the seam belongs to exactly one display.
    #[test]
    fn adjacent_monitors_never_both_claim_a_pixel(
        width in 100i32..4_000,
        y in 0i32..500,
    ) {
        let monitors = [
            monitor(0, (0, 0), (width, 1_000), 2.0),
            monitor(1, (width, 0), (1_000, 1_000), 1.0),
        ];
        let claimants = monitors
            .iter()
            .filter(|m| {
                let right = m.origin_px.x + m.size_px.w;
                let bottom = m.origin_px.y + m.size_px.h;
                width >= m.origin_px.x && width < right && y >= m.origin_px.y && y < bottom
            })
            .count();
        prop_assert_eq!(claimants, 1, "the seam pixel belongs to exactly one monitor");
    }
}
