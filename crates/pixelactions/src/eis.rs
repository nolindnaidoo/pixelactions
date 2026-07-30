//! The acting half of Wayland input: an EI sender over the socket the
//! portal granted.
//!
//! [`crate::portal`] negotiates consent and hands back a connected socket;
//! everything here is the protocol on top of it. The compositor is in
//! charge throughout — it decides which devices exist, what regions they
//! cover, and when they may emit. That is not a limitation to work around,
//! it is the design, and it is why this module reports what it was given
//! rather than assuming a screen.
//!
//! Three things learned the hard way, recorded so they are not re-learned:
//!
//! - **Flush after every batch.** The compositor opens with
//!   `ei_connection.ping` and will not proceed until it is answered.
//!   Queueing the reply without flushing deadlocks both sides: it waits
//!   for a pong, we wait for a seat, and nothing ever arrives.
//! - **`start_emulating` takes `(last_serial, sequence)`** in that order.
//!   reis's own example passes them the other way round.
//! - **Regions come from EIS, not from the screencast stream.** The
//!   granted stream is what *causes* the region to exist, but its geometry
//!   arrives here, on the device — so absolute placement needs no `PipeWire`
//!   connection.

use std::collections::HashMap;
use std::io::ErrorKind;
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow, bail};
use pixelactions_core::flow::Axis;
use pixelactions_core::stream::{Placement, Region};
use reis::{Interface, ei};
use xkbcommon::xkb;

/// evdev's left mouse button. EI speaks evdev codes, not a button enum.
const BTN_LEFT: u32 = 0x110;
/// evdev keycodes are xkb keycodes minus 8. A constant rather than a bare
/// `- 8` at four call sites, because getting it wrong types the wrong key.
const XKB_TO_EVDEV: u32 = 8;
/// One wheel detent, in the fractional units `ei_scroll` counts.
const SCROLL_DETENT: i32 = 120;
/// How long to wait for the compositor to produce a usable device before
/// giving up. Generous: it may be building a screencast pipeline.
const SETTLE: Duration = Duration::from_secs(10);
/// How long a key stays down. Short enough to be imperceptible, long
/// enough that press and release land in different frames — see `tap`.
const KEY_HOLD: Duration = Duration::from_millis(25);

/// A live EI sender with at least one absolute-pointer device.
pub struct Sender {
    context: ei::Context,
    devices: Vec<Slot>,
    /// The compositor's own keymap. Typing is bound to it — there is no
    /// remap trick on Wayland the way there is on X11.
    keymap: Option<xkb::Keymap>,
    seat_capabilities: HashMap<String, u64>,
    /// The newest serial the compositor has sent. Every request that takes
    /// a serial takes this one.
    serial: u32,
    sequence: u32,
    /// Set when a `RegionMappingId` arrives, consumed by the `Region` that
    /// follows it — the protocol pairs them by order, not by field.
    pending_mapping_id: Option<String>,
}

struct Slot {
    device: ei::Device,
    interfaces: HashMap<String, reis::Object>,
    regions: Vec<Region>,
    /// Resumed means "you may emit now". Paused means the opposite, and it
    /// can happen mid-run when the compositor takes input back.
    resumed: bool,
    emulating: bool,
}

impl Slot {
    fn interface<T: reis::Interface>(&self) -> Option<T> {
        self.interfaces.get(T::NAME)?.clone().downcast()
    }

    fn has<T: reis::Interface>(&self) -> bool {
        self.interfaces.contains_key(T::NAME)
    }
}

impl Sender {
    /// Handshake as a sender and wait until the compositor offers a device
    /// that can place a pointer absolutely.
    pub fn connect(socket: UnixStream) -> Result<Self> {
        // Non-blocking so `settle` can time out instead of hanging on a
        // compositor that never answers. reis reports this as WouldBlock,
        // which the pump treats as "nothing yet".
        socket
            .set_nonblocking(true)
            .map_err(|e| anyhow!("cannot configure the EIS socket: {e}"))?;
        let context =
            ei::Context::new(socket).map_err(|e| anyhow!("cannot open the EIS connection: {e}"))?;
        let handshake = reis::handshake::ei_handshake_blocking(
            &context,
            "pixelactions",
            ei::handshake::ContextType::Sender,
        )
        .map_err(|e| anyhow!("the EIS handshake failed: {e}"))?;

        let mut sender = Self {
            context,
            devices: Vec::new(),
            keymap: None,
            seat_capabilities: HashMap::new(),
            serial: handshake.serial,
            sequence: 0,
            pending_mapping_id: None,
        };
        sender.settle()?;
        Ok(sender)
    }

