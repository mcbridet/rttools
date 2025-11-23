use super::{AnalyzedRecord, RecordEncoding, RecordSignature, TapeFile};
use indexmap::IndexSet;
use std::collections::HashMap;
use std::str;

#[derive(Debug, Clone)]
pub enum AnsiLabel {
    Volume {
        serial: String,
        owner: String,
    },
    FileHeader1 {
        file: String,
        file_set: String,
        created: String,
    },
    FileHeader2 {
        record_format: String,
        block_len: String,
        record_len: String,
    },
    EndOfFile {
        blocks: String,
        file: String,
    },
    EndOfVolume {
        blocks: String,
        file: String,
    },
    UserHeader {
        id: String,
        payload: String,
    },
    UserTrailer {
        id: String,
    },
    Raw(String),
}

impl AnsiLabel {
    pub fn id(&self) -> &str {
        match self {
            AnsiLabel::Volume { .. } => "VOL1",
            AnsiLabel::FileHeader1 { .. } => "HDR1",
            AnsiLabel::FileHeader2 { .. } => "HDR2",
            AnsiLabel::EndOfFile { .. } => "EOF1",
            AnsiLabel::EndOfVolume { .. } => "EOV1",
            AnsiLabel::UserHeader { id, .. }
            | AnsiLabel::UserTrailer { id }
            | AnsiLabel::Raw(id) => id,
        }
    }
}

const LABEL_LENGTH: usize = 80;

fn trim_ascii(bytes: &[u8]) -> String {
    let text = bytes
        .iter()
        .map(|&b| if b.is_ascii() { b } else { b'.' })
        .collect::<Vec<_>>();
    String::from_utf8_lossy(&text).trim().to_string()
}

