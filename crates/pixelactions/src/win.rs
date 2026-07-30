//! Windows-only helpers: the OS calls the injector and `doctor` need, and
//! nothing else. The arithmetic they feed lives in
//! [`pixelactions_core::virtualdesk`], where it is tested without a screen.
//!
//! **Why this module exists at all.** enigo owns the Windows keyboard here
//! — `KEYEVENTF_UNICODE` for text, virtual keys with the extended-key flag
//! for chords — and that half needs nothing added. Absolute pointer motion
//! is the half it cannot do: enigo 0.6.1 normalizes against
//! `SM_CXSCREEN`/`SM_CYSCREEN`, the **primary monitor**, and never sets
//! `MOUSEEVENTF_VIRTUALDESK` (its `move_mouse` carries a `TODO` asking
//! whether it should). Every coordinate on a secondary display would land
//! on the primary one instead — silently, at a plausible-looking position.
//! That is precisely pitfall 4 in `design/02-TECHNICAL-FOUNDATIONS.md` §7,
//! so the ~40 lines below are written rather than depended on.
//!
//! **DPI awareness is a precondition, not a detail.** A process that is not
//! per-monitor-v2 aware is lied to by Windows: `GetSystemMetrics` and
//! `GetCursorPos` return coordinates virtualized against the primary
//! monitor's scale, so on a mixed-DPI desktop the numbers here and the
//! pixels pixelcoords recorded are different quantities. pixelcoords
//! declares the same awareness in its own `win.rs` for the same reason, and
//! the two must agree or every coordinate is wrong by a scale factor.

use pixelactions_core::virtualdesk::VirtualDesktop;
use windows::Win32::Foundation::{CloseHandle, HANDLE, POINT};
use windows::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::Win32::UI::HiDpi::{
    AreDpiAwarenessContextsEqual, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    GetThreadDpiAwarenessContext, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_MOVE, MOUSEEVENTF_VIRTUALDESK,
    MOUSEINPUT, SendInput,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
    SM_YVIRTUALSCREEN,
};

/// Declare this process per-monitor DPI-aware, before anything asks the OS
/// about coordinates.
///
/// Best-effort and idempotent, exactly as in pixelcoords: Windows refuses
/// the call once awareness has been set — by an earlier call, an
/// application manifest, or an app-compatibility override — and that
/// refusal is not an error. Returns whether *this* call established it.
///
/// enigo deliberately does not do this (a library that changed a host
/// application's DPI mode would rescale its windows); a CLI with no windows
/// of its own has no such conflict, so it is done here, once, in `main`.
pub fn become_dpi_aware() -> bool {
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) }.is_ok()
}

/// Whether the process really is per-monitor-v2 aware. Reported by
/// `doctor` rather than assumed, because an external compatibility
/// override can force a different mode and every coordinate would then be
/// silently rescaled.
pub fn is_per_monitor_aware_v2() -> bool {
    unsafe {
        AreDpiAwarenessContextsEqual(
            GetThreadDpiAwarenessContext(),
            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        )
    }
    .as_bool()
}

/// The bounding box of every attached monitor — the rectangle
/// `MOUSEEVENTF_VIRTUALDESK` measures an absolute event against.
pub fn virtual_desktop() -> VirtualDesktop {
    unsafe {
        VirtualDesktop {
            x: GetSystemMetrics(SM_XVIRTUALSCREEN),
            y: GetSystemMetrics(SM_YVIRTUALSCREEN),
            width: GetSystemMetrics(SM_CXVIRTUALSCREEN),
            height: GetSystemMetrics(SM_CYVIRTUALSCREEN),
        }
    }
}

/// Where Windows says the pointer is, in global physical pixels.
///
/// The same space the session records, given the awareness declared above
/// — which is what lets the corner kill switch and the probe compare a
/// read-back position against a coordinate that came out of a flow.
pub fn cursor_position() -> Option<(i32, i32)> {
    let mut point = POINT { x: 0, y: 0 };
    unsafe { GetCursorPos(&raw mut point) }.ok()?;
    Some((point.x, point.y))
}

/// Send one absolute pointer move, already normalized to the 0..65535 grid.
///
/// `MOUSEEVENTF_VIRTUALDESK` is what makes that grid mean the whole desktop
/// rather than the primary monitor. Without it the same numbers address a
/// different rectangle, and a click meant for a secondary display lands on
/// the primary one.
pub fn move_absolute(dx: i32, dy: i32) -> Result<(), &'static str> {
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: 0,
                dwFlags: MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
                // Always 0 — the system stamps its own; see the Old New
                // Thing on why a caller-supplied time is a bug source.
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let sent = unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
    if sent == 1 {
        return Ok(());
    }
    // The documented cause of a swallowed event, and the one a reader can
    // act on: a higher-integrity window owns the input desktop.
    Err(
        "Windows did not accept the event. This is what UIPI looks like: a process at \
         medium integrity cannot send input to an elevated window, the UAC dialog, or the \
         login screen. Run the target program unelevated, or accept that it is out of reach",
    )
}

/// Whether this process runs elevated.
///
/// Reported because it is the whole of the UIPI story in one bit: an
/// unelevated pixelactions cannot drive an elevated window, and an elevated
/// one can drive everything except the secure desktop. Saying which of the
/// two you are running beats a warning that applies to neither.
///
/// A token that cannot be opened or read is reported as `None` rather than
/// guessed at — "we could not tell" is a different answer from "no".
pub fn is_elevated() -> Option<bool> {
    let mut token = HANDLE::default();
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut token) }.ok()?;
    let mut elevation = TOKEN_ELEVATION::default();
    let mut returned = 0u32;
    let result = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            Some((&raw mut elevation).cast()),
            u32::try_from(std::mem::size_of::<TOKEN_ELEVATION>()).ok()?,
            &raw mut returned,
        )
    };
    // The handle is ours either way; released before the answer is read so
    // no early return can leak it.
    let _ = unsafe { CloseHandle(token) };
    result.ok()?;
    Some(elevation.TokenIsElevated != 0)
}
