//! Input synthesis, behind a seam.
//!
//! One trait with two implementations: the real one that moves your
//! mouse, and a recording one that moves nothing. The recorder is why
//! the run loop — ordering, settling, verification, abort — is testable
//! without a screen, which is the same justification `CaptureProvider`
//! has in the sister tool.
//!
//! **Coordinates arriving here are already converted.** `Space::Auto`
//! resolves to logical points on macOS and physical pixels on
//! Windows/X11, which is exactly what enigo's absolute coordinates mean
//! on each. That alignment is not luck — it's why the conversion lives
//! in core and is property-tested.

use anyhow::Result;
use pixelactions_core::flow::Axis;

/// A mouse button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Left,
}

/// What an injector can do. Deliberately small: every action a flow can
/// express, and nothing else.
pub trait Injector {
    /// Move the pointer to an absolute point in the platform's own input
    /// space.
    fn move_to(&mut self, x: f64, y: f64) -> Result<()>;
    fn click(&mut self, button: Button) -> Result<()>;
    fn double_click(&mut self, button: Button) -> Result<()>;
    fn press(&mut self, button: Button) -> Result<()>;
    fn release(&mut self, button: Button) -> Result<()>;
    /// Type literal text through the platform's Unicode path — layout
    /// independent, and unable to express shortcuts.
    fn text(&mut self, text: &str) -> Result<()>;
    /// Press a chord such as `cmd+s`: modifiers held, key tapped,
    /// modifiers released in reverse.
    fn chord(&mut self, chord: &str) -> Result<()>;
    /// Turn the wheel `amount` 15° clicks at the current pointer
    /// position. Positive is down (or right); negative is up (or left).
    ///
    /// Unlike every other method here, the argument is not a coordinate
    /// and has no exact meaning — the distance travelled depends on the
    /// reader's own OS scroll-speed setting.
    fn scroll(&mut self, amount: i32, axis: Axis) -> Result<()>;

    /// Where the cursor is now, in the platform's own input space.
    ///
    /// Read rather than remembered: the point of asking is to notice a
    /// *human* moving the mouse, which is exactly what a remembered
    /// position cannot see.
    fn cursor(&mut self) -> Result<(f64, f64)>;

    /// Prove injection actually works, harmlessly: read the cursor, move
    /// it one pixel, put it back, and confirm the OS agreed.
    ///
    /// This exists because a missing macOS Accessibility grant makes
    /// event posting a silent no-op — the call "succeeds" and nothing
    /// moves. Asking the system where the cursor ended up is the only
    /// honest check, and it is what makes `doctor` able to say "yes"
    /// rather than "probably".
    fn probe(&mut self) -> Result<()>;
}

/// Everything an injector was asked to do, in order. The test double —
/// it is why the run loop's ordering, settling, verification, and abort
/// behavior are testable without a screen.
#[cfg(test)]
#[derive(Debug, Clone, PartialEq)]
pub struct Recording {
    pub events: Vec<String>,
    /// Where this double reports the cursor to be. Settable so the run
    /// loop's kill-switch behavior is testable without a screen.
    pub cursor: (f64, f64),
}

#[cfg(test)]
impl Default for Recording {
    fn default() -> Self {
        Self {
            events: Vec::new(),
            // Deliberately not (0, 0) — that is a screen corner, and it
            // would trip the kill switch in every test that never meant
            // to exercise it.
            cursor: (400.0, 300.0),
        }
    }
}

