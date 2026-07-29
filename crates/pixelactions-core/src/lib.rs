//! Platform-free core for pixelactions.
//!
//! Everything here is arithmetic and data: parsing a flow file, resolving
//! its steps against a pixelcoords session, and converting a physical
//! pixel into the coordinate space a platform's input API expects. No
//! window system, no OS calls, no capture — those live in the binary.
//!
//! The split exists for the same reason it does in pixelcoords: the part
//! that decides *where to click* must be testable without a screen.
#![forbid(unsafe_code)]

pub mod chord;
pub mod convert;
pub mod flow;
pub mod plan;
pub mod protocol;
pub mod report;
pub mod verb;
