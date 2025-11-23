mod analyzer;
mod output;

use anyhow::{Context, Result};
use chrono::Local;
use clap::{ArgGroup, Parser};
use output::OutputOptions;
use rtsimh::VERSION;
use std::fs;
use std::io::{self, Read};
use std::path::Path;

const GIT_HASH: &str = env!("GIT_HASH");

#[derive(Debug, Parser)]
#[command(
    author,
    version,
    about = "ACMS rtinfo SIMH .tap analyser.",
    disable_colored_help = false
)]
#[command(group(
    ArgGroup::new("binary_visibility")
        .args(["show_binary", "suppress_binary"])
        .multiple(false),
))]
#[command(group(
    ArgGroup::new("ascii_visibility")
        .args(["show_ascii", "suppress_ascii"])
        .multiple(false),
))]
#[command(group(
    ArgGroup::new("label_visibility")
        .args(["show_labels", "suppress_labels"])
        .multiple(false),
))]
struct Cli {
    /// Path to the .tap file (use '-' for stdin)
    #[arg(value_name = "INPUT", default_value = "-")]
    input: String,

    /// Hide all previews unless explicitly re-enabled via --show-* flags
    #[arg(long)]
    summaries_only: bool,

    /// Show binary previews (overrides --summaries-only)
    #[arg(long)]
    show_binary: bool,
    /// Hide binary previews (default)
    #[arg(long)]
    suppress_binary: bool,

    /// Show ASCII/ANSI previews
    #[arg(long)]
    show_ascii: bool,
    /// Hide ASCII/ANSI previews (default)
    #[arg(long)]
    suppress_ascii: bool,

    /// Show 80-byte label previews
    #[arg(long)]
    show_labels: bool,
    /// Hide label previews (default)
    #[arg(long)]
    suppress_labels: bool,
}

impl Cli {
    fn output_options(&self) -> OutputOptions {
        let mut opts = OutputOptions::default();
        if self.summaries_only {
            // start from fully hidden baseline
            opts.show_binary = false;
            opts.show_ascii = false;
            opts.show_labels = false;
        }

        if self.show_binary {
            opts.show_binary = true;
        } else if self.suppress_binary {
            opts.show_binary = false;
        }

        if self.show_ascii {
            opts.show_ascii = true;
        } else if self.suppress_ascii {
            opts.show_ascii = false;
        }

        if self.show_labels {
            opts.show_labels = true;
        } else if self.suppress_labels {
            opts.show_labels = false;
        }

        opts
    }
}

fn main() -> Result<()> {
    // Display header before parsing args so it shows even on errors
    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S");
    println!("========================");
    println!("ACMS rtinfo v{} / {}", VERSION, GIT_HASH);
    println!("Timestamp: {}", timestamp);

    let cli = Cli::parse();

    // Show input path after successful parsing
    let input_path = if cli.input == "-" {
        "stdin".to_string()
    } else {
        std::fs::canonicalize(&cli.input)
            .unwrap_or_else(|_| Path::new(&cli.input).to_path_buf())
            .display()
            .to_string()
    };

    println!("Input: {}", input_path);
    println!("========================");
    println!();

    let data = read_input(&cli.input).context("failed to read input data")?;

    let analysis = analyzer::analyze_bytes(&data);
    let options = cli.output_options();
    let report = output::format_analysis(&analysis, &options);

    println!("{report}");

    Ok(())
}

fn read_input(path: &str) -> Result<Vec<u8>> {
    if path == "-" {
        let mut buffer = Vec::new();
        io::stdin()
            .read_to_end(&mut buffer)
            .context("stdin read failed")?;
        Ok(buffer)
    } else {
        fs::read(path).with_context(|| format!("failed to read file {path}"))
    }
}
