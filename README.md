# ✇ rttools - ACMS Tape Tools

Tools for working with magnetic tape devices and analysing SIMH tapes. Largely designed around 9-track tapes for now.

SIMH `.tap` specs can be found in `./docs`.

## Projects

### 🎞️ rtimage

**Magnetic Tape to SIMH Image Converter**

A command-line tool that copies data from physical magnetic tape devices (or files) to SIMH-compatible tape image files (`.tap` format).

**Features:** Multi-threaded and double-buffered reading, automatic tape mark detection, configurable retry logic.

**Permissions**:

Ideally the tool isn't run as **root**. For this to happen...
- The user that runs this tool must have access to tape devices. On Linux, you can do this (usually) by adding them to the `tape` group and logging out/in before running the tool.
- The tool will attempt to capture tape-related **kernel messages** such as medium errors, from `journalctl` or `dmesg`. If you do not see any output, either the tape drive is working great, or you must allow the user to see kernel messages. To do so, create the file `/etc/sysctl.d/99-dmesg-restrict.conf` with the following content: `kernel.dmesg_restrict = 0`. Run `sudo sysctl --system` to immediately apply the changes.



**Usage:**
```bash
# Read from tape device
rtimage /dev/nst0 output.tap

# Read from device shorthand
rtimage nst0 output.tap

# Read from stdin
rtimage - output.tap < raw_tape_data.bin

# With custom retry attempts
rtimage /dev/nst0 output.tap --max-reattempts 200
```

**rtimage** is heavily based on [`timage.c`](http://inwap.com/pdp10/usenet/timage.c) by **Natalie & Gwyn** ([gwyn@arl.army.mil](gwyn@arl.army.mil)).

---

### 🔍 rtinfo

**Tape Image Analyzer**

A CLI for analysing SIMH tape images, providing detailed information about tape structure, record formats, ANSI labels, and data signatures.

**Features:** Detects SIMH Extended Format markers and class bits, decodes ANSI tape labels, identifies record signatures, and offers configurable output verbosity.

**Usage:**
```bash
# Analyze tape image with default settings
rtinfo mytape.tap

# Show summaries only (minimal output)
rtinfo --summaries-only mytape.tap

# Enable specific previews
rtinfo --show-ascii --show-labels mytape.tap

# Read from stdin
rtinfo - < mytape.tap
```

**CLI Options:**
- `--summaries-only`: Hide all previews unless explicitly re-enabled.
- `--show-binary` / `--suppress-binary`: Control binary field previews.
- `--show-ascii` / `--suppress-ascii`: Control ASCII/ANSI field previews.
- `--show-labels` / `--suppress-labels`: Control 80-byte label previews.

---

### 📚 rtsimh

**SIMH Format Library**

A shared Rust library providing core functionality for working with SIMH tape image formats. Implements the SIMH Extended Format specification with full support for class bits, metadata markers (tape marks, erase gaps, half-gaps, end-of-medium), and class-aware record framing.

**Library Usage:**
```rust
use rtsimh::SimhTapeWriter;
use std::fs::File;
use std::io::BufWriter;

let file = File::create("output.tap")?;
let mut writer = SimhTapeWriter::new(BufWriter::new(file));

// Write a good data record (class 0)
writer.write_record(b"Hello, tape!")?;

// Write a bad data record (class 8)
writer.write_bad_record(b"Corrupted data")?;

// Write a tape mark
writer.write_tape_mark()?;

// Write end of medium marker
writer.write_end_of_medium()?;
```

---

## Building

```bash
# Build all projects
cargo build --release

# Build specific project
cargo build --release -p rtimage

# Run tests
cargo test --workspace
```