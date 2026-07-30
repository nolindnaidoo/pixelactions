//! Placing a session's physical pixel inside a Wayland input region.
//!
//! Every other platform takes absolute coordinates in a space this crate
//! can name at compile time — logical points on macOS, physical pixels on
//! Windows and X11. Wayland does not. Absolute pointer motion is bounded
//! by a **region** the compositor grants at runtime, derived from the
//! screencast stream the user consented to share, and expressed in the
//! compositor's *logical* pixels. "Exact physical pixel" on Wayland
//! therefore means "exact pixel within a region you were granted".
//!
//! So the last hop of the conversion cannot live in [`crate::convert`]
//! with the others: it needs a runtime fact. What lives here is the
//! arithmetic — regions and monitors in, a placement out — so the part
//! that decides where to click stays testable without a compositor. The
//! injector supplies the regions; it does no math of its own.
//!
//! Two rules carried over from [`crate::convert`], for the same reason:
//! the divisor is the **containing monitor's** scale as the session
//! recorded it, never a desktop-wide one; and a point that does not land
//! is **refused**, never clamped. A clamped point clicks somewhere
//! plausible and wrong, which is worse than an error.

use pixelcoords_core::session::MonitorRecord;
use serde::Serialize;

/// One absolute-input region, as the compositor described it.
///
/// Sizes and offsets are the compositor's logical pixels. `scale` is the
/// scale it reports for the region; it is recorded so `doctor` can show
/// it and so a mismatch against the session is visible, but the mapping
/// deliberately does **not** divide by it — see [`place`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Region {
    pub offset_x: i32,
    pub offset_y: i32,
    pub width: i32,
    pub height: i32,
    pub scale: f64,
    /// Ties this region to a screencast stream, when the compositor says
    /// so. Optional in the protocol, so never relied on for correctness.
    pub mapping_id: Option<String>,
}

impl Region {
    /// The logical size a monitor of this physical size and scale would
    /// occupy — the quantity a region is matched against.
    fn logical_size_of(monitor: &MonitorRecord) -> (i32, i32) {
        let scale = if monitor.scale > 0.0 {
            monitor.scale
        } else {
            1.0
        };
        (
            (f64::from(monitor.size_px.w) / scale).round() as i32,
            (f64::from(monitor.size_px.h) / scale).round() as i32,
        )
    }

    /// Whether this region plausibly *is* the given monitor.
    ///
    /// Compared in logical pixels derived from the session's own scale
    /// rather than from `self.scale`: the session is the authoritative
    /// record of a monitor's DPI factor, and compositors have been
    /// observed reporting a region scale of 1.0 regardless.
    fn covers(&self, monitor: &MonitorRecord) -> bool {
        Self::logical_size_of(monitor) == (self.width, self.height)
    }

    /// Whether a logical point falls inside this region.
    ///
    /// Test-only on purpose: `place` needs no bounds check because
    /// `covers` makes one impossible to fail, and this is what the test
    /// that pins that invariant checks against. Shipping it as public API
    /// would imply callers should be testing bounds themselves.
    #[cfg(test)]
    fn contains(&self, x: f64, y: f64) -> bool {
        let left = f64::from(self.offset_x);
        let top = f64::from(self.offset_y);
        x >= left
            && y >= top
            && x < left + f64::from(self.width)
            && y < top + f64::from(self.height)
    }
}

/// A point resolved into one region's logical space.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Placement {
    /// Index into the regions that were offered — which one to send to.
    pub region: usize,
    pub x: f64,
    pub y: f64,
}

/// Why a physical point could not be placed in any granted region.
///
/// Every variant is a refusal. There is deliberately no "closest region"
/// fallback: the whole promise of this tool is that a click lands where a
/// human marked it, and a near miss breaks that promise silently.
// Not `Eq`: two variants carry the coordinates that failed, and those are
// f64 because that is what a scaled division produces.
#[derive(Debug, Clone, PartialEq)]
pub enum PlaceError {
    /// The point is in a gap between monitors, or off the desktop.
    OutsideEveryMonitor { x: i32, y: i32 },
    /// Nothing was granted that looks like the monitor the point is on —
    /// the usual cause is consenting to share one screen of several.
    NoRegionForMonitor { monitor: usize },
    /// More than one granted region matches the monitor, so choosing one
    /// would be a guess.
    AmbiguousRegion { monitor: usize, matches: usize },
}

