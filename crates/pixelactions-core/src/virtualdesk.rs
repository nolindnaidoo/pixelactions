//! Normalizing a session's physical pixel into the grid Windows takes.
//!
//! `SendInput` with `MOUSEEVENTF_ABSOLUTE` does not accept pixels. It
//! accepts a number from 0 to 65535 on each axis, and the rectangle that
//! number is measured against depends on one flag: without
//! `MOUSEEVENTF_VIRTUALDESK` it is the **primary monitor**, and with it,
//! the **virtual desktop** — the bounding box of every monitor, whose
//! origin is the top-left of the primary display and whose coordinates are
//! therefore negative for anything placed above or to the left of it.
//!
//! A session records global physical pixels, which is the virtual desktop's
//! own space, so the whole conversion is this normalization. It lives here
//! rather than in the injector for the reason everything else does: the
//! part that decides where to click must be testable without a screen, and
//! this particular arithmetic has a famous off-by-one worth pinning for
//! every input rather than for a few examples.
//!
//! **The off-by-one.** Pixels span `0..=(dimension − 1)`, so the divisor is
//! `dimension − 1`, not `dimension`. Dividing by the full width leaves the
//! rightmost column and bottom row unreachable and every other pixel
//! fractionally short — a drift that grows with distance from the origin
//! and is invisible on a small screen. Rounding rather than truncating is
//! the other half of the same rule.

use serde::Serialize;

/// The largest value either axis of an absolute mouse event may carry.
/// Windows' own constant, and the reason the arithmetic below needs 64-bit
/// intermediates: `65535 × 65535` does not fit in an `i32`.
const FULL_SCALE: i64 = 65535;

/// The bounding box of every monitor attached to this machine, as Windows
/// reports it — `SM_XVIRTUALSCREEN`, `SM_YVIRTUALSCREEN`,
/// `SM_CXVIRTUALSCREEN`, `SM_CYVIRTUALSCREEN`.
///
/// The origin is the primary monitor's top-left, so `x` and `y` are
/// **negative** whenever a monitor sits above or to the left of it. That is
/// the normal case for a left-hand secondary display, not an error, and it
/// is the arrangement most likely to be mis-clicked by code that assumes a
/// desktop starts at (0, 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct VirtualDesktop {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl VirtualDesktop {
    /// The rightmost and bottom-most pixel that exists — the divisor, and
    /// the coordinate the off-by-one otherwise makes unreachable.
    fn last_pixel(self) -> (i64, i64) {
        (i64::from(self.width) - 1, i64::from(self.height) - 1)
    }

    /// Whether a global physical point is on this desktop at all.
    fn contains(self, x: i32, y: i32) -> bool {
        let right = i64::from(self.x) + i64::from(self.width);
        let bottom = i64::from(self.y) + i64::from(self.height);
        i64::from(x) >= i64::from(self.x)
            && i64::from(y) >= i64::from(self.y)
            && i64::from(x) < right
            && i64::from(y) < bottom
    }
}

/// Why a point could not be expressed as an absolute mouse event.
///
/// Both variants are refusals. Windows clamps an out-of-range absolute
/// coordinate to the edge of the desktop and clicks there, which is the one
/// outcome this tool exists to prevent — a click that lands somewhere
/// plausible and wrong is worse than a run that stops.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NormalizeError {
    /// The point is not on this machine's desktop. The usual cause is a
    /// session captured on a different machine, or on this one before a
    /// monitor was unplugged or rearranged.
    Outside {
        x: i32,
        y: i32,
        desktop: VirtualDesktop,
    },
    /// Windows reported a desktop with no pixels in it, which happens when
    /// there is no attached display — a headless server, or an RDP session
    /// that has been disconnected rather than logged out.
    Empty { desktop: VirtualDesktop },
}

impl std::fmt::Display for NormalizeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Outside { x, y, desktop } => write!(
                f,
                "({x}, {y}) is not a point on this machine's desktop, which spans \
                 {} × {} from ({}, {}). Windows would clamp an absolute event to the \
                 nearest edge and click there, so this is refused instead. Re-mark the \
                 region with pixelcoords on this machine",
                desktop.width, desktop.height, desktop.x, desktop.y
            ),
            Self::Empty { desktop } => write!(
                f,
                "Windows reports a virtual desktop of {} × {}, so there is no screen to \
                 aim at. A disconnected RDP session or a machine with no attached \
                 display looks like this",
                desktop.width, desktop.height
            ),
        }
    }
}

