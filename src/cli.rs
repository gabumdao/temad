use clap::{Parser, Subcommand};
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
}
