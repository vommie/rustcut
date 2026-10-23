//! Command line of the `rustcut` binary (clap).

use std::path::PathBuf;

use clap::Parser;

/// Local video cutter: mark sections, apply filters, render through a queue, tag and rate in RustView.
#[derive(Debug, Clone, Parser)]
#[command(name = "rustcut", version, about)]
pub struct Cli {
    /// Videos to open; the first one is loaded into the editor.
    #[arg(value_name = "FILE")]
    pub files: Vec<PathBuf>,

    /// Keep settings, data and logs below this folder instead of the XDG folders (isolated test profile).
    #[arg(long, value_name = "DIR")]
    pub home: Option<PathBuf>,
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory as _;

    use super::*;

    #[test]
    fn command_line_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn parses_files_and_home() {
        let cli = Cli::parse_from(["rustcut", "--home", "/tmp/p", "a.mkv", "b.mp4"]);
        assert_eq!(cli.home, Some(PathBuf::from("/tmp/p")));
        assert_eq!(cli.files, vec![PathBuf::from("a.mkv"), PathBuf::from("b.mp4")]);
    }
}
