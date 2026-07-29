//! macOS permission handling.
//!
//! Posting synthetic events requires the Accessibility grant. enigo
//! checks for it and errors, which leaves a first-time user staring at a
//! refusal with no path forward — macOS never shows its dialog unless
//! something asks for it.
//!
//! `AXIsProcessTrustedWithOptions` with the prompt option is that ask:
//! it puts the standard system dialog on screen and adds the calling
//! application to the Accessibility list, so the user only has to flip
//! the switch.
//!
//! This is the one module with `unsafe`, and it is here for the reason
//! AGENTS.md allows: calling an OS API. Everything below is FFI
//! declarations and one call.

use std::ffi::c_void;

// CoreFoundation's pointer types are all opaque; aliases would read
// nicer but extern blocks don't count as uses, so the lint sees them as
// dead. Inlined instead.

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
    /// The dictionary key whose truth makes the call prompt the user.
    static kAXTrustedCheckOptionPrompt: *const c_void;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFAllocatorDefault: *const c_void;
    static kCFBooleanTrue: *const c_void;
    static kCFTypeDictionaryKeyCallBacks: c_void;
    static kCFTypeDictionaryValueCallBacks: c_void;
    fn CFDictionaryCreate(
        allocator: *const c_void,
        keys: *const *const c_void,
        values: *const *const c_void,
        num_values: isize,
        key_callbacks: *const c_void,
        value_callbacks: *const c_void,
    ) -> *const c_void;
    fn CFRelease(cf: *const c_void);
}

/// Whether this process may post synthetic events. Never prompts.
pub fn is_trusted() -> bool {
    // SAFETY: a parameterless system call returning a plain bool.
    unsafe { AXIsProcessTrusted() }
}

/// Ask macOS for the Accessibility grant, showing its standard dialog.
///
/// Returns whether the process is trusted *right now* — which is almost
/// always `false` on the first call, because the user has yet to answer.
/// The point is the dialog and the resulting entry in the Accessibility
/// list, not the return value.
///
/// Note the grant attaches to the **responsible application**, not to
/// this binary: a CLI launched from a terminal inherits that terminal's
/// grant, which is why the message tells users to add the app they
/// launched from.
pub fn request_trust() -> bool {
    // SAFETY: builds a one-entry CFDictionary from framework-provided
    // statics, passes it to the documented API, and releases it. The
    // pointers come from CoreFoundation itself and outlive the call.
    unsafe {
        let keys = [kAXTrustedCheckOptionPrompt];
        let values = [kCFBooleanTrue];
        let options = CFDictionaryCreate(
            kCFAllocatorDefault,
            keys.as_ptr(),
            values.as_ptr(),
            1,
            std::ptr::addr_of!(kCFTypeDictionaryKeyCallBacks),
            std::ptr::addr_of!(kCFTypeDictionaryValueCallBacks),
        );
        let trusted = AXIsProcessTrustedWithOptions(options);
        if !options.is_null() {
            CFRelease(options);
        }
        trusted
    }
}