impl std::fmt::Display for PlaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OutsideEveryMonitor { x, y } => write!(
                f,
                "the point ({x}, {y}) is not on any monitor the session describes"
            ),
            Self::NoRegionForMonitor { monitor } => write!(
                f,
                "no shared region matches monitor {monitor} — the grant covers a \
                 different screen, so re-run and share the screen the region is on"
            ),
            Self::AmbiguousRegion { monitor, matches } => write!(
                f,
                "{matches} shared regions match monitor {monitor}, so which one to \
                 aim at would be a guess — share a single screen instead"
            ),
        }
    }
}

impl std::error::Error for PlaceError {}

/// Place a global physical point into the logical space of whichever
/// granted region covers its monitor.
///
/// The arithmetic is deliberately small: subtract the monitor's physical
/// origin, divide by that monitor's scale to get logical pixels, then add
/// the region's logical offset. The region's offset *is* the monitor's
/// position in the compositor's logical layout, which is why no global
/// logical layout has to be known or guessed.
pub fn place(
    monitors: &[MonitorRecord],
    regions: &[Region],
    global_x: i32,
    global_y: i32,
) -> Result<Placement, PlaceError> {
    let Some(monitor) = crate::convert::monitor_at(monitors, global_x, global_y) else {
        return Err(PlaceError::OutsideEveryMonitor {
            x: global_x,
            y: global_y,
        });
    };
    let mut matching = regions
        .iter()
        .enumerate()
        .filter(|(_, region)| region.covers(monitor));
    let Some((index, region)) = matching.next() else {
        return Err(PlaceError::NoRegionForMonitor {
            monitor: monitor.index,
        });
    };
    let extra = matching.count();
    if extra > 0 {
        return Err(PlaceError::AmbiguousRegion {
            monitor: monitor.index,
            matches: extra + 1,
        });
    }

    let scale = if monitor.scale > 0.0 {
        monitor.scale
    } else {
        1.0
    };
    // No bounds check follows, and none is needed: `covers` already
    // established that the region's logical size *equals* this monitor's,
    // and a point on the monitor is at most `size - 1` physical pixels
    // from its origin — so the divided offset always lands inside. A
    // guard here would be a branch no input can reach.
    let x = f64::from(region.offset_x) + f64::from(global_x - monitor.origin_px.x) / scale;
    let y = f64::from(region.offset_y) + f64::from(global_y - monitor.origin_px.y) / scale;
    Ok(Placement {
        region: index,
        x,
        y,
    })
}

