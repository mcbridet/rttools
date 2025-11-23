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
    lines.push(format!(
        "Total files: {}",
        format_with_commas(analysis.totals.files)
    ));
    lines.push(format!(
        "Total records: {}",
        format_with_commas(analysis.totals.records)
    ));
    lines.push(format!(
        "Total data bytes: {}",
        format_with_commas(analysis.totals.data_bytes)
    ));

    if let Some(offset) = analysis.end_of_tape_offset {
        lines.push(format!(
            "End-of-tape marker at offset {}",
            format_with_commas(offset)
        ));
    }

    if let Some(summary) = &analysis.tape_summary {
        lines.push(String::new());
        lines.push("Broad format detection:".to_string());
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
        lines.push("-------------------------".to_string());
        lines.push(format!(
            "File #{}: {} Records ({} bytes)",
            file.file_index,
            format_with_commas(file.records.len()),
            format_with_commas(file.data_bytes)
        ));
        if let Some(msg) = &file.tape_mark_warning {
            lines.push(format!("  Tape mark: {msg}"));
        }

        if let Some(summary) = &file.summary {
            lines.push("  Summary:".to_string());
            lines.extend(summary_lines(summary, "    "));
        }

        let runs = coalesce_record_runs(&file.records, opts);
        for run in runs {
            if run.count == 1 {
                lines.push(format!(
                    "  Record {:04}: {} bytes @ {} [{:?}]",
                    run.start_index,
                    format_with_commas(run.length),
                    format_with_commas(run.start_offset),
                    run.encoding
                ));
                lines.extend(run.body);
            } else {
                lines.push(format!(
                    "  Records {:04}-{:04} ({} records): {} bytes each @ offsets {}..{} [{:?}]",
                    run.start_index,
                    run.end_index,
                    format_with_commas(run.count),
                    format_with_commas(run.length),
                    format_with_commas(run.start_offset),
                    format_with_commas(run.end_offset),
                    run.encoding
                ));
                for body_line in &run.body {
                    lines.push(format!(
                        "{body_line} (repeated {}x)",
                        format_with_commas(run.count)
                    ));
                }
            }
        }
    }

    lines.push("-------------------------".to_string());
    lines.push("End of report.\n".to_string());
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
            vec![format!(
                "[binary data suppressed: {} bytes]",
                format_with_commas(record.length)
            )]
        }
        RecordEncoding::Ascii
        | RecordEncoding::MostlyAscii
        | RecordEncoding::Ansi
        | RecordEncoding::MostlyAnsi
            if !opts.show_ascii =>
        {
            vec![format!(
                "[ascii data suppressed: {} bytes]",
                format_with_commas(record.length)
            )]
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

fn record_body_lines(record: &AnalyzedRecord, opts: &OutputOptions) -> Vec<String> {
    let mut lines = Vec::new();
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
    lines
}

fn coalesce_record_runs(records: &[AnalyzedRecord], opts: &OutputOptions) -> Vec<RecordRun> {
    let mut runs = Vec::new();
    let mut current: Option<RecordRun> = None;

    for record in records {
        let body = record_body_lines(record, opts);
        let mut extends_current = false;
        if let Some(run) = current.as_mut() {
            if run.can_extend(record, &body) {
                run.extend(record);
                extends_current = true;
            }
        }

        if !extends_current {
            if let Some(run) = current.take() {
                runs.push(run);
            }
            current = Some(RecordRun::new(record, body));
        }
    }

    if let Some(run) = current {
        runs.push(run);
    }

    runs
}

fn format_with_commas<T: ToString>(value: T) -> String {
    let mut text = value.to_string();
    let mut idx = text.len() as isize - 3;
    while idx > 0 {
        text.insert(idx as usize, ',');
        idx -= 3;
    }
    text
}

struct RecordRun {
    start_index: usize,
    end_index: usize,
    count: usize,
    start_offset: u64,
    end_offset: u64,
    length: u32,
    encoding: RecordEncoding,
    body: Vec<String>,
}

impl RecordRun {
    fn new(record: &AnalyzedRecord, body: Vec<String>) -> Self {
        Self {
            start_index: record.record_index,
            end_index: record.record_index,
            count: 1,
            start_offset: record.offset,
            end_offset: record.offset,
            length: record.length,
            encoding: record.encoding,
            body,
        }
    }

    fn can_extend(&self, record: &AnalyzedRecord, body: &[String]) -> bool {
        record.length == self.length && record.encoding == self.encoding && *body == self.body
    }

    fn extend(&mut self, record: &AnalyzedRecord) {
        self.end_index = record.record_index;
        self.end_offset = record.offset;
        self.count += 1;
    }
}
