#![allow(dead_code)]

pub mod formats;
pub mod reader;
pub mod signature;

use indexmap::IndexSet;
use reader::{SimhTapeBlock, SimhTapeMark, SimhTapeReader, SimhTapeRecord};
use std::io::Cursor;

pub use formats::{
    AnsiLabel, TapeSummary, decode_ansi_label, extract_backup_command, summarize_file_records,
    summarize_tape,
};
pub use signature::{RecordSignature, SignatureDetector};

const MAX_COMMAND_RECORDS: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecordEncoding {
    Empty,
    Ascii,
    MostlyAscii,
    Ansi,
    MostlyAnsi,
    Binary,
}

impl Default for RecordEncoding {
    fn default() -> Self {
        RecordEncoding::Binary
    }
}

#[derive(Debug, Default, Clone)]
pub struct RecordPreview {
    pub hex_lines: Vec<String>,
    pub text_lines: Vec<String>,
    pub previewed_bytes: usize,
}

#[derive(Debug, Clone, Default)]
pub struct AnalyzedRecord {
    pub record_index: usize,
    pub offset: u64,
    pub length: u32,
    pub class: u8,
    pub encoding: RecordEncoding,
    pub label: Option<AnsiLabel>,
    pub signatures: Vec<RecordSignature>,
    pub warnings: Vec<String>,
    pub preview: RecordPreview,
    pub trailing_length: Option<u32>,
}

#[derive(Debug, Clone, Default)]
pub struct TapeFile {
    pub file_index: usize,
    pub records: Vec<AnalyzedRecord>,
    pub summary: Option<TapeSummary>,
    pub data_bytes: u64,
    pub tape_mark_warning: Option<String>,
}

#[derive(Debug, Default, Clone)]
pub struct TapeTotals {
    pub files: usize,
    pub records: usize,
    pub data_bytes: u64,
}

#[derive(Debug, Default, Clone)]
pub struct TapeAnalysis {
    pub filesize: Option<u64>,
    pub files: Vec<TapeFile>,
    pub totals: TapeTotals,
    pub warnings: Vec<String>,
    pub tape_summary: Option<TapeSummary>,
    pub backup_command: Option<String>,
    pub end_of_tape_offset: Option<u64>,
}

impl TapeAnalysis {
    pub fn aggregate_platforms(&self) -> IndexSet<String> {
        let mut platforms = IndexSet::new();
        if let Some(summary) = &self.tape_summary {
            for platform in &summary.platforms {
                platforms.insert(platform.clone());
            }
        }
        platforms
    }
}

pub fn analyze_bytes(bytes: &[u8]) -> TapeAnalysis {
    let mut analysis = TapeAnalysis::default();
    analysis.filesize = Some(bytes.len() as u64);
    let mut reader = SimhTapeReader::new(Cursor::new(bytes));
    let detector = SignatureDetector::default();
    let mut current_file: Option<TapeFile> = None;
    let mut command_records: Vec<Vec<u8>> = Vec::new();

    loop {
        match reader.next_block() {
            Ok(SimhTapeBlock::Record(record)) => {
                let SimhTapeRecord { header, data } = record;
                analysis.totals.records += 1;
                analysis.totals.data_bytes += header.length as u64;

                let file = current_file.get_or_insert_with(|| {
                    analysis.totals.files += 1;
                    TapeFile {
                        file_index: analysis.totals.files,
                        ..Default::default()
                    }
                });

                if command_records.len() < MAX_COMMAND_RECORDS {
                    command_records.push(data.clone());
                }

                let encoding = classify_encoding(&data);
                let preview = build_preview(&data, encoding);
                let label = decode_ansi_label(&data);
                let signatures = detector.detect(&data, header.length);

                let mut analyzed = AnalyzedRecord {
                    record_index: file.records.len() + 1,
                    offset: header.offset,
                    length: header.length,
                    class: header.class,
                    encoding,
                    label,
                    signatures,
                    warnings: Vec::new(),
                    preview,
                    trailing_length: header.trailing_length,
                };

                if header.trailing_length != Some(header.length) {
                    analyzed
                        .warnings
                        .push("Trailing length mismatch".to_string());
                }

                match header.class {
                    0 => {}
                    0x1..=0x6 => analyzed
                        .warnings
                        .push(format!("SIMH private data class 0x{:X}", header.class)),
                    0x8 => analyzed
                        .warnings
                        .push("SIMH class 8 (bad data record)".to_string()),
                    0x9..=0xD => analyzed
                        .warnings
                        .push(format!("SIMH reserved data class 0x{:X}", header.class)),
                    0xE => analyzed
                        .warnings
                        .push("SIMH tape description record (class E)".to_string()),
                    _ => analyzed
                        .warnings
                        .push(format!("SIMH unknown data class 0x{:X}", header.class)),
                }

                file.data_bytes += header.length as u64;
                file.records.push(analyzed);
            }
            Ok(SimhTapeBlock::TapeMark { kind, offset }) => {
                match kind {
                    SimhTapeMark::Single | SimhTapeMark::Double => {
                        if let Some(mut file) = current_file.take() {
                            if matches!(kind, SimhTapeMark::Double) {
                                file.tape_mark_warning =
                                    Some("Double tape mark encountered".to_string());
                            }
                            push_completed_file(&mut analysis, file);
                        } else if matches!(kind, SimhTapeMark::Double) {
                            analysis
                                .warnings
                                .push("Double tape mark observed outside of a file".to_string());
                        }
                    }
                    SimhTapeMark::EndOfTape => {
                        if let Some(file) = current_file.take() {
                            push_completed_file(&mut analysis, file);
                        }
                        analysis.end_of_tape_offset = Some(offset);
                        break;
                    }
                    SimhTapeMark::EraseGap => analysis
                        .warnings
                        .push(format!("Erase gap marker at offset 0x{offset:08X}")),
                    SimhTapeMark::HalfGapForward | SimhTapeMark::HalfGapReverse => analysis
                        .warnings
                        .push(format!("Half-gap marker at offset 0x{offset:08X}")),
                    SimhTapeMark::Private { class, value } => analysis.warnings.push(format!(
                        "SIMH private marker class 0x{class:X} value 0x{value:08X} at offset 0x{offset:08X}"
                    )),
                    SimhTapeMark::Reserved { class, value } => analysis.warnings.push(format!(
                        "SIMH reserved marker class 0x{class:X} value 0x{value:08X} at offset 0x{offset:08X}"
                    )),
                }
            }
            Ok(SimhTapeBlock::EndOfStream) => {
                if let Some(file) = current_file.take() {
                    push_completed_file(&mut analysis, file);
                }
                break;
            }
            Err(err) => {
                analysis.warnings.push(err.to_string());
                if let Some(file) = current_file.take() {
                    push_completed_file(&mut analysis, file);
                }
                break;
            }
        }
    }

    analysis.tape_summary = summarize_tape(&analysis.files);

    if analysis.backup_command.is_none() {
        if let Some(command) = extract_backup_command(&command_records) {
            analysis.backup_command = Some(command);
        }
    }

    analysis
}

