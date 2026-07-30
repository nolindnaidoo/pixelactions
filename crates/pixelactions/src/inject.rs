//! Input synthesis, behind a seam.
//!
//! One trait, a recording implementation that moves nothing, and one real
//! implementation per input path. The recorder is why the run loop —
//! ordering, settling, verification, abort — is testable without a screen,
//! which is the same justification `CaptureProvider` has in the sister
//! tool.
//!
//! Linux is the platform with two real implementations rather than one,
//! because the display server is a runtime fact there: [`X11Injector`]
//! speaks XTEST, [`WaylandInjector`] speaks the portal and EIS, and
//! [`session_server`] decides which the machine is actually running.
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

#[cfg(target_os = "macos")]
pub use platform::RealInjector;

/// Windows names its injector rather than calling it `RealInjector`, for
/// the same reason Linux names its two: the type says which input path it
/// speaks, and this one is not the enigo-only path macOS takes.
#[cfg(target_os = "windows")]
pub use win32::WindowsInjector;

/// Linux has two real injectors because it has two display servers, and
/// which one a machine runs is a runtime fact rather than a build-time one.
/// They are named rather than hidden behind one `RealInjector` so a call
/// site has to say which path it means.
#[cfg(target_os = "linux")]
pub use wayland::WaylandInjector;
#[cfg(target_os = "linux")]
pub use x11::X11Injector;

/// Which display server this session is running.
///
/// One place reads the environment, so `availability`, `doctor` and the
/// injector chosen for a run can never disagree about what they are looking
/// at. The decision itself is in core, where it is tested against every
/// session shape.
#[cfg(target_os = "linux")]
pub fn session_server() -> pixelactions_core::display::Server {
    pixelactions_core::display::detect(
        std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
        std::env::var("WAYLAND_DISPLAY").ok().as_deref(),
        std::env::var("DISPLAY").ok().as_deref(),
    )
}

