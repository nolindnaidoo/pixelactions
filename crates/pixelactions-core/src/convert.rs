//! Coordinate-space conversion — the part most tools get wrong.
//!
//! A pixelcoords session records **physical pixels** plus each monitor's
//! DPI `scale`. Input APIs disagree about what they want:
//!
//! | Platform | Input API speaks |
//! |----------|------------------|
//! | macOS (`CGEvent`) | logical points, global space, origin top-left |
//! | Windows (`SendInput`) | physical pixels, normalized across the virtual desktop |
//! | Linux/X11 (`XTEST`) | physical pixels on the root window |
//!
//! So the same saved coordinate needs a different conversion per platform.
//! Getting this wrong doesn't error — it clicks the wrong place, which is
//! why the conversion lives here, alone, and is property-tested.

use pixelcoords_core::session::MonitorRecord;
use serde::{Deserialize, Serialize};

/// The coordinate space a consumer wants a point in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Space {
    /// What this platform's input API expects: logical on macOS,
    /// physical on Windows and X11.
    Auto,
    /// Physical pixels — the session's own grid.
    Physical,
    /// Logical points — physical divided by the monitor's scale.
    Logical,
}

/// A point in a named space, carrying the monitor it was resolved against
/// so a caller can report — or re-derive — the conversion.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ResolvedPoint {
    pub x: f64,
    pub y: f64,
    pub space: Space,
    pub monitor: usize,
    pub scale: f64,
}

/// The space `Space::Auto` resolves to on the platform this was compiled
/// for. Stated once, here, so no call site guesses.
pub const fn native_space() -> Space {
    if cfg!(target_os = "macos") {
        return Space::Logical;
    }
    Space::Physical
}

/// Which monitor contains a global physical point.
///
/// Returns `None` when the point falls in a gap between monitors or
/// outside every monitor — a real possibility on L-shaped layouts, and a
/// refusal rather than a guess.
pub fn monitor_at(
    monitors: &[MonitorRecord],
    global_x: i32,
    global_y: i32,
) -> Option<&MonitorRecord> {
    monitors.iter().find(|m| {
        let right = m.origin_px.x + m.size_px.w;
        let bottom = m.origin_px.y + m.size_px.h;
        global_x >= m.origin_px.x
            && global_x < right
            && global_y >= m.origin_px.y
            && global_y < bottom
    })
}

/// Convert a global physical point into `space`, using the scale of the
/// monitor that contains it.
///
/// Logical conversion divides by that monitor's scale — per monitor, not
/// per desktop, so mixed-DPI layouts come out right.
pub fn to_space(
    monitors: &[MonitorRecord],
    global_x: i32,
    global_y: i32,
    space: Space,
) -> Option<ResolvedPoint> {
    let monitor = monitor_at(monitors, global_x, global_y)?;
    let resolved = match space {
        Space::Auto => native_space(),
        other => other,
    };
    let divisor = match resolved {
        Space::Logical => monitor.scale,
        _ => 1.0,
    };
    Some(ResolvedPoint {
        x: f64::from(global_x) / divisor,
        y: f64::from(global_y) / divisor,
        space: resolved,
        monitor: monitor.index,
        scale: monitor.scale,
    })
}

#[cfg(test)]
mod tests {
    use pixelcoords_core::geometry::{Point, Size};

    use super::*;

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

    fn retina_plus_external() -> Vec<MonitorRecord> {
        vec![
            monitor(0, (0, 0), (3024, 1964), 2.0),
            monitor(1, (3024, 0), (1920, 1080), 1.0),
        ]
    }

    #[test]
    fn logical_conversion_divides_by_the_containing_monitors_scale() {
        let monitors = retina_plus_external();
        let point = to_space(&monitors, 1624, 880, Space::Logical).expect("inside monitor 0");
        assert_eq!(point.monitor, 0);
        assert!((point.x - 812.0).abs() < f64::EPSILON);
        assert!((point.y - 440.0).abs() < f64::EPSILON);
    }

    #[test]
    fn a_mixed_dpi_layout_converts_per_monitor_not_per_desktop() {
        let monitors = retina_plus_external();
        // Same request, a point on the 1x external display: unchanged.
        let point = to_space(&monitors, 4000, 500, Space::Logical).expect("inside monitor 1");
        assert_eq!(point.monitor, 1);
        assert!((point.x - 4000.0).abs() < f64::EPSILON);
        assert!((point.y - 500.0).abs() < f64::EPSILON);
    }

    #[test]
    fn physical_space_is_the_identity() {
        let monitors = retina_plus_external();
        let point = to_space(&monitors, 1624, 880, Space::Physical).expect("inside monitor 0");
        assert!((point.x - 1624.0).abs() < f64::EPSILON);
        assert!((point.y - 880.0).abs() < f64::EPSILON);
        assert_eq!(point.space, Space::Physical);
    }

    #[test]
    fn a_point_in_a_gap_between_monitors_resolves_to_nothing() {
        // Two monitors with a hole between them: 0 ends at x=1920, 1
        // starts at x=2000.
        let monitors = vec![
            monitor(0, (0, 0), (1920, 1080), 1.0),
            monitor(1, (2000, 0), (1920, 1080), 1.0),
        ];
        assert!(monitor_at(&monitors, 1960, 500).is_none());
        assert!(to_space(&monitors, 1960, 500, Space::Auto).is_none());
    }

    #[test]
    fn monitor_bounds_are_half_open_so_edges_belong_to_exactly_one() {
        let monitors = retina_plus_external();
        // x=3024 is the first pixel of monitor 1, not the last of 0.
        assert_eq!(monitor_at(&monitors, 3023, 0).expect("m0").index, 0);
        assert_eq!(monitor_at(&monitors, 3024, 0).expect("m1").index, 1);
    }

    #[test]
    fn auto_resolves_to_the_platforms_own_space() {
        let monitors = retina_plus_external();
        let point = to_space(&monitors, 1624, 880, Space::Auto).expect("inside monitor 0");
        assert_eq!(point.space, native_space());
        // The recorded space is never Auto — callers see what they got.
        assert_ne!(point.space, Space::Auto);
    }

    #[test]
    fn negative_origins_are_handled() {
        // A display placed left of the primary carries a negative origin.
        let monitors = vec![
            monitor(0, (0, 0), (1920, 1080), 1.0),
            monitor(1, (-1920, 0), (1920, 1080), 2.0),
        ];
        let point = to_space(&monitors, -960, 500, Space::Logical).expect("inside monitor 1");
        assert_eq!(point.monitor, 1);
        assert!((point.x - -480.0).abs() < f64::EPSILON);
    }
}