fn classify_encoding(data: &[u8]) -> RecordEncoding {
    if data.is_empty() {
        return RecordEncoding::Empty;
    }

    let mut printable = 0usize;
    let mut extended = 0usize;
    for &byte in data {
        if (32..=126).contains(&byte) || [9, 10, 13].contains(&byte) {
            printable += 1;
        } else if (128..=255).contains(&byte) {
            printable += 1;
            extended += 1;
        }
    }

    let total = data.len();
    let printable_pct = (printable as f32 / total as f32) * 100.0;
    let extended_pct = if printable == 0 {
        0.0
    } else {
        (extended as f32 / printable as f32) * 100.0
    };

    if printable_pct > 95.0 {
        if extended_pct < 5.0 {
            RecordEncoding::Ascii
        } else {
            RecordEncoding::Ansi
        }
    } else if printable_pct > 70.0 {
        if extended_pct < 5.0 {
            RecordEncoding::MostlyAscii
        } else {
            RecordEncoding::MostlyAnsi
        }
    } else {
        RecordEncoding::Binary
    }
}

const PREVIEW_BYTES: usize = 64;

fn build_preview(data: &[u8], encoding: RecordEncoding) -> RecordPreview {
    if data.is_empty() {
        return RecordPreview::default();
    }

    let mut preview = RecordPreview::default();
    let limit = data.len().min(PREVIEW_BYTES);
    let printable_encoding = matches!(
        encoding,
        RecordEncoding::Ascii
            | RecordEncoding::MostlyAscii
            | RecordEncoding::Ansi
            | RecordEncoding::MostlyAnsi
    );

    for chunk in data[..limit].chunks(16) {
        let hex = chunk
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join(" ");
        preview.hex_lines.push(format!("    Hex:  {hex}"));

        if printable_encoding {
            let text = chunk
                .iter()
                .map(|&b| match b {
                    9 | 10 | 13 => ' ',
                    32..=126 => b as char,
                    128..=255
                        if matches!(
                            encoding,
                            RecordEncoding::Ansi | RecordEncoding::MostlyAnsi
                        ) =>
                    {
                        b as char
                    }
                    _ => '.',
                })
                .collect::<String>();
            preview.text_lines.push(format!("    Text: {text}"));
        } else {
            preview.text_lines.push("    Text: (binary)".to_string());
        }
    }

    preview.previewed_bytes = limit;
    preview
}

fn push_completed_file(analysis: &mut TapeAnalysis, mut file: TapeFile) {
    if file.summary.is_none() {
        file.summary = summarize_file_records(&file.records);
    }
    analysis.files.push(file);
}