impl std::error::Error for NormalizeError {}

/// Convert a global physical pixel into the absolute pair `SendInput` takes
/// alongside `MOUSEEVENTF_VIRTUALDESK`.
///
/// Rounds to the nearest, and divides by the last pixel rather than the
/// dimension — see the module note. A single-pixel axis (a degenerate
/// display, but expressible) has one reachable coordinate, and it is 0.
pub fn normalize(desktop: VirtualDesktop, x: i32, y: i32) -> Result<(i32, i32), NormalizeError> {
    if desktop.width <= 0 || desktop.height <= 0 {
        return Err(NormalizeError::Empty { desktop });
    }
    if !desktop.contains(x, y) {
        return Err(NormalizeError::Outside { x, y, desktop });
    }
    let (last_x, last_y) = desktop.last_pixel();
    Ok((
        scale(i64::from(x) - i64::from(desktop.x), last_x),
        scale(i64::from(y) - i64::from(desktop.y), last_y),
    ))
}

/// One axis: an offset from the desktop's own origin, over the span of that
/// axis, times full scale — rounded, in 64-bit, because the numerator
/// reaches 4.3 billion on a wide desktop and this workspace panics on
/// overflow rather than wrapping.
fn scale(offset: i64, span: i64) -> i32 {
    if span <= 0 {
        return 0;
    }
    ((offset * FULL_SCALE + span / 2) / span) as i32
}

