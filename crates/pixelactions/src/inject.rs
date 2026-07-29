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

use anyhow::{Context, Result};
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

/// Split a chord into its modifiers and its final key: `cmd+shift+s` →
/// `(["cmd", "shift"], "s")`. Platform-free, so it is tested here rather
/// than behind a permission prompt.
pub fn split_chord(chord: &str) -> Result<(Vec<&str>, &str)> {
    let mut parts: Vec<&str> = chord
        .split('+')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    let key = parts.pop().context("chord is empty")?;
    Ok((parts, key))
}

#[cfg(target_os = "macos")]
pub use platform::RealInjector;

#[cfg(target_os = "macos")]
mod platform {
    use anyhow::{Context, Result, anyhow};
    use enigo::{
        Axis as EnigoAxis, Button as EnigoButton, Coordinate, Direction, Enigo, Key, Keyboard,
        Mouse, Settings,
    };
    use pixelactions_core::flow::Axis;

    use super::{Button, Injector, split_chord};

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
            let (modifiers, key) = split_chord(chord)?;
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

    #[test]
    fn chords_split_into_modifiers_and_a_key() {
        let (modifiers, key) = split_chord("cmd+shift+s").expect("valid");
        assert_eq!(modifiers, vec!["cmd", "shift"]);
        assert_eq!(key, "s");
    }

    #[test]
    fn a_bare_key_has_no_modifiers() {
        let (modifiers, key) = split_chord("enter").expect("valid");
        assert!(modifiers.is_empty());
        assert_eq!(key, "enter");
    }

    #[test]
    fn whitespace_around_chord_parts_is_tolerated() {
        let (modifiers, key) = split_chord("cmd + s").expect("valid");
        assert_eq!(modifiers, vec!["cmd"]);
        assert_eq!(key, "s");
    }

    #[test]
    fn an_empty_chord_is_an_error() {
        assert!(split_chord("").is_err());
        assert!(split_chord("+").is_err());
    }
}
