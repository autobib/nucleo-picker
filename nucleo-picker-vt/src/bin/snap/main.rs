mod report;

use std::error::Error;
use std::fs::{self, File};
use std::io;
use std::io::BufWriter;
use std::io::Write;
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use nucleo_picker_vt::snap::Snap;

#[derive(Parser)]
struct Args {
    #[command(subcommand)]
    command: Command,
    /// Write output to a file instead of STDOUT.
    #[arg(short, long, global = true)]
    out: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Command {
    /// Generate a diff report comparing `.snap` and `.snap.new` files.
    Diff {
        /// The directory containing the snapshots.
        snapshot_dir: PathBuf,
    },
    /// Generate a summary report of all `.snap` files.
    Summary {
        /// The directory containing the snapshots.
        snapshot_dir: PathBuf,
    },
    /// Generate a SVG image of a single `.snap` or `.snap.new` file.
    View {
        /// The path to the snap file.
        path: PathBuf,
    },
}

fn main() -> Result<(), Box<dyn Error>> {
    let Args { command, out } = Args::parse();
    if let Some(path) = out {
        run_subcommand(command, BufWriter::new(File::create(path)?))
    } else {
        run_subcommand(command, io::stdout().lock())
    }
}

fn run_subcommand<W: Write>(cmd: Command, mut output: W) -> Result<(), Box<dyn Error>> {
    match cmd {
        Command::Diff { snapshot_dir } => report::write_diff(&snapshot_dir, &mut output)?,
        Command::Summary { snapshot_dir } => report::write_summary(&snapshot_dir, &mut output)?,
        Command::View { path } => {
            let snapshot = fs::read_to_string(path)?;
            let Snap { header: _, pane } = snapshot.parse()?;
            pane.write_svg(&mut output)?;
        }
    }
    output.flush()?;
    Ok(())
}
