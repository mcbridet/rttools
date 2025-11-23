#![allow(dead_code)]

use crate::analyzer::{AnalyzedRecord, RecordEncoding, TapeAnalysis, TapeSummary};

#[derive(Debug, Clone, Copy)]
pub struct OutputOptions {
    pub show_binary: bool,
    pub show_ascii: bool,
    pub show_labels: bool,
}

impl Default for OutputOptions {
    fn default() -> Self {
        Self {
            show_binary: false,
            show_ascii: false,
            show_labels: false,
        }
    }
}

pub fn format_analysis(analysis: &TapeAnalysis, opts: &OutputOptions) -> String {
    let mut lines = Vec::new();
    lines.push(format!("Total files: {}", analysis.totals.files));
    lines.push(format!("Total records: {}", analysis.totals.records));
    lines.push(format!("Total data bytes: {}", analysis.totals.data_bytes));

    if let Some(offset) = analysis.end_of_tape_offset {
        lines.push(format!("End-of-tape marker at offset {}", offset));
    }

    if let Some(summary) = &analysis.tape_summary {
        lines.push(String::new());
        lines.push("Tape format detection:".to_string());
        lines.extend(summary_lines(summary, "  "));
    }

    if let Some(command) = &analysis.backup_command {
        lines.push(String::new());
        lines.push("Backup command hint:".to_string());
        lines.push(format!("  {command}"));
    }

    if !analysis.warnings.is_empty() {
        lines.push("Warnings:".to_string());
        for warning in &analysis.warnings {
            lines.push(format!("  - {warning}"));
        }
    }

    for file in &analysis.files {
        lines.push(String::new());
        lines.push(format!(
            "File {} — {} records ({} bytes)",
            file.file_index,
            file.records.len(),
            file.data_bytes
        ));
        if let Some(msg) = &file.tape_mark_warning {
            lines.push(format!("  Tape mark: {msg}"));
        }

        if let Some(summary) = &file.summary {
            lines.push("  Summary:".to_string());
            lines.extend(summary_lines(summary, "    "));
        }

        for record in &file.records {
            lines.push(format!(
                "  Record {:04}: {} bytes @ {} [{:?}]",
                record.record_index, record.length, record.offset, record.encoding
            ));
            if let Some(label) = &record.label {
                lines.push(format!("    Label: {:?}", label));
            }
            if !record.signatures.is_empty() {
                for signature in &record.signatures {
                    lines.push(format!("    → {}", signature.describe_full()));
                }
            }
            for preview_line in record_preview_lines(record, opts) {
                lines.push(format!("    {preview_line}"));
            }
            for warning in &record.warnings {
                lines.push(format!("    Warning: {warning}"));
            }
        }
    }

    lines.join("\n")
}

fn summary_lines(summary: &TapeSummary, indent: &str) -> Vec<String> {
    let mut lines = Vec::new();
    if !summary.platforms.is_empty() {
        let platforms = summary.platforms.iter().cloned().collect::<Vec<_>>();
        lines.push(format!("{indent}Platforms: {}", platforms.join(", ")));
    }
    if !summary.formats.is_empty() {
        let formats = summary.formats.iter().cloned().collect::<Vec<_>>();
        lines.push(format!("{indent}Formats: {}", formats.join(", ")));
    }
    if !summary.details.is_empty() {
        lines.push(format!("{indent}Details:"));
        for detail in &summary.details {
            lines.push(format!("{indent}  - {detail}"));
        }
    }
    if lines.is_empty() {
        lines.push(format!("{indent}(no summary details)"));
    }
    lines
}

pub fn record_preview_lines(record: &AnalyzedRecord, opts: &OutputOptions) -> Vec<String> {
    if record.label.is_some() && !opts.show_labels {
        return vec!["[label preview suppressed]".to_string()];
    }

    match record.encoding {
        RecordEncoding::Binary if !opts.show_binary => {
            vec![format!("[binary data suppressed: {} bytes]", record.length)]
        }
        RecordEncoding::Ascii
        | RecordEncoding::MostlyAscii
        | RecordEncoding::Ansi
        | RecordEncoding::MostlyAnsi
            if !opts.show_ascii =>
        {
            vec![format!("[ascii data suppressed: {} bytes]", record.length)]
        }
        _ => {
            let mut lines = Vec::new();
            for (hex_line, text_line) in record
                .preview
                .hex_lines
                .iter()
                .zip(record.preview.text_lines.iter())
            {
                lines.push(hex_line.clone());
                lines.push(text_line.clone());
            }
            lines
        }
    }
}