    /// The regions of the device absolute motion will be sent to. This is
    /// what [`pixelactions_core::stream::place`] maps into.
    pub fn regions(&self) -> &[Region] {
        self.pointer_slot()
            .map_or(&[], |slot| slot.regions.as_slice())
    }

    /// Whether the compositor gave us a keyboard with a keymap. Without
    /// one, typing and chords are refused rather than guessed at.
    pub fn can_type(&self) -> bool {
        self.keymap.is_some() && self.keyboard().is_some()
    }

    fn pointer_slot(&self) -> Option<&Slot> {
        self.devices
            .iter()
            .find(|slot| slot.has::<ei::PointerAbsolute>())
    }

    fn keyboard(&self) -> Option<(ei::Keyboard, &Slot)> {
        let slot = self
            .devices
            .iter()
            .find(|slot| slot.has::<ei::Keyboard>())?;
        Some((slot.interface::<ei::Keyboard>()?, slot))
    }

    /// Read and dispatch whatever has arrived, for up to `budget`.
    fn pump(&mut self, budget: Duration) -> Result<()> {
        let deadline = Instant::now() + budget;
        loop {
            match self.context.read() {
                Ok(_) => {}
                Err(error) if error.kind() == ErrorKind::WouldBlock => {}
                Err(error) => bail!("the EIS connection broke: {error}"),
            }
            while let Some(pending) = self.context.pending_event() {
                match pending {
                    reis::PendingRequestResult::Request(event) => self.dispatch(event),
                    // A protocol error is the compositor and this build
                    // disagreeing; carrying on would be acting blind.
                    reis::PendingRequestResult::ParseError(error) => {
                        bail!("the compositor sent an EI message this build cannot read: {error}")
                    }
                    reis::PendingRequestResult::InvalidObject(object) => {
                        bail!("the compositor referred to an unknown EI object {object}")
                    }
                }
            }
            // Anything queued by dispatch -- the ping reply above all --
            // is still in a buffer until now. See the module note.
            let _ = self.context.flush();
            if Instant::now() >= deadline {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn dispatch(&mut self, event: ei::Event) {
        match event {
            ei::Event::Connection(_, event) => Self::on_connection(event),
            ei::Event::Seat(seat, event) => self.on_seat(&seat, event),
            ei::Event::Device(device, event) => self.on_device(&device, event),
            ei::Event::Keyboard(_, ei::keyboard::Event::Keymap { size, keymap, .. }) => {
                self.on_keymap(size, keymap);
            }
            _ => {}
        }
    }

    /// The connection carries no state we keep: seats announce themselves
    /// separately, and a ping only needs answering.
    fn on_connection(event: ei::connection::Event) {
        // Answering this is not optional -- see the module note.
        if let ei::connection::Event::Ping { ping } = event {
            ping.done(0);
        }
    }

    fn on_seat(&mut self, seat: &ei::Seat, event: ei::seat::Event) {
        match event {
            ei::seat::Event::Capability { mask, interface } => {
                self.seat_capabilities.insert(interface, mask);
            }
            // Bind everything a flow can express, in one call: the seat
            // creates devices from what was bound, and a capability not
            // asked for here is one no step can use later.
            ei::seat::Event::Done => {
                let mask = [
                    ei::PointerAbsolute::NAME,
                    ei::Button::NAME,
                    ei::Scroll::NAME,
                    ei::Keyboard::NAME,
                ]
                .iter()
                .filter_map(|name| self.seat_capabilities.get(*name))
                .fold(0u64, |all, one| all | one);
                if mask != 0 {
                    seat.bind(mask);
                }
            }
            _ => {}
        }
    }

    fn on_device(&mut self, device: &ei::Device, event: ei::device::Event) {
        // Devices announce themselves before anything else, so a slot
        // always exists by the time later events arrive.
        let known = self.devices.iter().position(|slot| &slot.device == device);
        let index = known.unwrap_or_else(|| {
            self.devices.push(Slot {
                device: device.clone(),
                interfaces: HashMap::new(),
                regions: Vec::new(),
                resumed: false,
                emulating: false,
            });
            self.devices.len() - 1
        });
        match event {
            ei::device::Event::Interface { object } => {
                self.devices[index]
                    .interfaces
                    .insert(object.interface().to_owned(), object);
            }
            ei::device::Event::RegionMappingId { mapping_id } => {
                self.pending_mapping_id = Some(mapping_id);
            }
            ei::device::Event::Region {
                offset_x,
                offset_y,
                width,
                hight,
                scale,
            } => {
                self.devices[index].regions.push(Region {
                    offset_x: offset_x as i32,
                    offset_y: offset_y as i32,
                    width: width as i32,
                    height: hight as i32,
                    scale: f64::from(scale),
                    mapping_id: self.pending_mapping_id.take(),
                });
            }
            ei::device::Event::Resumed { serial } => {
                self.serial = serial;
                self.devices[index].resumed = true;
            }
            ei::device::Event::Paused { serial } => {
                self.serial = serial;
                self.devices[index].resumed = false;
                self.devices[index].emulating = false;
            }
            ei::device::Event::Destroyed { serial } => {
                self.serial = serial;
                self.devices.remove(index);
            }
            _ => {}
        }
    }

    fn on_keymap(&mut self, size: u32, keymap: std::os::fd::OwnedFd) {
        let context = xkb::Context::new(0);
        // SAFETY: the fd and its length come from the compositor over the
        // EI socket, which is the documented way a keymap is delivered.
        // xkbcommon mmaps exactly `size` bytes of it and copies what it
        // needs; the fd is owned here and dropped after.
        let loaded = unsafe {
            xkb::Keymap::new_from_fd(
                &context,
                keymap,
                size as usize,
                xkb::KEYMAP_FORMAT_TEXT_V1,
                xkb::KEYMAP_COMPILE_NO_FLAGS,
            )
        };
        // A keymap that will not compile is not fatal: pointer steps still
        // work, and typing refuses by name via `can_type`.
        if let Ok(Some(keymap)) = loaded {
            self.keymap = Some(keymap);
        }
    }

    /// Wait for a device that can place a pointer, and — if the seat says
    /// a keyboard is coming — for that keyboard and its keymap too.
    ///
    /// The keyboard has to be waited for on the *seat's* advertisement,
    /// not on whether a keyboard device has shown up yet. The pointer
    /// becomes usable first, and treating "no keyboard device so far" as
    /// "no keyboard at all" loses the race almost as often as it wins it
    /// — leaving `can_type` false for a grant that does have a keyboard,
    /// so every later `type` step refuses for no reason.
    fn settle(&mut self) -> Result<()> {
        let deadline = Instant::now() + SETTLE;
        while Instant::now() < deadline {
            self.pump(Duration::from_millis(100))?;
            let pointer_ready = self
                .pointer_slot()
                .is_some_and(|slot| slot.resumed && !slot.regions.is_empty());
            let keyboard_promised = self.seat_capabilities.contains_key(ei::Keyboard::NAME);
            let keyboard_settled = !keyboard_promised || self.can_type();
            if pointer_ready && keyboard_settled {
                return Ok(());
            }
        }

        // Out of time. A pointer that works is still worth running with:
        // a click-only flow needs no keyboard, and `text` and `chord`
        // refuse by name through `can_type`. Only a missing pointer is
        // fatal, because nothing can be aimed without it.
        if self
            .pointer_slot()
            .is_some_and(|slot| slot.resumed && !slot.regions.is_empty())
        {
            return Ok(());
        }

        let Some(slot) = self.pointer_slot() else {
            bail!(
                "the compositor granted a session but never offered a pointer that can be \
                 placed at a coordinate. On GNOME and KDE this is what \
                 org.freedesktop.portal.RemoteDesktop provides; other compositors do not \
                 implement it yet"
            );
        };
        if slot.regions.is_empty() {
            bail!(
                "the pointer the compositor offered has no region, so there is no coordinate \
                 space to aim in. This is what the linked screen share provides — re-run and \
                 share a screen rather than cancelling that part"
            );
        }
        bail!("the compositor never allowed the pointer to emit (it stayed paused)")
    }

    /// Bring a device to the state where it may emit, and remember it.
    fn begin(&mut self, index: usize) -> Result<()> {
        if !self.devices[index].resumed {
            bail!(
                "the compositor has paused input emulation, which it does when something \
                 else takes over the pointer. Nothing was sent"
            );
        }
        if self.devices[index].emulating {
            return Ok(());
        }
        let serial = self.serial;
        let sequence = self.sequence;
        self.sequence += 1;
        // Argument order is (last_serial, sequence) -- see module note.
        self.devices[index].device.start_emulating(serial, sequence);
        self.devices[index].emulating = true;
        Ok(())
    }

    /// Close one logical action. EI batches until a frame, and the
    /// compositor acts on frames — so a click without one is a click that
    /// never happens.
    fn frame(&mut self, index: usize) -> Result<()> {
        let serial = self.serial;
        self.devices[index].device.frame(serial, now_micros());
        self.context
            .flush()
            .map_err(|e| anyhow!("cannot send to the compositor: {e}"))
    }

    fn pointer_index(&self) -> Result<usize> {
        self.devices
            .iter()
            .position(Slot::has::<ei::PointerAbsolute>)
            .ok_or_else(|| anyhow!("the pointer device went away mid-run"))
    }

    fn keyboard_index(&self) -> Result<usize> {
        self.devices
            .iter()
            .position(Slot::has::<ei::Keyboard>)
            .ok_or_else(|| anyhow!("the compositor offered no keyboard, so this step cannot run"))
    }

    /// Place the pointer at an already-resolved point in a region.
    pub fn move_to(&mut self, placement: Placement) -> Result<()> {
        let index = self.pointer_index()?;
        self.begin(index)?;
        let absolute = self.devices[index]
            .interface::<ei::PointerAbsolute>()
            .ok_or_else(|| anyhow!("the pointer device lost its absolute-motion interface"))?;
        absolute.motion_absolute(placement.x as f32, placement.y as f32);
        self.frame(index)
    }

    pub fn button(&mut self, press: bool) -> Result<()> {
        let index = self.pointer_index()?;
        self.begin(index)?;
        let button = self.devices[index]
            .interface::<ei::Button>()
            .ok_or_else(|| anyhow!("the compositor offered no mouse buttons"))?;
        let state = if press {
            ei::button::ButtonState::Press
        } else {
            ei::button::ButtonState::Released
        };
        button.button(BTN_LEFT, state);
        self.frame(index)
    }

    pub fn scroll(&mut self, clicks: i32, axis: Axis) -> Result<()> {
        let index = self.pointer_index()?;
        self.begin(index)?;
        let scroll = self.devices[index]
            .interface::<ei::Scroll>()
            .ok_or_else(|| anyhow!("the compositor offered no scroll capability"))?;
        let amount = clicks.saturating_mul(SCROLL_DETENT);
        match axis {
            Axis::Vertical => scroll.scroll_discrete(0, amount),
            Axis::Horizontal => scroll.scroll_discrete(amount, 0),
        }
        self.frame(index)
    }

    /// Tap one evdev keycode, holding `modifiers` around it.
    ///
    /// Press and release go in **separate frames** with a gap between
    /// them, because a frame is what the compositor acts on: putting both
    /// in one makes a key that was never observably held. Applications
    /// that bind an action to a key's *release* then never see the edge —
    /// measured against the pixelcoords overlay, whose release-bound keys
    /// did nothing until this was split, while its press-bound keys had
    /// worked all along.
    fn tap(&mut self, keycode: u32, modifiers: &[u32]) -> Result<()> {
        let index = self.keyboard_index()?;
        self.begin(index)?;
        let keyboard = self.devices[index]
            .interface::<ei::Keyboard>()
            .ok_or_else(|| anyhow!("the keyboard device lost its interface"))?;
        for modifier in modifiers {
            keyboard.key(*modifier, ei::keyboard::KeyState::Press);
        }
        keyboard.key(keycode, ei::keyboard::KeyState::Press);
        self.frame(index)?;
        std::thread::sleep(KEY_HOLD);

        let keyboard = self.devices[index]
            .interface::<ei::Keyboard>()
            .ok_or_else(|| anyhow!("the keyboard device lost its interface"))?;
        keyboard.key(keycode, ei::keyboard::KeyState::Released);
        // Reverse order, and unconditionally: a modifier left held is
        // worse than a keystroke that did not land, because it corrupts
        // everything the user types next.
        for modifier in modifiers.iter().rev() {
            keyboard.key(*modifier, ei::keyboard::KeyState::Released);
        }
        self.frame(index)
    }

    /// Type literal text through the compositor's own keymap.
    ///
    /// Unlike X11, there is no temporary-remap trick available here: an EI
    /// keyboard carries the compositor's keymap and nothing else. A
    /// character the active layout cannot reach is therefore refused by
    /// name rather than silently typed as something else.
    pub fn text(&mut self, text: &str) -> Result<()> {
        let keymap = self
            .keymap
            .clone()
            .ok_or_else(|| anyhow!("the compositor sent no keymap, so text cannot be typed"))?;
        for character in text.chars() {
            let Some((keycode, level)) = key_for_char(&keymap, character) else {
                bail!(
                    "the active keyboard layout has no key for {character:?}, and Wayland \
                     gives no way to remap one for a moment the way X11 does. Type it with \
                     a layout that has it, or paste instead"
                );
            };
            let modifiers = modifiers_for_level(&keymap, level).ok_or_else(|| {
                anyhow!(
                    "{character:?} needs modifier level {level} on this layout, which this \
                     build cannot reproduce"
                )
            })?;
            self.tap(keycode, &modifiers)?;
        }
        Ok(())
    }

    /// Press a chord such as `ctrl+s`.
    pub fn chord(&mut self, chord: &str) -> Result<()> {
        let keymap = self
            .keymap
            .clone()
            .ok_or_else(|| anyhow!("the compositor sent no keymap, so chords cannot be sent"))?;
        let (modifier_names, key) = pixelactions_core::chord::split(chord)?;
        let mut modifiers = Vec::with_capacity(modifier_names.len());
        for name in &modifier_names {
            let keysym = keysym_for_token(name)
                .ok_or_else(|| anyhow!("{name:?} is not a modifier this build knows"))?;
            modifiers.push(keycode_for_keysym(&keymap, keysym).ok_or_else(|| {
                anyhow!("the active layout has no {name:?} key to hold for {chord:?}")
            })?);
        }
        let keysym = keysym_for_token(key)
            .ok_or_else(|| anyhow!("{key:?} is not a key name this build knows"))?;
        let keycode = keycode_for_keysym(&keymap, keysym)
            .ok_or_else(|| anyhow!("the active layout has no {key:?} key for {chord:?}"))?;
        self.tap(keycode, &modifiers)
    }
}

impl Drop for Sender {
    /// Stop emulating on the way out. A device left emulating is a device
    /// the compositor keeps reserved after this process is gone.
    fn drop(&mut self) {
        let serial = self.serial;
        for slot in &mut self.devices {
            if slot.emulating {
                slot.device.stop_emulating(serial);
                slot.emulating = false;
            }
        }
        let _ = self.context.flush();
    }
}

/// Monotonic microseconds — the clock EI timestamps use.
fn now_micros() -> u64 {
    // Anchored to process start rather than the epoch. EI only compares
    // timestamps to each other, so any monotonic origin works.
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    let start = START.get_or_init(Instant::now);
    start.elapsed().as_micros() as u64
}

/// Find a key that produces `character`, and the shift level it needs.
///
/// A linear scan of the keymap, which is what xkbcommon supports: the
/// mapping runs keycode+level to keysym, and this question is the inverse.
/// A keymap is a few hundred keycodes, and a step types a handful of
/// characters, so the cost is invisible next to one screen capture.
fn key_for_char(keymap: &xkb::Keymap, character: char) -> Option<(u32, u32)> {
    let wanted = xkb::Keysym::from_char(character);
    let layout = 0;
    for raw in keymap.min_keycode().raw()..=keymap.max_keycode().raw() {
        let keycode = xkb::Keycode::new(raw);
        for level in 0..keymap.num_levels_for_key(keycode, layout) {
            if keymap
                .key_get_syms_by_level(keycode, layout, level)
                .contains(&wanted)
            {
                return Some((raw - XKB_TO_EVDEV, level));
            }
        }
    }
    None
}

/// The evdev keycode of the first key that produces `keysym` unmodified.
fn keycode_for_keysym(keymap: &xkb::Keymap, keysym: xkb::Keysym) -> Option<u32> {
    let layout = 0;
    for raw in keymap.min_keycode().raw()..=keymap.max_keycode().raw() {
        let keycode = xkb::Keycode::new(raw);
        for level in 0..keymap.num_levels_for_key(keycode, layout) {
            if keymap
                .key_get_syms_by_level(keycode, layout, level)
                .contains(&keysym)
            {
                return Some(raw - XKB_TO_EVDEV);
            }
        }
    }
    None
}

/// Which modifiers to hold to reach a shift level.
///
/// The level-to-modifier relationship is a layout convention rather than
/// something xkbcommon will answer directly: level 1 is unmodified, 2 is
/// Shift, 3 is `AltGr`, 4 is both. Levels beyond that exist on some layouts
/// and are refused rather than approximated.
fn modifiers_for_level(keymap: &xkb::Keymap, level: u32) -> Option<Vec<u32>> {
    let shift = || keycode_for_keysym(keymap, xkb::keysyms::KEY_Shift_L.into());
    let altgr = || keycode_for_keysym(keymap, xkb::keysyms::KEY_ISO_Level3_Shift.into());
    match level {
        0 => Some(Vec::new()),
        1 => Some(vec![shift()?]),
        2 => Some(vec![altgr()?]),
        3 => Some(vec![shift()?, altgr()?]),
        _ => None,
    }
}

/// Map a chord token to a keysym. Modifier names are spelled the way a
/// human writes them, and deliberately the same way the macOS injector
/// spells them, so one chord string works on both.
fn keysym_for_token(token: &str) -> Option<xkb::Keysym> {
    let keysym = match token.to_ascii_lowercase().as_str() {
        // Super is the Wayland/Linux name for what a Mac calls cmd. The
        // aliases exist so a flow written on a Mac still parses here.
        "cmd" | "command" | "meta" | "super" => xkb::keysyms::KEY_Super_L,
        "ctrl" | "control" => xkb::keysyms::KEY_Control_L,
        "alt" | "option" | "opt" => xkb::keysyms::KEY_Alt_L,
        "shift" => xkb::keysyms::KEY_Shift_L,
        "tab" => xkb::keysyms::KEY_Tab,
        "enter" | "return" => xkb::keysyms::KEY_Return,
        "esc" | "escape" => xkb::keysyms::KEY_Escape,
        "space" => xkb::keysyms::KEY_space,
        "backspace" | "delete" => xkb::keysyms::KEY_BackSpace,
        "up" => xkb::keysyms::KEY_Up,
        "down" => xkb::keysyms::KEY_Down,
        "left" => xkb::keysyms::KEY_Left,
        "right" => xkb::keysyms::KEY_Right,
        other => {
            let mut characters = other.chars();
            let first = characters.next()?;
            if characters.next().is_some() {
                return None;
            }
            return Some(xkb::Keysym::from_char(first));
        }
    };
    Some(keysym.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The names in a chord are the same on every platform, which is the
    /// whole point of writing them out rather than taking keycodes.
    #[test]
    fn modifier_names_map_the_way_a_human_writes_them() {
        for name in ["cmd", "command", "meta", "super", "SUPER"] {
            assert_eq!(
                keysym_for_token(name),
                Some(xkb::keysyms::KEY_Super_L.into()),
                "{name}"
            );
        }
        for name in ["ctrl", "control"] {
            assert_eq!(
                keysym_for_token(name),
                Some(xkb::keysyms::KEY_Control_L.into())
            );
        }
        for name in ["alt", "option", "opt"] {
            assert_eq!(keysym_for_token(name), Some(xkb::keysyms::KEY_Alt_L.into()));
        }
    }

    #[test]
    fn named_keys_resolve_and_single_characters_fall_through() {
        assert_eq!(
            keysym_for_token("enter"),
            Some(xkb::keysyms::KEY_Return.into())
        );
        assert_eq!(
            keysym_for_token("esc"),
            Some(xkb::keysyms::KEY_Escape.into())
        );
        assert_eq!(
            keysym_for_token("space"),
            Some(xkb::keysyms::KEY_space.into())
        );
        assert_eq!(keysym_for_token("s"), Some(xkb::Keysym::from_char('s')));
        assert_eq!(keysym_for_token("7"), Some(xkb::Keysym::from_char('7')));
    }

    /// A multi-character token that is not a known name is refused, rather
    /// than silently becoming its first letter.
    #[test]
    fn an_unknown_multi_character_key_is_refused() {
        assert_eq!(keysym_for_token("fnord"), None);
        assert_eq!(keysym_for_token(""), None);
    }

    #[test]
    fn levels_beyond_altgr_are_refused_rather_than_approximated() {
        // A real keymap is needed to resolve modifier keycodes, so this
        // checks only the shape of the refusal, which needs none.
        let keymap = compiled_us_keymap();
        assert!(modifiers_for_level(&keymap, 4).is_none());
        assert!(modifiers_for_level(&keymap, 99).is_none());
        assert_eq!(modifiers_for_level(&keymap, 0), Some(Vec::new()));
    }

    /// Typing rests on this inverse lookup, so it is checked against a
    /// real compiled keymap rather than a fake: a wrong keycode here types
    /// the wrong character, which no unit test on a stub would catch.
    #[test]
    fn characters_resolve_to_keys_on_a_real_keymap() {
        let keymap = compiled_us_keymap();
        let (a_code, a_level) = key_for_char(&keymap, 'a').expect("'a' is on a US layout");
        assert_eq!(a_level, 0, "lowercase needs no modifier");
        // 30 is evdev's KEY_A, which is where 'a' lives on any US layout.
        assert_eq!(a_code, 30);

        let (upper_code, upper_level) = key_for_char(&keymap, 'A').expect("'A' is shift+a");
        assert_eq!(upper_code, a_code, "same key, different level");
        assert_eq!(upper_level, 1);
        assert_eq!(
            modifiers_for_level(&keymap, upper_level).expect("shift resolves"),
            vec![keycode_for_keysym(&keymap, xkb::keysyms::KEY_Shift_L.into()).expect("shift")]
        );
    }

    #[test]
    fn a_character_no_layout_has_is_refused_not_approximated() {
        let keymap = compiled_us_keymap();
        // Not reachable on a plain US layout at any level.
        assert!(key_for_char(&keymap, '☃').is_none());
    }

    /// Every name core promises a flow author, answered here and reachable
    /// on a real layout. This is the Wayland half of the pair that keeps
    /// the two key tables from drifting — `inject::keys` carries the other.
    #[test]
    fn every_promised_name_exists_on_a_real_keymap() {
        let keymap = compiled_us_keymap();
        for name in pixelactions_core::chord::NAMED_KEYS {
            let keysym = keysym_for_token(name).unwrap_or_else(|| panic!("no keysym for {name}"));
            assert_ne!(
                keysym,
                xkb::Keysym::from_char(name.chars().next().expect("non-empty")),
                "{name} fell through to its first character instead of naming a key"
            );
            assert!(
                keycode_for_keysym(&keymap, keysym).is_some(),
                "no keycode for {name}"
            );
        }
    }

    /// Compiled from names, not from a compositor: this is the layout
    /// xkbcommon builds by default, which is a US layout.
    fn compiled_us_keymap() -> xkb::Keymap {
        let context = xkb::Context::new(0);
        xkb::Keymap::new_from_names(&context, "", "", "us", "", None, xkb::COMPILE_NO_FLAGS)
            .expect("xkbcommon can compile a US layout")
    }
}