/// Turn a point in a region's logical space back into a global physical
/// pixel.
///
/// The inverse exists for one reason: the kill switch. Where a compositor
/// can report the pointer's position it reports it in the region's space,
/// and the corner check happens in the session's physical grid — so the
/// reading has to come back before it can be judged. Keeping the inverse
/// next to the forward mapping is how they stay consistent.
pub fn unplace(
    monitors: &[MonitorRecord],
    regions: &[Region],
    placement: Placement,
) -> Option<(f64, f64)> {
    let region = regions.get(placement.region)?;
    let monitor = monitors.iter().find(|monitor| region.covers(monitor))?;
    let scale = if monitor.scale > 0.0 {
        monitor.scale
    } else {
        1.0
    };
    Some((
        f64::from(monitor.origin_px.x) + (placement.x - f64::from(region.offset_x)) * scale,
        f64::from(monitor.origin_px.y) + (placement.y - f64::from(region.offset_y)) * scale,
    ))
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

    fn region(offset: (i32, i32), size: (i32, i32), scale: f64) -> Region {
        Region {
            offset_x: offset.0,
            offset_y: offset.1,
            width: size.0,
            height: size.1,
            scale,
            mapping_id: None,
        }
    }

    /// The shape this was first verified against by hand: one 1x monitor,
    /// one region, offsets zero. The mapping is the identity, and if this
    /// ever stops being true the manual verification stops meaning
    /// anything.
    #[test]
    fn a_single_unscaled_monitor_maps_one_to_one() {
        let monitors = vec![monitor(0, (0, 0), (1800, 1130), 1.0)];
        let regions = vec![region((0, 0), (1800, 1130), 1.0)];
        let placed = place(&monitors, &regions, 900, 565).expect("centre of the only screen");
        assert_eq!(placed.region, 0);
        assert!((placed.x - 900.0).abs() < f64::EPSILON);
        assert!((placed.y - 565.0).abs() < f64::EPSILON);
    }

    #[test]
    fn a_scaled_monitor_divides_by_the_sessions_scale() {
        // A 2x panel: 3600x2260 physical is 1800x1130 logical, which is
        // the size the compositor grants.
        let monitors = vec![monitor(0, (0, 0), (3600, 2260), 2.0)];
        let regions = vec![region((0, 0), (1800, 1130), 2.0)];
        let placed = place(&monitors, &regions, 1624, 880).expect("inside");
        assert!((placed.x - 812.0).abs() < f64::EPSILON);
        assert!((placed.y - 440.0).abs() < f64::EPSILON);
    }

    /// The reason `covers` ignores the region's own scale: a compositor
    /// reporting 1.0 for a 2x screen must not change where we click.
    #[test]
    fn a_region_scale_of_one_on_a_retina_screen_does_not_move_the_click() {
        let monitors = vec![monitor(0, (0, 0), (3600, 2260), 2.0)];
        let honest = vec![region((0, 0), (1800, 1130), 2.0)];
        let understated = vec![region((0, 0), (1800, 1130), 1.0)];
        assert_eq!(
            place(&monitors, &honest, 1624, 880).expect("honest"),
            place(&monitors, &understated, 1624, 880).expect("understated")
        );
    }

    #[test]
    fn a_mixed_dpi_layout_uses_each_monitors_own_scale() {
        let monitors = vec![
            monitor(0, (0, 0), (3600, 2260), 2.0),
            monitor(1, (3600, 0), (1920, 1080), 1.0),
        ];
        // Logical layout puts the 1x screen right of the 2x one's 1800.
        let regions = vec![
            region((0, 0), (1800, 1130), 2.0),
            region((1800, 0), (1920, 1080), 1.0),
        ];
        let on_retina = place(&monitors, &regions, 1624, 880).expect("monitor 0");
        assert_eq!(on_retina.region, 0);
        assert!((on_retina.x - 812.0).abs() < f64::EPSILON);

        let on_external = place(&monitors, &regions, 4000, 500).expect("monitor 1");
        assert_eq!(on_external.region, 1);
        // 4000 physical is 400 into a 1x screen, at logical offset 1800.
        assert!((on_external.x - 2200.0).abs() < f64::EPSILON);
        assert!((on_external.y - 500.0).abs() < f64::EPSILON);
    }

    #[test]
    fn a_display_left_of_primary_carries_negative_origins() {
        let monitors = vec![
            monitor(0, (0, 0), (1920, 1080), 1.0),
            monitor(1, (-2560, 0), (2560, 1440), 2.0),
        ];
        let regions = vec![
            region((0, 0), (1920, 1080), 1.0),
            region((-1280, 0), (1280, 720), 2.0),
        ];
        let placed = place(&monitors, &regions, -1280, 720).expect("inside monitor 1");
        assert_eq!(placed.region, 1);
        // 1280 physical into the 2x screen is 640 logical, from -1280.
        assert!((placed.x - -640.0).abs() < f64::EPSILON);
        assert!((placed.y - 360.0).abs() < f64::EPSILON);
    }

    #[test]
    fn a_point_off_every_monitor_is_refused() {
        let monitors = vec![monitor(0, (0, 0), (1800, 1130), 1.0)];
        let regions = vec![region((0, 0), (1800, 1130), 1.0)];
        assert_eq!(
            place(&monitors, &regions, 5000, 5000),
            Err(PlaceError::OutsideEveryMonitor { x: 5000, y: 5000 })
        );
    }

    /// Sharing one screen of two is the common consent mistake, and it
    /// has to name the fix rather than click on the wrong screen.
    #[test]
    fn sharing_the_wrong_screen_is_refused_by_name() {
        let monitors = vec![
            monitor(0, (0, 0), (1920, 1080), 1.0),
            monitor(1, (1920, 0), (2560, 1440), 1.0),
        ];
        let regions = vec![region((0, 0), (1920, 1080), 1.0)];
        assert_eq!(
            place(&monitors, &regions, 3000, 700),
            Err(PlaceError::NoRegionForMonitor { monitor: 1 })
        );
    }

    #[test]
    fn two_identical_screens_shared_at_once_are_ambiguous_not_guessed() {
        let monitors = vec![
            monitor(0, (0, 0), (1920, 1080), 1.0),
            monitor(1, (1920, 0), (1920, 1080), 1.0),
        ];
        // Same logical size, so nothing distinguishes them.
        let regions = vec![
            region((0, 0), (1920, 1080), 1.0),
            region((1920, 0), (1920, 1080), 1.0),
        ];
        assert_eq!(
            place(&monitors, &regions, 100, 100),
            Err(PlaceError::AmbiguousRegion {
                monitor: 0,
                matches: 2
            })
        );
    }

    #[test]
    fn no_regions_at_all_is_refused() {
        let monitors = vec![monitor(0, (0, 0), (1800, 1130), 1.0)];
        assert_eq!(
            place(&monitors, &[], 100, 100),
            Err(PlaceError::NoRegionForMonitor { monitor: 0 })
        );
    }

    /// The invariant that lets `place` skip a bounds check: whatever the
    /// scale or origin, every point on a covered monitor lands inside the
    /// region. Stated over the awkward cases rather than assumed.
    #[test]
    fn every_point_on_a_covered_monitor_lands_inside_its_region() {
        let cases = [
            ((0, 0), (1800, 1130), 1.0, (0, 0)),
            ((0, 0), (3600, 2260), 2.0, (0, 0)),
            ((-2560, -100), (2560, 1440), 2.0, (-1280, -50)),
            // Sizes that do not divide evenly by the scale.
            ((0, 0), (1801, 1131), 2.0, (0, 0)),
            ((0, 0), (1799, 1129), 2.0, (400, 300)),
            // A downscaled display, where logical is larger than physical.
            ((0, 0), (1000, 800), 0.5, (0, 0)),
        ];
        for (origin, size, scale, offset) in cases {
            let monitors = vec![monitor(0, origin, size, scale)];
            let logical = Region::logical_size_of(&monitors[0]);
            let regions = vec![region(offset, (logical.0, logical.1), scale)];
            // The extremes are where an off-by-one would show up.
            let corners = [
                (origin.0, origin.1),
                (origin.0 + size.0 - 1, origin.1),
                (origin.0, origin.1 + size.1 - 1),
                (origin.0 + size.0 - 1, origin.1 + size.1 - 1),
            ];
            for (x, y) in corners {
                let placed = place(&monitors, &regions, x, y)
                    .unwrap_or_else(|error| panic!("({x}, {y}) on {size:?}@{scale}: {error}"));
                assert!(
                    regions[0].contains(placed.x, placed.y),
                    "({x}, {y}) on {size:?}@{scale} mapped to ({}, {}), outside {:?}",
                    placed.x,
                    placed.y,
                    regions[0]
                );
            }
        }
    }

    #[test]
    fn the_far_edge_belongs_to_the_region_but_one_past_it_does_not() {
        let monitors = vec![monitor(0, (0, 0), (1800, 1130), 1.0)];
        let regions = vec![region((0, 0), (1800, 1130), 1.0)];
        assert!(place(&monitors, &regions, 1799, 1129).is_ok());
        // 1800 is off this monitor entirely, so it fails earlier.
        assert_eq!(
            place(&monitors, &regions, 1800, 0),
            Err(PlaceError::OutsideEveryMonitor { x: 1800, y: 0 })
        );
    }

    #[test]
    fn a_nonsense_scale_is_treated_as_one_rather_than_dividing_by_zero() {
        let monitors = vec![monitor(0, (0, 0), (1800, 1130), 0.0)];
        let regions = vec![region((0, 0), (1800, 1130), 1.0)];
        let placed = place(&monitors, &regions, 900, 565).expect("scale 0 falls back to 1");
        assert!((placed.x - 900.0).abs() < f64::EPSILON);
    }

    #[test]
    fn unplace_returns_the_physical_pixel_it_came_from() {
        let monitors = vec![
            monitor(0, (0, 0), (3600, 2260), 2.0),
            monitor(1, (3600, 0), (1920, 1080), 1.0),
        ];
        let regions = vec![
            region((0, 0), (1800, 1130), 2.0),
            region((1800, 0), (1920, 1080), 1.0),
        ];
        for (x, y) in [(1624, 880), (4000, 500), (0, 0)] {
            let placed = place(&monitors, &regions, x, y).expect("inside");
            let (back_x, back_y) = unplace(&monitors, &regions, placed).expect("reversible");
            assert!((back_x - f64::from(x)).abs() < 1e-9, "x for ({x}, {y})");
            assert!((back_y - f64::from(y)).abs() < 1e-9, "y for ({x}, {y})");
        }
    }

    #[test]
    fn unplace_of_an_unknown_region_is_nothing_rather_than_a_panic() {
        let monitors = vec![monitor(0, (0, 0), (1800, 1130), 1.0)];
        let regions = vec![region((0, 0), (1800, 1130), 1.0)];
        let bogus = Placement {
            region: 7,
            x: 0.0,
            y: 0.0,
        };
        assert!(unplace(&monitors, &regions, bogus).is_none());
    }

    #[test]
    fn the_refusals_all_say_something_useful() {
        let messages = [
            PlaceError::OutsideEveryMonitor { x: 1, y: 2 }.to_string(),
            PlaceError::NoRegionForMonitor { monitor: 1 }.to_string(),
            PlaceError::AmbiguousRegion {
                monitor: 0,
                matches: 2,
            }
            .to_string(),
        ];
        for message in messages {
            assert!(message.len() > 30, "too terse: {message}");
        }
    }
}