/// How Windows reads an absolute coordinate back, used to state the
/// round-trip rule as a test rather than as a comment.
///
/// Not `pub`: nothing in the binary needs it, and shipping it would imply
/// the injector should be checking its own arithmetic at runtime.
#[cfg(test)]
fn denormalize(desktop: VirtualDesktop, dx: i32, dy: i32) -> (i32, i32) {
    let (last_x, last_y) = desktop.last_pixel();
    let back = |value: i32, span: i64| {
        if span <= 0 {
            return 0;
        }
        ((i64::from(value) * span + FULL_SCALE / 2) / FULL_SCALE) as i32
    };
    (desktop.x + back(dx, last_x), desktop.y + back(dy, last_y))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 1920×1080 primary with a 1920×1200 secondary to its left, which is
    /// the layout that catches everything: negative origins, a desktop
    /// wider and taller than the primary, and a bottom-right corner that
    /// belongs to neither monitor alone.
    const MIXED: VirtualDesktop = VirtualDesktop {
        x: -1920,
        y: -120,
        width: 3840,
        height: 1200,
    };

    const PRIMARY_ONLY: VirtualDesktop = VirtualDesktop {
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
    };

    /// The corners are the whole point. The origin must be 0 and the last
    /// pixel must be full scale — dividing by the dimension rather than the
    /// dimension minus one yields 65500 here, which is a pixel short of the
    /// edge and unreachable forever.
    #[test]
    fn the_far_corner_is_reachable_at_all() {
        let (x, y) = normalize(PRIMARY_ONLY, 0, 0).expect("the origin is a pixel");
        assert_eq!((x, y), (0, 0));

        let (x, y) = normalize(PRIMARY_ONLY, 1919, 1079).expect("the last pixel is a pixel");
        assert_eq!(
            (x, y),
            (65535, 65535),
            "the rightmost column and bottom row must be addressable"
        );
    }

    /// The negative-origin case, which is what `MOUSEEVENTF_VIRTUALDESK`
    /// exists for. A secondary display placed left of the primary starts at
    /// a negative x, and its top-left is the desktop's 0 — not the
    /// primary's.
    #[test]
    fn a_display_left_of_the_primary_normalizes_from_the_desktop_origin() {
        assert_eq!(
            normalize(MIXED, -1920, -120).expect("the desktop's own corner"),
            (0, 0)
        );
        assert_eq!(
            normalize(MIXED, 1919, 1079).expect("the far corner"),
            (65535, 65535)
        );
        // The primary monitor's own origin is not the desktop's. It sits
        // just past the middle of a desktop twice its width — past, not on,
        // because the divisor is the last pixel rather than the width — and
        // it round-trips. Normalizing against the primary alone, which is
        // what enigo does, would send 0 here and click the far left edge of
        // the secondary display instead.
        let (x, y) = normalize(MIXED, 0, 0).expect("the primary's origin");
        assert_eq!((x, y), (32776, 6559));
        assert_eq!(denormalize(MIXED, x, y), (0, 0));
    }

    /// Every pixel survives the round trip through Windows' reading of the
    /// number. This is the off-by-one and the rounding stated as one rule,
    /// for every pixel on both axes rather than for the corners.
    #[test]
    fn every_pixel_maps_back_to_itself() {
        for desktop in [PRIMARY_ONLY, MIXED] {
            for step in 0..desktop.width {
                let x = desktop.x + step;
                let (dx, dy) = normalize(desktop, x, desktop.y).expect("on the desktop");
                assert_eq!(
                    denormalize(desktop, dx, dy).0,
                    x,
                    "x={x} came back wrong on {desktop:?}"
                );
            }
            for step in 0..desktop.height {
                let y = desktop.y + step;
                let (dx, dy) = normalize(desktop, desktop.x, y).expect("on the desktop");
                assert_eq!(
                    denormalize(desktop, dx, dy).1,
                    y,
                    "y={y} came back wrong on {desktop:?}"
                );
            }
        }
    }

    /// Truncating instead of rounding would show up as a point that maps
    /// back one pixel short. Stated directly, because it is the half of the
    /// rule the `− 1` alone does not fix.
    #[test]
    fn rounding_is_to_the_nearest_not_toward_zero() {
        // 1000 × 65535 / 1919 = 34150.60…, so the nearest is 34151 and
        // truncation would give 34150.
        let (dx, _) = normalize(PRIMARY_ONLY, 1000, 0).expect("on screen");
        assert_eq!(dx, 34151);
        assert_eq!(denormalize(PRIMARY_ONLY, dx, 0).0, 1000);
    }

    /// A point off the desktop is named and refused, never clamped: Windows
    /// would click the nearest edge, which is the failure this tool exists
    /// to prevent.
    #[test]
    fn a_point_off_the_desktop_is_refused_by_name() {
        for (x, y) in [(1920, 0), (0, 1080), (-1, 0), (0, -1)] {
            let error = normalize(PRIMARY_ONLY, x, y).expect_err("off the desktop");
            let message = error.to_string();
            assert!(message.contains(&format!("({x}, {y})")), "{message}");
            assert!(message.contains("clamp"), "says why it refused: {message}");
            assert!(matches!(error, NormalizeError::Outside { .. }));
        }
        // And the same point is fine on a desktop that does contain it.
        assert!(normalize(MIXED, -1, 0).is_ok());
    }

    /// No attached display is a state to report, not a division by zero.
    #[test]
    fn a_desktop_with_no_pixels_is_refused_rather_than_divided_by() {
        for desktop in [
            VirtualDesktop {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            },
            VirtualDesktop {
                x: 0,
                y: 0,
                width: 1920,
                height: 0,
            },
        ] {
            let error = normalize(desktop, 0, 0).expect_err("nothing to aim at");
            assert!(matches!(error, NormalizeError::Empty { .. }));
            assert!(error.to_string().contains("no screen to aim at"));
        }
    }

    /// A single-pixel axis has exactly one coordinate and no span to divide
    /// by. Degenerate, but expressible, and it must not panic.
    #[test]
    fn a_single_pixel_axis_has_one_reachable_coordinate() {
        let sliver = VirtualDesktop {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        };
        assert_eq!(normalize(sliver, 0, 0).expect("the only pixel"), (0, 0));
        assert!(normalize(sliver, 1, 0).is_err());
    }

    /// The intermediate multiplication reaches 4.3 billion, which does not
    /// fit in an `i32`, and this workspace panics on overflow in release as
    /// well as debug. A desktop at the top of the addressable range is the
    /// case that would find it.
    #[test]
    fn a_desktop_at_full_scale_does_not_overflow() {
        let huge = VirtualDesktop {
            x: 0,
            y: 0,
            width: 65536,
            height: 65536,
        };
        assert_eq!(normalize(huge, 0, 0).expect("the origin"), (0, 0));
        assert_eq!(
            normalize(huge, 65535, 65535).expect("the far corner"),
            (65535, 65535)
        );
    }
}
