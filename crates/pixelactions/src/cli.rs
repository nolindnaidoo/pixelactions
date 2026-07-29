use std::path::PathBuf;

use clap::{Parser, Subcommand};

const EXAMPLES: &str = "\
Examples:
  pixelactions plan flow.toml            resolve every step, act on nothing
  pixelactions plan flow.toml --json     the same, machine-readable
  pixelactions doctor                    permissions, displays, sister tool

A flow references a pixelcoords session by label, never by coordinate.
Exit codes: 0 done, 1 a step failed honestly, 2 malformed question,
3 refused (permission missing, unsupported platform).";

/// Execute desktop interactions from pixelcoords sessions.
#[derive(Debug, Parser)]
#[command(name = "pixelactions", version, about, after_help = EXAMPLES)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Resolve a flow against its session and print what would happen —
    /// every coordinate, after conversion, with the monitor it landed on.
    /// Touches nothing.
    Plan {
        /// Path to the flow file
        flow: PathBuf,
        /// Machine-readable plan on stdout instead of the human one
        #[arg(long)]
        json: bool,
        /// Override the flow's coordinate space (default: the flow's own
        /// setting, normally `auto` = what this platform's input API wants)
        #[arg(long, value_enum)]
        space: Option<SpaceArg>,
    },
    /// Check what pixelactions needs to run: OS support, input
    /// permission, displays, and the pixelcoords binary it calls.
    Doctor {
        /// Machine-readable report on stdout instead of the human one
        #[arg(long)]
        json: bool,
    },
}

/// The coordinate space to resolve points into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum SpaceArg {
    /// What this platform's input API expects — logical on macOS,
    /// physical on Windows and X11
    Auto,
    /// Physical pixels, the session's own grid
    Physical,
    /// Logical points — physical divided by the monitor's scale
    Logical,
}

impl From<SpaceArg> for pixelactions_core::convert::Space {
    fn from(arg: SpaceArg) -> Self {
        match arg {
            SpaceArg::Auto => Self::Auto,
            SpaceArg::Physical => Self::Physical,
            SpaceArg::Logical => Self::Logical,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_definition_is_consistent() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }
}