/// Whether this build can synthesize input in the session it is running
/// in, and if not, why not in terms the reader can act on.
///
/// A runtime question on Linux, where the same binary faces X11 or
/// Wayland depending on the login session, so it cannot be a `cfg`.
#[cfg(target_os = "linux")]
pub fn availability() -> Result<(), String> {
    use pixelactions_core::display::Server;

    match session_server() {
        Server::Wayland => {}
        // X11 has no permission model to check — any client may inject into
        // any other, which is precisely the hole Wayland closes. So the only
        // question is whether the display answers, and the honest way to
        // find out is to connect: naming a session `x11` proves nothing
        // about a server being on the other end of `DISPLAY`. Cheap enough
        // to ask (a local socket, no prompt, nothing granted), and asking
        // here is what makes a dead display a refusal — exit 3 with the
        // reason — rather than a run that claims support and then fails
        // while building the injector.
        Server::X11 => {
            return X11Injector::new()
                .map(|_| ())
                .map_err(|error| format!("{error:#}"));
        }
        Server::Unknown => {
            return Err(
                "no desktop session was found — neither XDG_SESSION_TYPE, WAYLAND_DISPLAY \
                 nor DISPLAY names one. Synthesizing input needs a display server to send \
                 it to; `plan` works without one"
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

/// Off Linux the answer is a build-time fact: there is one windowing system
/// per platform, and no session to inspect. Windows joins macOS here — it
/// has no grant to check, so a build that compiled the injector can use it,
/// and anything that will actually stop an event (UIPI) is discovered when
/// the event is sent, not before.
#[cfg(not(target_os = "linux"))]
pub fn availability() -> Result<(), String> {
    if cfg!(any(target_os = "macos", target_os = "windows")) {
        return Ok(());
    }
    Err("input synthesis is not implemented for this platform yet — `plan` works everywhere".into())
}

/// Chord tokens → enigo keys, shared by every platform whose keyboard is
/// enigo's: macOS, X11 and Windows.
///
/// One table, because a chord string is a portability promise. `cmd+s`
/// written on a Mac has to press Super+s on Linux and Win+s on Windows, and
/// it does — `Key::Meta` is `Super_L` on X11 and `VK_LWIN` on Windows. A
/// second copy of this table per platform is how that promise quietly stops
/// being true, which is why Windows joined this one rather than growing its
/// own.
///
/// Wayland cannot share it: an EI keyboard is addressed by keysym against
/// the compositor's own keymap, not by enigo key, so `eis` carries the
/// parallel table. Both are checked against
/// [`pixelactions_core::chord::NAMED_KEYS`], which is what keeps them from
/// drifting.
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
mod keys {
    use anyhow::{Context, Result, anyhow};
    use enigo::Key;
    use pixelactions_core::chord::NAMED_KEYS;

    /// Map one chord token to an enigo key. Modifiers are named the way a
    /// human writes them; anything else is a single character.
    pub fn key_for(token: &str) -> Result<Key> {
        let key = match token.to_ascii_lowercase().as_str() {
            // Super is the Linux name for what a Mac calls cmd. The aliases
            // exist so one flow file runs on both.
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
                        "unknown key {other:?} in chord — use a single character or one of: {}",
                        NAMED_KEYS.join(", ")
                    ));
                }
                Key::Unicode(first)
            }
        };
        Ok(key)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// The names in a chord are the same on every platform, which is
        /// the whole point of writing them out rather than taking keycodes.
        #[test]
        fn modifier_names_map_the_way_a_human_writes_them() {
            for name in ["cmd", "command", "meta", "super", "SUPER"] {
                assert_eq!(key_for(name).expect(name), Key::Meta, "{name}");
            }
            for name in ["ctrl", "control"] {
                assert_eq!(key_for(name).expect(name), Key::Control, "{name}");
            }
            for name in ["alt", "option", "opt"] {
                assert_eq!(key_for(name).expect(name), Key::Alt, "{name}");
            }
        }

        /// Every name core promises a flow author, answered here. A name
        /// listed there and missing from this table is a chord that parses
        /// and then fails at the injector, which is the worst place to find
        /// out.
        #[test]
        fn every_promised_name_resolves() {
            for name in NAMED_KEYS {
                let key = key_for(name).unwrap_or_else(|error| panic!("{name}: {error}"));
                assert_ne!(
                    key,
                    Key::Unicode(name.chars().next().expect("non-empty")),
                    "{name} fell through to its first character instead of naming a key"
                );
            }
        }

        #[test]
        fn a_single_character_becomes_itself() {
            assert_eq!(key_for("s").expect("s"), Key::Unicode('s'));
            assert_eq!(key_for("7").expect("7"), Key::Unicode('7'));
            assert_eq!(key_for("ü").expect("ü"), Key::Unicode('ü'));
        }

        /// A multi-character token that is not a known name is refused
        /// rather than silently becoming its first letter.
        #[test]
        fn an_unknown_multi_character_key_is_refused_and_says_what_is_allowed() {
            let error = key_for("fnord").expect_err("not a key");
            let message = error.to_string();
            assert!(message.contains("fnord"), "{message}");
            assert!(message.contains("shift"), "lists the names: {message}");
            assert!(key_for("").is_err());
        }
    }
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
mod wayland {
    use anyhow::{Result, anyhow, bail};
    use pixelactions_core::flow::Axis;
    use pixelactions_core::stream::place;
    use pixelcoords_core::session::MonitorRecord;

    use super::{Button, Injector};
    use crate::{eis, portal};

    pub struct WaylandInjector {
        sender: eis::Sender,
        monitors: Vec<MonitorRecord>,
        /// Never read, never dropped early. The portal ties the session's
        /// life to this handle; releasing it kills the EIS socket.
        _grant: portal::Grant,
    }

    impl WaylandInjector {
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

    impl Injector for WaylandInjector {
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

/// X11: XTEST against the root window, in the pixels the session already
/// speaks.
///
/// The platform with the least between a coordinate and a click. XTEST
/// takes root-window coordinates — one space covering every output,
/// origin at the top-left of the whole X screen — which is exactly what
/// `convert::native_space()` already resolves `Space::Auto` to here, so
/// there is no conversion at this layer and no state to hold. With `XRandR`,
/// several monitors are one screen, so the session's global `origin_px`
/// layout maps straight through.
///
/// Two things X11 has that Wayland does not, and both matter:
///
/// - **The pointer position can be read**, so the corner kill switch works
///   and the probe is a real proof rather than an acceptance.
/// - **Typing is layout-independent.** A character the active keymap
///   cannot reach is typed by binding it to a spare keycode for the
///   keystroke and unbinding it after — enigo's remap path. An EI keyboard
///   on Wayland is welded to the compositor's keymap and cannot do this.
///
/// What X11 has that nothing else does: **no permission model at all.**
/// Any client may inject into any other, so there is nothing to grant and
/// nothing to check. `doctor` says so rather than implying a guard exists.
#[cfg(target_os = "linux")]
mod x11 {
    use anyhow::{Result, anyhow, bail};
    use enigo::{
        Axis as EnigoAxis, Button as EnigoButton, Coordinate, Direction, Enigo, Keyboard, Mouse,
        Settings,
    };
    use pixelactions_core::flow::Axis;

    use super::{Button, Injector, keys::key_for};

    /// How long to wait before asking the server where the pointer ended
    /// up. `XSync` only proves the server *processed* the fake event; the
    /// pointer position it then reports is what proves it acted on it.
    const PROBE_SETTLE: std::time::Duration = std::time::Duration::from_millis(40);

    pub struct X11Injector {
        enigo: Enigo,
    }

    impl X11Injector {
        /// Connect to `$DISPLAY`.
        ///
        /// Nothing is granted and nothing prompts — this either reaches an
        /// X server or it does not, and a failure here is environmental.
        pub fn new() -> Result<Self> {
            let display = std::env::var("DISPLAY").unwrap_or_default();
            let enigo = Enigo::new(&Settings::default()).map_err(|error| {
                anyhow!(
                    "cannot connect to the X server at DISPLAY={display:?}: {error}. \
                     Check that DISPLAY names the session you meant, that the server is \
                     running, and that this user is allowed to connect to it (`xhost` \
                     restrictions and a different user's session are the usual causes)"
                )
            })?;
            Ok(Self { enigo })
        }

        /// Where the server says the pointer is, in root-window pixels.
        fn location(&mut self) -> Result<(i32, i32)> {
            self.enigo
                .location()
                .map_err(|error| anyhow!("cannot read the cursor position: {error}"))
        }

        /// One harmless pixel sideways, reported as whether the server
        /// honored it. Put back either way.
        fn nudge(&mut self, from: (i32, i32), step: i32) -> Result<bool> {
            self.enigo
                .move_mouse(from.0 + step, from.1, Coordinate::Abs)
                .map_err(|error| anyhow!("cannot move the cursor: {error}"))?;
            std::thread::sleep(PROBE_SETTLE);
            let after = self.location()?;
            let _ = self.enigo.move_mouse(from.0, from.1, Coordinate::Abs);
            Ok(after != from)
        }
    }

    fn to_enigo(button: Button) -> EnigoButton {
        match button {
            Button::Left => EnigoButton::Left,
        }
    }

    /// A resolved point as XTEST root coordinates, or why it is not one.
    ///
    /// Root-window coordinates start at (0, 0) and span every output, so a
    /// negative one cannot be expressed at all. Refusing it by name is the
    /// point: the alternative is a coordinate the server clamps to a screen
    /// corner and clicks there, which is the one outcome this tool exists
    /// to prevent. The upper bound is enigo's — root coordinates are `i16`,
    /// which no real display approaches.
    ///
    /// A negative point reaching here means the session describes a layout
    /// this display server does not have, which is what a session captured
    /// on another platform looks like.
    fn root_point(x: f64, y: f64) -> Result<(i32, i32)> {
        let (px, py) = (x.round() as i32, y.round() as i32);
        if px < 0 || py < 0 {
            bail!(
                "({x:.0}, {y:.0}) is not a point on this X screen: XTEST addresses the root \
                 window, whose coordinates start at (0, 0) and span every output, so a \
                 negative one cannot be expressed. Re-mark the region with pixelcoords on \
                 this session"
            );
        }
        Ok((px, py))
    }

    impl Injector for X11Injector {
        /// Takes global physical pixels and passes them to XTEST unchanged.
        fn move_to(&mut self, x: f64, y: f64) -> Result<()> {
            let (px, py) = root_point(x, y)?;
            self.enigo
                .move_mouse(px, py, Coordinate::Abs)
                .map_err(|error| anyhow!("move to ({px}, {py}) failed: {error}"))
        }

        fn click(&mut self, button: Button) -> Result<()> {
            self.enigo
                .button(to_enigo(button), Direction::Click)
                .map_err(|error| anyhow!("click failed: {error}"))
        }

        fn double_click(&mut self, button: Button) -> Result<()> {
            self.click(button)?;
            // The application decides what counts as a double-click by
            // timing, same as everywhere else; a short gap keeps both
            // inside its window.
            std::thread::sleep(std::time::Duration::from_millis(40));
            self.click(button)
        }

        fn press(&mut self, button: Button) -> Result<()> {
            self.enigo
                .button(to_enigo(button), Direction::Press)
                .map_err(|error| anyhow!("press failed: {error}"))
        }

        fn release(&mut self, button: Button) -> Result<()> {
            self.enigo
                .button(to_enigo(button), Direction::Release)
                .map_err(|error| anyhow!("release failed: {error}"))
        }

        /// Type through the remap path, so the text does not have to be on
        /// the active layout.
        ///
        /// X11 has no Unicode-text event, so this is one keystroke per
        /// character; a character the keymap cannot reach gets a spare
        /// keycode bound to it for that keystroke, and the binding is
        /// reverted when the connection drops. That is what makes `é` on a
        /// US layout type `é` rather than nothing.
        fn text(&mut self, text: &str) -> Result<()> {
            self.enigo
                .text(text)
                .map_err(|error| anyhow!("typing failed: {error}"))
        }

        fn chord(&mut self, chord: &str) -> Result<()> {
            let (modifiers, key) = pixelactions_core::chord::split(chord)?;
            let mut held = Vec::new();
            for token in &modifiers {
                let modifier = key_for(token)?;
                self.enigo
                    .key(modifier, Direction::Press)
                    .map_err(|error| anyhow!("holding {token} failed: {error}"))?;
                held.push(modifier);
            }
            let result = self
                .enigo
                .key(key_for(key)?, Direction::Click)
                .map_err(|error| anyhow!("pressing {key} failed: {error}"));
            // Release in reverse whatever happened to the key press — a
            // stuck modifier is worse than a failed chord.
            for modifier in held.into_iter().rev() {
                let _ = self.enigo.key(modifier, Direction::Release);
            }
            result
        }

        /// Wheel clicks land wherever the pointer is, as buttons 4–7.
        fn scroll(&mut self, amount: i32, axis: Axis) -> Result<()> {
            let axis = match axis {
                Axis::Vertical => EnigoAxis::Vertical,
                Axis::Horizontal => EnigoAxis::Horizontal,
            };
            self.enigo
                .scroll(amount, axis)
                .map_err(|error| anyhow!("cannot scroll: {error}"))
        }

        /// X11 will say where the pointer is, which is what makes the
        /// corner kill switch work on this platform and not on Wayland.
        fn cursor(&mut self) -> Result<(f64, f64)> {
            let (x, y) = self.location()?;
            Ok((f64::from(x), f64::from(y)))
        }

        /// Move one pixel and ask the server whether it happened.
        ///
        /// Both directions are tried because X11 clamps a move to the
        /// screen: a pointer already parked on the right edge cannot go
        /// further right, and reporting that as a failure would blame the
        /// tool for where the mouse happened to be sitting. Only a pointer
        /// that moves neither way is a real refusal — a pointer grab, or a
        /// server built without XTEST.
        fn probe(&mut self) -> Result<()> {
            let from = self.location()?;
            for step in [1, -1] {
                if self.nudge(from, step)? {
                    return Ok(());
                }
            }
            Err(anyhow!(
                "the cursor did not move: the X server processed the XTEST event and the \
                 pointer stayed at ({}, {}). Something holds a pointer grab, or this server \
                 was built without the XTEST extension",
                from.0,
                from.1
            ))
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// The session speaks in whole pixels but conversion produces
        /// fractions, and XTEST takes integers — so the rounding happens
        /// here, once, rather than in each call site's cast.
        #[test]
        fn a_fractional_point_rounds_to_the_nearest_pixel() {
            assert_eq!(root_point(10.4, 20.6).expect("on screen"), (10, 21));
            assert_eq!(root_point(0.0, 0.0).expect("the origin is a pixel"), (0, 0));
            // Rounds toward zero's neighbour, not through it: 0.4 must not
            // become -0 and then trip the guard below.
            assert_eq!(root_point(0.4, 0.4).expect("on screen"), (0, 0));
        }

        /// The refusal that matters. A negative coordinate is what a session
        /// from another platform's layout looks like, and X11 would clamp it
        /// to a corner and click there.
        #[test]
        fn a_negative_point_is_refused_by_name_not_clamped() {
            for (x, y) in [(-1.0, 100.0), (100.0, -1.0), (-1920.0, -1080.0)] {
                let error = root_point(x, y).expect_err("off the root window");
                let message = error.to_string();
                assert!(message.contains("root window"), "{message}");
                assert!(
                    message.contains(&format!("({x:.0}, {y:.0})")),
                    "names the point it refused: {message}"
                );
            }
        }
    }
}

/// Windows: `SendInput` across the whole virtual desktop, with enigo's
/// keyboard.
///
/// **A split implementation, deliberately.** enigo owns everything that
/// carries no coordinate — buttons, the wheel, `KEYEVENTF_UNICODE` text,
/// and virtual-key chords with the extended-key flag that arrows and
/// right-hand modifiers need. Absolute pointer motion is the one thing it
/// gets wrong here: 0.6.1 normalizes against `SM_CXSCREEN`/`SM_CYSCREEN` —
/// the primary monitor — and never sets `MOUSEEVENTF_VIRTUALDESK`, so a
/// coordinate on any other display silently lands on the primary one. That
/// path is written out in [`crate::win`] instead, over the arithmetic in
/// [`pixelactions_core::virtualdesk`].
///
/// Two things Windows shares with X11 and not with Wayland: the pointer
/// position can be **read**, so the corner kill switch is armed and the
/// probe is a real proof; and typing is **layout-independent**, because
/// `KEYEVENTF_UNICODE` carries the character itself rather than a key that
/// happens to produce it.
///
/// What Windows has that nothing else does: **UIPI**. A process at medium
/// integrity cannot send input to a window at high integrity, to the UAC
/// dialog, or to the login screen. `SendInput` reports that by accepting
/// fewer events than it was given, which is the one failure this module
/// translates into a sentence naming the cause.
#[cfg(target_os = "windows")]
mod win32 {
    use anyhow::{Result, anyhow, bail};
    use enigo::{
        Axis as EnigoAxis, Button as EnigoButton, Direction, Enigo, Keyboard, Mouse, Settings,
    };
    use pixelactions_core::flow::Axis;
    use pixelactions_core::virtualdesk::normalize;

    use super::{Button, Injector, keys::key_for};
    use crate::win;

    /// How long to wait before asking Windows where the pointer ended up.
    /// `SendInput` queues onto the input thread's message loop; the call
    /// returning proves it was queued, and only the read-back proves it was
    /// acted on.
    const PROBE_SETTLE: std::time::Duration = std::time::Duration::from_millis(40);

    pub struct WindowsInjector {
        /// Keyboard, buttons and wheel. The pointer does not go through
        /// here — see the module note.
        enigo: Enigo,
    }

    impl WindowsInjector {
        /// Construct, and mean it.
        ///
        /// There is no Accessibility grant on Windows and nothing to
        /// prompt for, so unlike macOS a failure here is not a missing
        /// permission — it is a session with no window station to talk to,
        /// which is what a service account or a disconnected RDP session
        /// looks like.
        pub fn new() -> Result<Self> {
            let enigo = Enigo::new(&Settings::default()).map_err(|error| {
                anyhow!(
                    "cannot synthesize input: {error}. Windows asks for no permission to \
                     do this, so a failure here means there is no interactive desktop to \
                     send to — a service, a scheduled task with \"run whether the user is \
                     logged on or not\", or an RDP session that has been disconnected \
                     rather than logged out"
                )
            })?;
            Ok(Self { enigo })
        }

        /// One harmless pixel sideways, reported as whether Windows honored
        /// it. Put back either way.
        fn nudge(from: (i32, i32), step: i32) -> Result<bool> {
            place(from.0 + step, from.1)?;
            std::thread::sleep(PROBE_SETTLE);
            let after = read_cursor()?;
            let _ = place(from.0, from.1);
            Ok(after != from)
        }
    }

    /// Normalize a global physical pixel and send it. The whole of the
    /// Windows coordinate story, in one place.
    ///
    /// The desktop is read on every move rather than cached at
    /// construction: a monitor unplugged or a resolution changed mid-run
    /// moves the rectangle these numbers are measured against, and a stale
    /// one would put every subsequent click somewhere plausible and wrong.
    fn place(x: i32, y: i32) -> Result<()> {
        let desktop = win::virtual_desktop();
        let (dx, dy) = normalize(desktop, x, y).map_err(|error| anyhow!("{error}"))?;
        win::move_absolute(dx, dy).map_err(|reason| anyhow!("move to ({x}, {y}): {reason}"))
    }

    /// Where Windows says the pointer is, or why it would not say.
    fn read_cursor() -> Result<(i32, i32)> {
        win::cursor_position().ok_or_else(|| anyhow!("cannot read the cursor position"))
    }

    fn to_enigo(button: Button) -> EnigoButton {
        match button {
            Button::Left => EnigoButton::Left,
        }
    }

    impl Injector for WindowsInjector {
        /// Takes global physical pixels — the session's own space, and the
        /// virtual desktop's, given the per-monitor DPI awareness `main`
        /// declares at startup.
        fn move_to(&mut self, x: f64, y: f64) -> Result<()> {
            place(x.round() as i32, y.round() as i32)
        }

        fn click(&mut self, button: Button) -> Result<()> {
            self.enigo
                .button(to_enigo(button), Direction::Click)
                .map_err(|error| anyhow!("click failed: {error}"))
        }

        fn double_click(&mut self, button: Button) -> Result<()> {
            self.click(button)?;
            // The application decides what counts as a double-click by
            // timing, same as everywhere else; a short gap keeps both
            // inside its window.
            std::thread::sleep(std::time::Duration::from_millis(40));
            self.click(button)
        }

        fn press(&mut self, button: Button) -> Result<()> {
            self.enigo
                .button(to_enigo(button), Direction::Press)
                .map_err(|error| anyhow!("press failed: {error}"))
        }

        fn release(&mut self, button: Button) -> Result<()> {
            self.enigo
                .button(to_enigo(button), Direction::Release)
                .map_err(|error| anyhow!("release failed: {error}"))
        }

        /// `KEYEVENTF_UNICODE`, so the text does not have to be reachable
        /// on the active layout. The documented limits are the same as
        /// macOS's Unicode path: it cannot express a shortcut, and a target
        /// with an IME active may compose rather than commit.
        fn text(&mut self, text: &str) -> Result<()> {
            self.enigo
                .text(text)
                .map_err(|error| anyhow!("typing failed: {error}"))
        }

        fn chord(&mut self, chord: &str) -> Result<()> {
            let (modifiers, key) = pixelactions_core::chord::split(chord)?;
            let mut held = Vec::new();
            for token in &modifiers {
                let modifier = key_for(token)?;
                self.enigo
                    .key(modifier, Direction::Press)
                    .map_err(|error| anyhow!("holding {token} failed: {error}"))?;
                held.push(modifier);
            }
            let result = self
                .enigo
                .key(key_for(key)?, Direction::Click)
                .map_err(|error| anyhow!("pressing {key} failed: {error}"));
            // Release in reverse whatever happened to the key press — a
            // stuck modifier is worse than a failed chord.
            for modifier in held.into_iter().rev() {
                let _ = self.enigo.key(modifier, Direction::Release);
            }
            result
        }

        /// Wheel clicks land wherever the pointer is. `MOUSEEVENTF_WHEEL`
        /// carries no coordinate, so this needs nothing from the virtual
        /// desktop and enigo's path is the right one.
        fn scroll(&mut self, amount: i32, axis: Axis) -> Result<()> {
            let axis = match axis {
                Axis::Vertical => EnigoAxis::Vertical,
                Axis::Horizontal => EnigoAxis::Horizontal,
            };
            self.enigo
                .scroll(amount, axis)
                .map_err(|error| anyhow!("cannot scroll: {error}"))
        }

        /// Windows answers where the pointer is, which is what keeps the
        /// corner kill switch armed here as it is on X11 and macOS.
        fn cursor(&mut self) -> Result<(f64, f64)> {
            let (x, y) = read_cursor()?;
            Ok((f64::from(x), f64::from(y)))
        }

        /// Move one pixel and ask Windows whether it happened.
        ///
        /// Both directions are tried for the same reason X11 tries both: a
        /// pointer already parked on the right edge cannot go further
        /// right, and reporting that as a failure would blame the tool for
        /// where the mouse happened to be sitting.
        ///
        /// Here the edge does not clamp, it **refuses** — one pixel past
        /// the desktop is not a point, and `place` says so. So a refusal
        /// from the first direction is exactly what the second direction
        /// exists for, and it is carried rather than propagated: only a
        /// pointer that moves neither way has failed.
        fn probe(&mut self) -> Result<()> {
            let from = read_cursor()?;
            let mut refused = None;
            for step in [1, -1] {
                match Self::nudge(from, step) {
                    Ok(true) => return Ok(()),
                    Ok(false) => {}
                    Err(error) => refused = Some(error),
                }
            }
            if let Some(error) = refused {
                return Err(error.context(
                    "the cursor could not be moved in either direction to prove injection works",
                ));
            }
            bail!(
                "the cursor did not move: Windows accepted the event and the pointer \
                 stayed at ({}, {}). Something holds the input desktop — a higher-integrity \
                 window with focus is the usual cause, and no permission exists that would \
                 change that",
                from.0,
                from.1
            )
        }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use anyhow::{Result, anyhow};
    use enigo::{
        Axis as EnigoAxis, Button as EnigoButton, Coordinate, Direction, Enigo, Keyboard, Mouse,
        Settings,
    };
    use pixelactions_core::flow::Axis;

    use super::{Button, Injector, keys::key_for};

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