pub fn decode_ansi_label(bytes: &[u8]) -> Option<AnsiLabel> {
    if bytes.len() != LABEL_LENGTH {
        return None;
    }

    let id = str::from_utf8(&bytes[..4]).ok()?.trim().to_string();

    match id.as_str() {
        "VOL1" => {
            let serial = trim_ascii(&bytes[4..10]);
            let owner = trim_ascii(&bytes[37..51]);
            Some(AnsiLabel::Volume { serial, owner })
        }
        "HDR1" => {
            let file = trim_ascii(&bytes[4..21]);
            let file_set = trim_ascii(&bytes[21..27]);
            let created = trim_ascii(&bytes[41..47]);
            Some(AnsiLabel::FileHeader1 {
                file,
                file_set,
                created,
            })
        }
        "HDR2" => {
            let record_format = trim_ascii(&bytes[4..5]);
            let block_len = trim_ascii(&bytes[5..10]);
            let record_len = trim_ascii(&bytes[10..15]);
            Some(AnsiLabel::FileHeader2 {
                record_format,
                block_len,
                record_len,
            })
        }
        "EOF1" => {
            let file = trim_ascii(&bytes[4..21]);
            let blocks = trim_ascii(&bytes[54..60]);
            Some(AnsiLabel::EndOfFile { file, blocks })
        }
        "EOV1" => {
            let file = trim_ascii(&bytes[4..21]);
            let blocks = trim_ascii(&bytes[54..60]);
            Some(AnsiLabel::EndOfVolume { file, blocks })
        }
        _ => {
            if id.starts_with("UHL") {
                let payload = trim_ascii(&bytes[4..]);
                Some(AnsiLabel::UserHeader { id, payload })
            } else if id.starts_with("UTL") {
                Some(AnsiLabel::UserTrailer { id })
            } else {
                Some(AnsiLabel::Raw(id))
            }
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct TapeSummary {
    pub platforms: IndexSet<String>,
    pub formats: IndexSet<String>,
    pub details: Vec<String>,
}

impl TapeSummary {
    pub fn add_platform(&mut self, platform: impl Into<String>) {
        let platform = platform.into();
        if !platform.is_empty() {
            self.platforms.insert(platform);
        }
    }

    pub fn add_format(&mut self, format: impl Into<String>) {
        let format = format.into();
        if !format.is_empty() {
            self.formats.insert(format);
        }
    }

    pub fn add_detail(&mut self, detail: impl Into<String>) {
        let detail = detail.into();
        if !detail.is_empty() && !self.details.contains(&detail) {
            self.details.push(detail);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.platforms.is_empty() && self.formats.is_empty() && self.details.is_empty()
    }

    pub fn merge(&mut self, other: &TapeSummary) {
        for platform in &other.platforms {
            self.platforms.insert(platform.clone());
        }
        for format in &other.formats {
            self.formats.insert(format.clone());
        }
        for detail in &other.details {
            self.add_detail(detail.clone());
        }
    }
}

pub fn summarize_file_records(records: &[AnalyzedRecord]) -> Option<TapeSummary> {
    if records.is_empty() {
        return None;
    }

    let mut summary = TapeSummary::default();
    let mut data_lengths = Vec::new();

    for record in records {
        if let Some(label) = &record.label {
            summary.add_platform("ANSI/ISO Standard Labeled Tape");
            match label {
                AnsiLabel::FileHeader1 { file, .. } if !file.is_empty() => {
                    summary.add_detail(format!("HDR1 declares file '{file}'"));
                    let upper = file.to_uppercase();
                    if upper.ends_with(".BCK") || upper.contains(".BCK") || upper.contains(".BAK") {
                        summary.add_format("DEC BACKUP save set (.BCK)");
                    }
                    if upper.ends_with(".SAV") {
                        summary.add_format("DEC save image (.SAV)");
                    }
                }
                _ => {}
            }
        } else {
            data_lengths.push(record.length);
        }
    }

    for record in records
        .iter()
        .filter(|record| record.label.is_none())
        .take(3)
    {
        add_signatures_to_summary(&mut summary, &record.signatures);
    }

    if !data_lengths.is_empty() {
        if let Some(common) = common_block_size(&data_lengths) {
            summary.add_detail(format!("Predominant data block size: {common} bytes"));
        }
    }

    if summary.is_empty() {
        None
    } else {
        Some(summary)
    }
}

pub fn summarize_tape(files: &[TapeFile]) -> Option<TapeSummary> {
    if files.is_empty() {
        return None;
    }

    let mut summary = TapeSummary::default();
    let mut label_count = 0usize;

    for file in files {
        if let Some(file_summary) = &file.summary {
            summary.merge(file_summary);
        }
        label_count += file.records.iter().filter(|r| r.label.is_some()).count();
    }

    if label_count > 0 {
        summary.add_detail(format!(
            "Tape includes {label_count} ANSI/ISO label record{}",
            if label_count == 1 { "" } else { "s" }
        ));
        summary.add_platform("ANSI/ISO Standard Labeled Tape");
    }

    if summary.formats.is_empty() {
        if let Some(record) = files
            .iter()
            .flat_map(|file| file.records.iter())
            .find(|record| record.label.is_none())
        {
            add_signatures_to_summary(&mut summary, &record.signatures);
            if summary.formats.is_empty() {
                summary.add_format(format!(
                    "Content appears {}",
                    encoding_label(record.encoding)
                ));
            }
        }
    }

    if summary.is_empty() {
        None
    } else {
        Some(summary)
    }
}

pub fn extract_backup_command(records: &[Vec<u8>]) -> Option<String> {
    for record in records {
        if record.len() == LABEL_LENGTH {
            if let Some(command) = parse_label_for_command(record) {
                return Some(command);
            }
        }
    }

    for record in records {
        if let Some(command) = parse_data_block_for_command(record) {
            return Some(command);
        }
    }

    None
}

fn add_signatures_to_summary(summary: &mut TapeSummary, signatures: &[RecordSignature]) {
    for signature in signatures {
        if let Some(platform) = &signature.platform {
            summary.add_platform(platform.clone());
        }
        if let Some(format) = &signature.format {
            summary.add_format(format.clone());
        }
        summary.add_detail(signature.summary_detail());
    }
}

fn common_block_size(lengths: &[u32]) -> Option<u32> {
    let mut counts: HashMap<u32, usize> = HashMap::new();
    for &len in lengths {
        *counts.entry(len).or_insert(0) += 1;
    }
    counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(len, _)| len)
}

fn parse_label_for_command(record: &[u8]) -> Option<String> {
    let id = str::from_utf8(&record[0..4]).ok()?.trim().to_string();
    if id.starts_with("UHL") {
        let content = String::from_utf8_lossy(&record[4..]).trim().to_string();
        if looks_like_command(&content) {
            return Some(content);
        }
    }

    if id == "VOL1" && record.len() >= 80 {
        let comment = String::from_utf8_lossy(&record[51..80]).trim().to_string();
        if looks_like_command(&comment) {
            return Some(comment);
        }
    }

    None
}

fn parse_data_block_for_command(record: &[u8]) -> Option<String> {
    if record.len() <= 512 {
        return None;
    }

    let window = record.len().min(1024);
    let haystack = &record[..window];
    if let Some(idx) = find_subsequence(haystack, b"BACKUP/") {
        if idx >= 2 {
            let len_bytes = &haystack[idx - 2..idx];
            let potential_len = u16::from_le_bytes([len_bytes[0], len_bytes[1]]) as usize;
            if (50..500).contains(&potential_len) {
                let end = (idx + potential_len).min(record.len());
                if let Ok(command) = std::str::from_utf8(&record[idx..end]) {
                    let trimmed = command.trim();
                    if looks_like_command(trimmed) {
                        return Some(trimmed.to_string());
                    }
                }
            }
        }

        let end_limit = (idx + 400).min(record.len());
        let mut end_idx = idx;
        for (i, &byte) in record[idx..end_limit].iter().enumerate() {
            if !is_printable_ascii(byte) {
                break;
            }
            end_idx = idx + i + 1;
        }
        if end_idx == idx {
            return None;
        }
        if let Ok(command) = std::str::from_utf8(&record[idx..end_idx]) {
            let trimmed = command.trim();
            if trimmed.len() > 30 && trimmed.matches('/').count() >= 3 {
                return Some(trimmed.to_string());
            }
        }
    }

    None
}

fn looks_like_command(text: &str) -> bool {
    if text.len() < 6 {
        return false;
    }
    let upper = text.to_ascii_uppercase();
    const KEYWORDS: [&str; 6] = ["BACKUP", "COMMAND", "LOG", "VERIFY", "/", "$"];
    KEYWORDS.iter().any(|keyword| upper.contains(keyword))
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn is_printable_ascii(byte: u8) -> bool {
    byte == b'\t' || byte == b'\n' || byte == b'\r' || (32..=126).contains(&byte)
}

fn encoding_label(encoding: RecordEncoding) -> &'static str {
    match encoding {
        RecordEncoding::Empty => "empty",
        RecordEncoding::Ascii => "ASCII",
        RecordEncoding::MostlyAscii => "mostly ASCII",
        RecordEncoding::Ansi => "ANSI/Extended ASCII",
        RecordEncoding::MostlyAnsi => "mostly ANSI",
        RecordEncoding::Binary => "binary",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer::RecordPreview;

    #[test]
    fn decode_vol1_label_extracts_fields() {
        let mut bytes = vec![b' '; LABEL_LENGTH];
        bytes[..4].copy_from_slice(b"VOL1");
        bytes[4..10].copy_from_slice(b"TAPE01");
        bytes[37..41].copy_from_slice(b"ACMS");

        let label = decode_ansi_label(&bytes).expect("label parsed");
        match label {
            AnsiLabel::Volume { serial, owner } => {
                assert_eq!(serial, "TAPE01");
                assert_eq!(owner, "ACMS");
            }
            _ => panic!("unexpected label variant"),
        }
    }

    #[test]
    fn summarize_file_records_collects_platforms_formats() {
        let hdr_label = AnsiLabel::FileHeader1 {
            file: "BACKUP.BCK".to_string(),
            file_set: "001".to_string(),
            created: "250101".to_string(),
        };

        let labeled = sample_record(Some(hdr_label), Vec::new());
        let signatures = vec![
            RecordSignature::new("gzip", "GZIP compressed file")
                .with_format("GZIP")
                .with_platform("Unix/Linux"),
        ];
        let data_record = sample_record(None, signatures);

        let summary = summarize_file_records(&[labeled, data_record]).expect("summary present");
        assert!(summary.platforms.contains("ANSI/ISO Standard Labeled Tape"));
        assert!(summary.platforms.contains("Unix/Linux"));
        assert!(summary.formats.contains("GZIP"));
        assert!(
            summary
                .details
                .iter()
                .any(|detail| detail.contains("HDR1 declares file"))
        );
    }

    #[test]
    fn extract_backup_command_prefers_uhl_text() {
        let mut uhl = vec![b' '; LABEL_LENGTH];
        uhl[..3].copy_from_slice(b"UHL");
        uhl[3] = b'1';
        let cmd = b"BACKUP/IMAGE DUA0: DUA1:/SAVE";
        uhl[4..4 + cmd.len()].copy_from_slice(cmd);

        let command = extract_backup_command(&vec![uhl]).expect("command found");
        assert!(command.starts_with("BACKUP/IMAGE"));
        assert!(command.contains("DUA1"));
    }

    fn sample_record(label: Option<AnsiLabel>, signatures: Vec<RecordSignature>) -> AnalyzedRecord {
        AnalyzedRecord {
            record_index: 1,
            offset: 0,
            length: 80,
            class: 0,
            encoding: RecordEncoding::Ascii,
            label,
            signatures,
            warnings: Vec::new(),
            preview: RecordPreview::default(),
            trailing_length: Some(80),
        }
    }
}
