use std::path::PathBuf;

use clap::{Parser, Subcommand};

const EXAMPLES: &str = "\
Examples:
  pixelactions run --session DIR click:submit type:\"hi\" key:cmd+s wait:done --yes
  pixelactions plan --flow flow.toml     resolve every step, act on nothing
  pixelactions run --flow flow.toml --yes
  pixelactions serve --session DIR       drive it from your own program
  pixelactions doctor --probe            prove input permission, harmlessly

Verbs: click double verify wait gone type key drag:FROM>TO
       scroll:LABEL>N hscroll:LABEL>N pause:MS
They mirror the flow file's actions one-for-one, so learning either
teaches the other.

Actions reference a pixelcoords session by label, never by coordinate.
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
        /// Path to a flow file. Omit when passing chained verbs.
        #[arg(long, conflicts_with = "session")]
        flow: Option<PathBuf>,
        /// Session directory, for chained verbs
        #[arg(long, value_name = "DIR")]
        session: Option<PathBuf>,
        /// Chained actions to resolve
        #[arg(value_name = "VERB:ARG")]
        verbs: Vec<String>,
        /// Machine-readable plan on stdout instead of the human one
        #[arg(long)]
        json: bool,
        /// Override the flow's coordinate space (default: the flow's own
        /// setting, normally `auto` = what this platform's input API wants)
        #[arg(long, value_enum)]
        space: Option<SpaceArg>,
    },
    /// Perform actions: either a flow file, or verbs chained on the
    /// command line. Requires --yes; without it, prints what it would do
    /// and refuses.
    Run {
        /// Path to a flow file. Omit when passing chained verbs.
        #[arg(long, conflicts_with = "session")]
        flow: Option<PathBuf>,
        /// Session directory, for chained verbs (a flow file names its own)
        #[arg(long, value_name = "DIR")]
        session: Option<PathBuf>,
        /// Chained actions: click:submit type:"hello" key:cmd+s wait:done
        #[arg(value_name = "VERB:ARG")]
        verbs: Vec<String>,
        /// Machine-readable run report on stdout instead of the human one
        #[arg(long)]
        json: bool,
        /// Actually perform the flow. Without this, nothing is injected.
        #[arg(long)]
        yes: bool,
    },
    /// Speak the line protocol on stdin/stdout: one JSON request per
    /// line, one JSON response back. This is how a program in any
    /// language drives pixelactions — it owns the loop, we do the steps.
    /// Logs go to stderr; stdout is protocol only.
    Serve {
        /// Session directory to act against
        #[arg(long, value_name = "DIR")]
        session: PathBuf,
    },
    /// Check what pixelactions needs to run: OS support, input
    /// permission, displays, and the pixelcoords binary it calls.
    Doctor {
        /// Machine-readable report on stdout instead of the human one
        #[arg(long)]
        json: bool,
        /// Actually try to move the cursor one pixel and back, to prove
        /// input permission rather than assume it. Harmless; the cursor
        /// returns to where it was.
        #[arg(long)]
        probe: bool,
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