#[cfg(test)]
impl Injector for Recording {
    fn move_to(&mut self, x: f64, y: f64) -> Result<()> {
        self.events.push(format!("move {x:.0},{y:.0}"));
        Ok(())
    }
    fn click(&mut self, _button: Button) -> Result<()> {
        self.events.push("click".into());
        Ok(())
    }
    fn double_click(&mut self, _button: Button) -> Result<()> {
        self.events.push("double_click".into());
        Ok(())
    }
    fn press(&mut self, _button: Button) -> Result<()> {
        self.events.push("press".into());
        Ok(())
    }
    fn release(&mut self, _button: Button) -> Result<()> {
        self.events.push("release".into());
        Ok(())
    }
    fn text(&mut self, text: &str) -> Result<()> {
        self.events.push(format!("text {text}"));
        Ok(())
    }
    fn chord(&mut self, chord: &str) -> Result<()> {
        self.events.push(format!("chord {chord}"));
        Ok(())
    }
    fn scroll(&mut self, amount: i32, axis: Axis) -> Result<()> {
        let way = match axis {
            Axis::Vertical => "v",
            Axis::Horizontal => "h",
        };
        self.events.push(format!("scroll {way}{amount}"));
        Ok(())
    }
    fn cursor(&mut self) -> Result<(f64, f64)> {
        Ok(self.cursor)
    }
    fn probe(&mut self) -> Result<()> {
        self.events.push("probe".into());
        Ok(())
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub use platform::RealInjector;

/// Whether this build can synthesize input in the session it is running
/// in, and if not, why not in terms the reader can act on.
///
/// A runtime question on Linux, where the same binary faces X11 or
/// Wayland depending on the login session, so it cannot be a `cfg`.
#[cfg(target_os = "linux")]
pub fn availability() -> Result<(), String> {
    use pixelactions_core::display::{Server, detect};

    let server = detect(
        std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
        std::env::var("WAYLAND_DISPLAY").ok().as_deref(),
        std::env::var("DISPLAY").ok().as_deref(),
    );
    match server {
        Server::Wayland => {}
        Server::X11 => {
            return Err(
                "this is an X11 session, and this build only synthesizes input on Wayland. \
                 X11 support is tracked separately; `plan` works everywhere"
                    .to_string(),
            );
        }
        Server::Unknown => {
            return Err(
                "no desktop session was found — neither XDG_SESSION_TYPE, WAYLAND_DISPLAY \
                 nor DISPLAY names one. Synthesizing input needs a compositor to send it \
                 to; `plan` works without one"
                    .to_string(),
            );
        }
    }
    let capabilities = crate::portal::capabilities()
        .map_err(|error| format!("cannot ask the desktop portal what it supports: {error:#}"))?;
    if !capabilities.usable() {
        return Err(format!(
            "this compositor cannot grant input: the portal offers RemoteDesktop version {} \
             (needs 2 or newer for ConnectToEIS), device types {:#b} (needs keyboard and \
             pointer), ScreenCast version {}. GNOME and KDE implement this; wlroots \
             compositors do not yet",
            capabilities.remote_desktop_version,
            capabilities.device_types,
            capabilities.screen_cast_version
        ));
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn availability() -> Result<(), String> {
    if cfg!(target_os = "macos") {
        return Ok(());
    }
    Err("input synthesis is not implemented for this platform yet — `plan` works everywhere".into())
}

/// Wayland: the portal grants, EIS carries, and the coordinate space is a
/// region the compositor chose.
///
/// The structure differs from every other platform for one reason worth
/// stating: this injector must hold state. Elsewhere a coordinate is
/// absolute in a space known at compile time, so an injector is
/// stateless. Here the space is a **granted region**, learned at runtime,
/// so the monitors and the grant have to live alongside the socket — and
/// the grant must outlive every event sent through it, or the compositor
/// revokes the session mid-run.
#[cfg(target_os = "linux")]
mod platform {
    use anyhow::{Result, anyhow, bail};
    use pixelactions_core::flow::Axis;
    use pixelactions_core::stream::place;
    use pixelcoords_core::session::MonitorRecord;

    use super::{Button, Injector};
    use crate::{eis, portal};

    pub struct RealInjector {
        sender: eis::Sender,
        monitors: Vec<MonitorRecord>,
        /// Never read, never dropped early. The portal ties the session's
        /// life to this handle; releasing it kills the EIS socket.
        _grant: portal::Grant,
    }

    impl RealInjector {
        /// Negotiate consent and connect. The monitors come from the
        /// session because the physical pixels a flow resolves to mean
        /// nothing without them.
        pub fn new(monitors: &[MonitorRecord]) -> Result<Self> {
            let mut grant = portal::grant()?;
            let sender = eis::Sender::connect(grant.take_socket()?)?;
            Ok(Self {
                sender,
                monitors: monitors.to_vec(),
                _grant: grant,
            })
        }

        /// Whether typing is possible in this grant.
        pub fn can_type(&self) -> bool {
            self.sender.can_type()
        }

        /// How the compositor described the area it will accept
        /// coordinates in — for `doctor`, which reports it rather than
        /// making the reader guess why a click was refused.
        pub fn regions(&self) -> &[pixelactions_core::stream::Region] {
            self.sender.regions()
        }
    }

    impl Injector for RealInjector {
        /// Takes physical pixels, like every other platform on Linux, and
        /// does the last hop here because only this layer knows the
        /// granted regions. The arithmetic itself is in core, tested.
        fn move_to(&mut self, x: f64, y: f64) -> Result<()> {
            let placement = place(
                &self.monitors,
                self.sender.regions(),
                x.round() as i32,
                y.round() as i32,
            )
            .map_err(|error| anyhow!("cannot place the pointer: {error}"))?;
            self.sender.move_to(placement)
        }

        fn click(&mut self, button: Button) -> Result<()> {
            self.press(button)?;
            self.release(button)
        }

        fn double_click(&mut self, button: Button) -> Result<()> {
            self.click(button)?;
            // The compositor decides what counts as a double-click by
            // timing, same as macOS; a short gap keeps both inside its
            // window without relying on one frame carrying both.
            std::thread::sleep(std::time::Duration::from_millis(40));
            self.click(button)
        }

        fn press(&mut self, button: Button) -> Result<()> {
            match button {
                Button::Left => self.sender.button(true),
            }
        }

        fn release(&mut self, button: Button) -> Result<()> {
            match button {
                Button::Left => self.sender.button(false),
            }
        }

        fn text(&mut self, text: &str) -> Result<()> {
            self.sender.text(text)
        }

        fn chord(&mut self, chord: &str) -> Result<()> {
            self.sender.chord(chord)
        }

        fn scroll(&mut self, amount: i32, axis: Axis) -> Result<()> {
            self.sender.scroll(amount, axis)
        }

        /// Wayland has no "where is the pointer" query, by design — the
        /// isolation that makes injection require consent also means no
        /// client may ask where another client's pointer is.
        ///
        /// This returns an error rather than a plausible number on
        /// purpose. The caller is the kill switch, and its contract is
        /// that a cursor it cannot read fails the step naming
        /// `failsafe = false`. A stubbed (0, 0) would sit in a screen
        /// corner and abort every run; any other stub would silently
        /// disable the check. Reporting the truth lets the existing guard
        /// do the right thing.
        ///
        /// Lifting this needs the screencast stream's cursor metadata,
        /// which is a `PipeWire` connection this build does not open.
        fn cursor(&mut self) -> Result<(f64, f64)> {
            bail!(
                "Wayland exposes no way to ask where the pointer is — the same isolation \
                 that makes input injection require your consent also hides the pointer \
                 from other programs. The corner kill switch therefore has nothing to \
                 watch on this platform. Set failsafe = false in the flow to run without \
                 it, deliberately"
            )
        }

        /// Prove the grant, honestly.
        ///
        /// Reaching here at all means the portal granted a session, the
        /// compositor offered a pointer that takes coordinates, and it
        /// gave that pointer a region — which is everything that can be
        /// established without a way to read the cursor back. It is
        /// deliberately *not* the one-pixel move macOS does, because with
        /// no cursor query there would be nothing to compare against, and
        /// "it did not error" is not "it moved".
        fn probe(&mut self) -> Result<()> {
            if self.sender.regions().is_empty() {
                bail!("the compositor granted input but described no region to aim in");
            }
            Ok(())
        }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use anyhow::{Context, Result, anyhow};
    use enigo::{
        Axis as EnigoAxis, Button as EnigoButton, Coordinate, Direction, Enigo, Key, Keyboard,
        Mouse, Settings,
    };
    use pixelactions_core::flow::Axis;

    use super::{Button, Injector};

    /// The real thing: enigo over Core Graphics.
    ///
    /// macOS requires an Accessibility grant to post synthetic events.
    /// Without it, `CGEventPost` silently does nothing — so construction
    /// failing loudly here is better than a run that reports success and
    /// moves no mouse.
    pub struct RealInjector {
        enigo: Enigo,
    }

    impl RealInjector {
        pub fn new() -> Result<Self> {
            let enigo = Enigo::new(&Settings::default()).map_err(|e| {
                anyhow!(
                    "cannot synthesize input: {e}. On macOS this usually means the \
                     Accessibility permission is missing — grant it under System Settings \
                     > Privacy & Security > Accessibility for the terminal running \
                     pixelactions, then try again"
                )
            })?;
            Ok(Self { enigo })
        }
    }

    fn to_enigo(button: Button) -> EnigoButton {
        match button {
            Button::Left => EnigoButton::Left,
        }
    }

    /// Map a chord token to an enigo key. Modifiers are named the way a
    /// human writes them; anything else is a single character.
    fn key_for(token: &str) -> Result<Key> {
        let key = match token.to_ascii_lowercase().as_str() {
            "cmd" | "command" | "meta" | "super" => Key::Meta,
            "ctrl" | "control" => Key::Control,
            "alt" | "option" | "opt" => Key::Alt,
            "shift" => Key::Shift,
            "tab" => Key::Tab,
            "enter" | "return" => Key::Return,
            "esc" | "escape" => Key::Escape,
            "space" => Key::Space,
            "backspace" | "delete" => Key::Backspace,
            "up" => Key::UpArrow,
            "down" => Key::DownArrow,
            "left" => Key::LeftArrow,
            "right" => Key::RightArrow,
            other => {
                let mut chars = other.chars();
                let first = chars.next().context("empty key in chord")?;
                if chars.next().is_some() {
                    return Err(anyhow!(
                        "unknown key {other:?} in chord — use a single character or a \
                         named key (cmd, ctrl, alt, shift, tab, enter, esc, space, \
                         backspace, arrows)"
                    ));
                }
                Key::Unicode(first)
            }
        };
        Ok(key)
    }

    impl Injector for RealInjector {
        fn move_to(&mut self, x: f64, y: f64) -> Result<()> {
            self.enigo
                .move_mouse(x as i32, y as i32, Coordinate::Abs)
                .map_err(|e| anyhow!("move to ({x:.0}, {y:.0}) failed: {e}"))
        }

        fn click(&mut self, button: Button) -> Result<()> {
            self.enigo
                .button(to_enigo(button), Direction::Click)
                .map_err(|e| anyhow!("click failed: {e}"))
        }

        fn double_click(&mut self, button: Button) -> Result<()> {
            self.click(button)?;
            // The OS decides what counts as a double-click by timing; a
            // short gap keeps the two clicks inside that window without
            // relying on posting them in the same instant.
            std::thread::sleep(std::time::Duration::from_millis(40));
            self.click(button)
        }

        fn press(&mut self, button: Button) -> Result<()> {
            self.enigo
                .button(to_enigo(button), Direction::Press)
                .map_err(|e| anyhow!("press failed: {e}"))
        }

        fn release(&mut self, button: Button) -> Result<()> {
            self.enigo
                .button(to_enigo(button), Direction::Release)
                .map_err(|e| anyhow!("release failed: {e}"))
        }

        fn text(&mut self, text: &str) -> Result<()> {
            self.enigo
                .text(text)
                .map_err(|e| anyhow!("typing failed: {e}"))
        }

        fn scroll(&mut self, amount: i32, axis: Axis) -> Result<()> {
            let axis = match axis {
                Axis::Vertical => EnigoAxis::Vertical,
                Axis::Horizontal => EnigoAxis::Horizontal,
            };
            self.enigo
                .scroll(amount, axis)
                .map_err(|e| anyhow!("cannot scroll: {e}"))
        }

        fn cursor(&mut self) -> Result<(f64, f64)> {
            let (x, y) = self
                .enigo
                .location()
                .map_err(|e| anyhow!("cannot read the cursor position: {e}"))?;
            Ok((f64::from(x), f64::from(y)))
        }

        fn probe(&mut self) -> Result<()> {
            let (x, y) = self
                .enigo
                .location()
                .map_err(|e| anyhow!("cannot read the cursor position: {e}"))?;
            // One pixel, then straight back. Small enough to be invisible,
            // real enough that the OS either honors it or does not.
            let target = (x + 1, y);
            self.enigo
                .move_mouse(target.0, target.1, Coordinate::Abs)
                .map_err(|e| anyhow!("cannot move the cursor: {e}"))?;
            std::thread::sleep(std::time::Duration::from_millis(60));
            let after = self
                .enigo
                .location()
                .map_err(|e| anyhow!("cannot read the cursor position: {e}"))?;
            let _ = self.enigo.move_mouse(x, y, Coordinate::Abs);

            if after == (x, y) {
                return Err(anyhow!(
                    "the cursor did not move — macOS accepted the event and discarded it, \
                     which is what happens without the Accessibility permission. Grant it \
                     under System Settings > Privacy & Security > Accessibility for the \
                     application running pixelactions (your terminal, if you launched it \
                     from one), then quit and reopen that application"
                ));
            }
            Ok(())
        }

        fn chord(&mut self, chord: &str) -> Result<()> {
            let (modifiers, key) = pixelactions_core::chord::split(chord)?;
            let mut held = Vec::new();
            for token in &modifiers {
                let modifier = key_for(token)?;
                self.enigo
                    .key(modifier, Direction::Press)
                    .map_err(|e| anyhow!("holding {token} failed: {e}"))?;
                held.push(modifier);
            }
            let result = self
                .enigo
                .key(key_for(key)?, Direction::Click)
                .map_err(|e| anyhow!("pressing {key} failed: {e}"));
            // Release in reverse whatever happened to the key press — a
            // stuck modifier is worse than a failed chord.
            for modifier in held.into_iter().rev() {
                let _ = self.enigo.key(modifier, Direction::Release);
            }
            result
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_recording_injector_captures_order() {
        let mut injector = Recording::default();
        injector.move_to(10.4, 20.6).expect("recorded");
        injector.click(Button::Left).expect("recorded");
        injector.text("hi").expect("recorded");
        assert_eq!(injector.events, vec!["move 10,21", "click", "text hi"]);
    }
}
