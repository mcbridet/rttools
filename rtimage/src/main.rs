mod reader;
mod utils;

use crate::reader::{TapeEvent, start_reader_thread};
use rtsimh::{SimhTapeWriter, VERSION};
use crate::utils::{make_input_name, make_output_name};
use anyhow::{Context, Result, bail};
use chrono::Local;
use clap::Parser;
use crossbeam_channel::bounded;
use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Read};
use std::path::Path;
use std::thread;

const GIT_HASH: &str = env!("GIT_HASH");

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "Copy magnetic tape to SIMH tape image format.",
    long_about = "rtimage copies data from a magnetic tape device (or file) to a SIMH-compatible tape image file.\nMaintained by the ACMS (Australia Computer Museum Society) and based on timage.c by Natalie & Gwyn (gwyn@arl.army.mil)",
    after_help = "EXAMPLES:\n  \
                  rtimage /dev/nst0 mytape.tap\n  \
                  rtimage nst0 mytape.tap\n  \
                  rtimage - mytape.tap < raw_tape_data.bin"
)]
struct Args {
    /// Input device path (e.g., /dev/nst0, nst0) or "-" for stdin.
    #[arg(value_name = "INPUT")]
    input: String,

    /// Output filename (extension .tap will be added if missing).
    #[arg(value_name = "OUTPUT")]
    output: String,

    /// Maximum number of reattempts when drive returns 0 bytes (Tape Mark) unexpectedly.
    #[arg(long, default_value_t = 100, value_name = "COUNT")]
    max_reattempts: u32,

    /// Force overwrite if output file already exists.
    #[arg(long)]
    ignore_existing: bool,
}

fn main() -> Result<()> {
    // Display header before parsing args so it shows even on errors
    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S");
    println!("========================");
    println!("ACMS rtimage v{} / {}", VERSION, GIT_HASH);

    let args = Args::parse();

    let out_path = make_output_name(&args.output);
    let input_name = make_input_name(&args.input);

    println!("Timestamp: {}", timestamp);
    println!("SCSI Device: {}", input_name);
    println!("Destination: {}", out_path);
    println!("========================");
    println!();

    // Display output path after successful parsing
    let full_output_path = std::fs::canonicalize(&out_path)
        .unwrap_or_else(|_| {
            // If file doesn't exist yet, get absolute path of parent + filename
            let path = Path::new(&out_path);
            if let Some(parent) = path.parent() {
                std::fs::canonicalize(parent)
                    .map(|p| p.join(path.file_name().unwrap()))
                    .unwrap_or_else(|_| path.to_path_buf())
            } else {
                path.to_path_buf()
            }
        })
        .display()
        .to_string();

    // Check if output exists
    if std::path::Path::new(&out_path).exists() && !args.ignore_existing {
        bail!("Output file '{}' already exists.", out_path);
    }

    // Open Output
    let output_file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&out_path)
        .context("Failed to open output file")?;

    let mut tape_writer = SimhTapeWriter::new(BufWriter::new(output_file));

    let mut count = 0;
    let mut bytes = 0;
    let mut reattempts = 0;

    // Loop for reading tape files (separated by Tape Marks)
    loop {
        // Open Input (Re-open for each file on tape)
        let input: Box<dyn Read + Send> = if let Some(ref path) = input_name {
            Box::new(File::open(path).context("Failed to open input device")?)
        } else {
            // Stdin can't be re-opened.
            if count > 0 {
                break; // We already read stdin once.
            }
            Box::new(io::stdin())
        };

        let (sender, receiver) = bounded(2);
        let reader_handle = start_reader_thread(input, sender);

        let mut file_block_count = 0;
        let mut tape_mark_seen = false;

        for event in receiver {
            match event {
                TapeEvent::Data(data) => {
                    bytes += data.len();
                    tape_writer.write_record(&data)?;
                    file_block_count += 1;
                    count += 1;
                    // Reset reattempts on successful read
                    reattempts = 0;
                }
                TapeEvent::TapeMark => {
                    tape_mark_seen = true;
                    break; // End of this tape file
                }
                TapeEvent::Error(e) => {
                    eprintln!("Error reading tape: {}", e);
                    return Err(anyhow::anyhow!(e));
                }
            }
        }

        // Wait for reader to finish
        let _ = reader_handle.join();

        if !tape_mark_seen {
            // Reader exited without TM? (Error or Pipe closed)
            break;
        }

        tape_writer.write_tape_mark()?;

        if file_block_count == 0 {
            // We read 0 blocks and hit a TM.
            if reattempts < args.max_reattempts {
                eprintln!(
                    "\n[Attempt {}/{}] Not receiving any data from drive, retrying...",
                    reattempts + 1,
                    args.max_reattempts
                );
                thread::sleep(std::time::Duration::from_millis(500));
                reattempts += 1;
                continue;
            } else {
                bail!(
                    "Max reattempts ({}) reached. Drive reported 0 bytes repeatedly.",
                    args.max_reattempts
                );
            }
        }
    }

    // Write final Tape Mark (EOT)
    tape_writer.write_tape_mark()?;
    println!("Pulled {} records ({} bytes).", count, bytes);

    Ok(())
}
