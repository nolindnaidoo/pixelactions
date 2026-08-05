//! Platform-free core for pixelactions.
//!
//! Everything here is arithmetic and data: parsing a flow file, resolving
//! its steps against a pixelcoords session, and converting a physical
//! pixel into the coordinate space a platform's input API expects. No
//! window system, no OS calls, no capture — those live in the binary.
//!
//! The split exists for the same reason it does in pixelcoords: the part
//! that decides *where to click* must be testable without a screen.
//!
//! The crate README is included below, which makes every example on the
//! crates.io page a compiled doctest — documentation that cannot rot
//! without failing CI.
#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

pub mod audit;
/// Internal: key-chord parsing for input synthesis. Public only because the binary is a
/// separate crate; not part of this crate's API.
#[doc(hidden)]
pub mod chord;
pub mod convert;
/// Internal: which display server this session runs on. Public only because the binary is a
/// separate crate; not part of this crate's API.
#[doc(hidden)]
pub mod display;
pub mod flow;
pub mod plan;
pub mod protocol;
pub mod report;
/// Internal: Wayland screencast coordinate space. Public only because the binary is a
/// separate crate; not part of this crate's API.
#[doc(hidden)]
pub mod stream;
pub mod verb;
/// Internal: Windows virtual-desktop normalization. Public only because the binary is a
/// separate crate; not part of this crate's API.
#[doc(hidden)]
pub mod virtualdesk;
