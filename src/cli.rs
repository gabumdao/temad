use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Generator, Shell};
use std::io;
use std::path::PathBuf;

/// temad — A lightweight Dynamic Theme and Wallpaper Manager
///
/// Manages wallpaper slideshows, color extraction, and desktop theming
/// through a TOML-driven configuration.
#[derive(Parser, Debug)]
#[command(name = "temad", version, about, long_about = None)]
pub struct Cli {
    /// Path to a custom configuration file
    #[arg(short, long, global = true)]
    pub config: Option<PathBuf>,

    /// Set the verbosity level (repeat for more: -v, -vv, -vvv)
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    pub verbose: u8,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Start the background slideshow daemon
    Daemon,

    /// Immediately set a specific wallpaper and extract colors
    Set {
        /// Path to the wallpaper image file
        path: PathBuf,
    },

    /// Apply the next wallpaper from the configured directory
    Next,

    /// Apply the previous wallpaper from the configured directory
    Prev,

    /// Generate shell completion scripts
    Completions {
        /// Shell to generate completions for
        shell: Shell,
    },
}

pub fn generate_completions<G: Generator>(generator: G, buf: &mut dyn io::Write) {
    generate(generator, &mut Cli::command(), "temad", buf);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bash_completions_include_existing_commands() {
        let mut buf = Vec::new();
        generate_completions(Shell::Bash, &mut buf);
        let output = String::from_utf8(buf).expect("valid UTF-8 completion output");

        assert!(output.contains("temad"));
        assert!(output.contains("daemon"));
        assert!(output.contains("set"));
        assert!(output.contains("next"));
        assert!(output.contains("prev"));
        assert!(output.contains("completions"));
    }

    #[test]
    fn next_command_parses() {
        let cli = Cli::try_parse_from(["temad", "next"]).expect("next should parse");
        assert!(matches!(cli.command, Command::Next));
    }

    #[test]
    fn prev_command_parses() {
        let cli = Cli::try_parse_from(["temad", "prev"]).expect("prev should parse");
        assert!(matches!(cli.command, Command::Prev));
    }
}
