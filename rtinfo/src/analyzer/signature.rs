use std::cmp::min;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordSignature {
    pub tag: String,
    pub description: String,
    pub format: Option<String>,
    pub platform: Option<String>,
    pub confidence: String,
    pub details: Option<String>,
}

impl RecordSignature {
    pub fn new(tag: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            tag: tag.into(),
            description: description.into(),
            format: None,
            platform: None,
            confidence: "medium".to_string(),
            details: None,
        }
    }

    pub fn with_format(mut self, fmt: impl Into<String>) -> Self {
        self.format = Some(fmt.into());
        self
    }

    pub fn with_platform(mut self, platform: impl Into<String>) -> Self {
        self.platform = Some(platform.into());
        self
    }

    pub fn with_confidence(mut self, confidence: impl Into<String>) -> Self {
        self.confidence = confidence.into();
        self
    }

    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        self.details = Some(details.into());
        self
    }

    pub fn summary_detail(&self) -> String {
        let mut detail = self.description.clone();
        if !self.confidence.is_empty() {
            detail.push_str(&format!(" (confidence: {})", self.confidence));
        }
        if let Some(extra) = &self.details {
            if !extra.is_empty() {
                detail.push_str(&format!(" - {}", extra));
            }
        }
        detail
    }

    pub fn describe_full(&self) -> String {
        let mut line = self.description.clone();
        if !self.confidence.is_empty() {
            line.push_str(&format!(" [{}]", self.confidence));
        }

        let mut extras = Vec::new();
        if let Some(fmt) = &self.format {
            if !fmt.is_empty() {
                extras.push(fmt.clone());
            }
        }
        if let Some(platform) = &self.platform {
            if !platform.is_empty() {
                extras.push(platform.clone());
            }
        }
        if !extras.is_empty() {
            line.push_str(&format!(" -> {}", extras.join(" / ")));
        }

        if let Some(extra) = &self.details {
            if !extra.is_empty() {
                line.push_str(&format!(" - {}", extra));
            }
        }

        line
    }
}

#[derive(Default)]
pub struct SignatureDetector;

impl SignatureDetector {
    pub fn detect(&self, data: &[u8], declared_length: u32) -> Vec<RecordSignature> {
        let mut signatures = Vec::new();

        self.detect_magic_prefixes(data, &mut signatures);
        self.detect_legacy_compression(data, &mut signatures);
        self.detect_tar(data, &mut signatures);
        self.detect_cpio(data, &mut signatures);
        self.detect_pdp11_formats(data, &mut signatures);
        self.detect_vms_backup(data, &mut signatures);
        self.detect_dec_bru(data, &mut signatures);
        self.detect_pdp11_backup(data, &mut signatures);
        self.detect_unix_dump(data, &mut signatures);
        self.detect_afio(data, &mut signatures);
        self.detect_qic(data, &mut signatures);
        self.detect_windows_backup(data, &mut signatures);
        self.detect_novell_sms(data, &mut signatures);
        self.detect_ibm_standard_labels(data, &mut signatures);
        self.detect_ltfs(data, &mut signatures);
        self.detect_mxf(data, &mut signatures);
        self.detect_block_size(declared_length, &mut signatures);

        if signatures.is_empty() {
            self.detect_text(data, &mut signatures);
        }

        signatures
    }

    fn detect_legacy_compression(&self, data: &[u8], signatures: &mut Vec<RecordSignature>) {
        if data.len() >= 2 {
            let first = data[0];
            let second = data[1];
            if matches!((first, second), (0x1f, 0x9d) | (0x1f, 0xa0)) {
                signatures.push(
                    RecordSignature::new("compress-z", "Unix compress (.Z) file")
                        .with_format("compress (.Z)")
                        .with_platform("Unix/System V")
                        .with_confidence("high"),
                );
            }

            if first == 0x1a && (1..=8).contains(&second) {
                signatures.push(
                    RecordSignature::new("arc", "ARC/PKPAK archive header")
                        .with_format("ARC archive")
                        .with_platform("MS-DOS / CP/M")
                        .with_confidence("medium"),
                );
            }

            if first == 0x60 && second == 0xea {
                signatures.push(
                    RecordSignature::new("arj", "ARJ archive header")
                        .with_format("ARJ archive")
                        .with_platform("MS-DOS / OS/2")
                        .with_confidence("high"),
                );
            }
        }

        if data.len() >= 4 {
            if &data[0..4] == b"ZOO " {
                signatures.push(
                    RecordSignature::new("zoo", "ZOO archive header")
                        .with_format("ZOO archive")
                        .with_platform("MS-DOS")
                        .with_confidence("medium"),
                );
            } else if &data[0..4] == b"SIT!" {
                signatures.push(
                    RecordSignature::new("stuffit", "StuffIt archive")
                        .with_format("StuffIt archive")
                        .with_platform("Classic Mac OS")
                        .with_confidence("high"),
                );
            } else if &data[0..4] == b"MSCF" {
                signatures.push(
                    RecordSignature::new("cab", "Microsoft Cabinet (CAB) file")
                        .with_format("Microsoft Cabinet (CAB)")
                        .with_platform("Windows 3.x/95/NT")
                        .with_confidence("high"),
                );
            } else if &data[0..4] == b"SZDD" {
                signatures.push(
                    RecordSignature::new("szdd", "Microsoft Compress (SZDD) file")
                        .with_format("Microsoft Compress (.??_)")
                        .with_platform("MS-DOS / Windows")
                        .with_confidence("medium")
                        .with_details("Can be expanded with the 'expand' utility"),
                );
            }
        }

        if data.len() >= 7
            && data[2] == b'-'
            && data[3] == b'l'
            && matches!(data[4], b'h' | b'z')
            && data[5].is_ascii_alphanumeric()
            && data[6] == b'-'
        {
            signatures.push(
                RecordSignature::new("lha", "LHA/LZH archive header")
                    .with_format("LHA/LZH archive")
                    .with_platform("MS-DOS / Amiga")
                    .with_confidence("medium")
                    .with_details("Header marker '-lh?-' starts at offset 2"),
            );
        }
    }

    fn detect_magic_prefixes(&self, data: &[u8], signatures: &mut Vec<RecordSignature>) {
        struct Magic {
            magic: &'static [u8],
            tag: &'static str,
            description: &'static str,
            format: Option<&'static str>,
            platform: Option<&'static str>,
            confidence: &'static str,
        }

        const MAGIC_PREFIXES: &[Magic] = &[
            Magic {
                magic: b"\x1f\x8b",
                tag: "gzip",
                description: "GZIP compressed file",
                format: Some("GZIP"),
                platform: Some("Unix/Linux"),
                confidence: "high",
            },
            Magic {
                magic: b"BZ",
                tag: "bzip2",
                description: "BZIP2 compressed file",
                format: Some("BZIP2"),
                platform: Some("Unix/Linux"),
                confidence: "high",
            },
            Magic {
                magic: b"!<arch>\n",
                tag: "ar",
                description: "Unix ar archive",
                format: Some("ar archive"),
                platform: Some("Unix/Linux"),
                confidence: "high",
            },
            Magic {
                magic: b"PK\x03\x04",
                tag: "zip",
                description: "ZIP archive",
                format: Some("ZIP"),
                platform: None,
                confidence: "high",
            },
            Magic {
                magic: b"Rar!",
                tag: "rar",
                description: "RAR archive",
                format: Some("RAR"),
                platform: None,
                confidence: "high",
            },
            Magic {
                magic: b"7z\xbc\xaf'\x1c",
                tag: "7zip",
                description: "7-Zip archive",
                format: Some("7-Zip"),
                platform: None,
                confidence: "high",
            },
            Magic {
                magic: b"\x7fELF",
                tag: "elf",
                description: "ELF executable",
                format: Some("ELF"),
                platform: Some("Unix/Linux"),
                confidence: "high",
            },
            Magic {
                magic: b"\x89PNG",
                tag: "png",
                description: "PNG image",
                format: Some("PNG"),
                platform: None,
                confidence: "high",
            },
            Magic {
                magic: b"GIF87a",
                tag: "gif87",
                description: "GIF image (87a)",
                format: Some("GIF"),
                platform: None,
                confidence: "high",
            },
            Magic {
                magic: b"GIF89a",
                tag: "gif89",
                description: "GIF image (89a)",
                format: Some("GIF"),
                platform: None,
                confidence: "high",
            },
            Magic {
                magic: b"\xff\xd8\xff",
                tag: "jpeg",
                description: "JPEG image",
                format: Some("JPEG"),
                platform: None,
                confidence: "high",
            },
            Magic {
                magic: b"BM",
                tag: "bmp",
                description: "Bitmap image",
                format: Some("BMP"),
                platform: None,
                confidence: "high",
            },
            Magic {
                magic: b"%PDF",
                tag: "pdf",
                description: "PDF document",
                format: Some("PDF"),
                platform: None,
                confidence: "high",
            },
            Magic {
                magic: b"<!DO",
                tag: "doctype",
                description: "HTML/XML document",
                format: Some("HTML"),
                platform: None,
                confidence: "medium",
            },
            Magic {
                magic: b"<html",
                tag: "html",
                description: "HTML document",
                format: Some("HTML"),
                platform: None,
                confidence: "medium",
            },
            Magic {
                magic: b"<?xml",
                tag: "xml",
                description: "XML document",
                format: Some("XML"),
                platform: None,
                confidence: "medium",
            },
            Magic {
                magic: b"#!",
                tag: "shebang",
                description: "Script with shebang",
                format: Some("Script"),
                platform: None,
                confidence: "medium",
            },
        ];

        for magic in MAGIC_PREFIXES {
            if data.starts_with(magic.magic) {
                let sig = RecordSignature::new(magic.tag, magic.description)
                    .with_confidence(magic.confidence);
                push_signature(signatures, sig, magic.format, magic.platform);
            }
        }
    }

    fn detect_tar(&self, data: &[u8], signatures: &mut Vec<RecordSignature>) {
        if data.len() < 512 {
            return;
        }
        let magic = &data[257..263];
        if magic == b"ustar\0" || &data[257..265] == b"ustar  \0" {
            signatures.push(
                RecordSignature::new("tar-ustar", "POSIX tar header (ustar)")
                    .with_format("tar archive")
                    .with_platform("Unix/Linux")
                    .with_confidence("high"),
            );
        } else {
            let name_field = &data[0..100];
            let mode_field = &data[100..108];
            let size_field = &data[124..136];
            if name_field.iter().all(|b| *b == 0 || (32..=126).contains(b))
                && mode_field.iter().all(|b| *b == 0 || (32..=126).contains(b))
                && size_field
                    .iter()
                    .all(|b| b.is_ascii_digit() || *b == 0 || *b == b' ')
            {
                signatures.push(
                    RecordSignature::new("tar-legacy", "Tar header (V7 style)")
                        .with_format("tar archive")
                        .with_platform("Unix/Linux")
                        .with_confidence("medium"),
                );
            }
        }
    }

    fn detect_cpio(&self, data: &[u8], signatures: &mut Vec<RecordSignature>) {
        if data.len() >= 6 {
            match &data[0..6] {
                b"070701" | b"070702" => signatures.push(
                    RecordSignature::new("cpio-newc", "CPIO archive header (new ASCII)")
                        .with_format("CPIO archive")
                        .with_platform("Unix/Linux")
                        .with_confidence("high"),
                ),
                b"070707" => signatures.push(
                    RecordSignature::new("cpio-old", "CPIO archive header (old ASCII)")
                        .with_format("CPIO archive")
                        .with_platform("Unix/Linux")
                        .with_confidence("medium"),
                ),
                _ => {}
            }
        }

        if data.len() >= 2 {
            let first_two = &data[0..2];
            if matches!(
                first_two,
                b"\x71\xc7" | b"\xc7\x71" | b"\xc7\x70" | b"\x70\xc7"
            ) {
                signatures.push(
                    RecordSignature::new("cpio-binary", "CPIO archive header (binary)")
                        .with_format("CPIO archive")
                        .with_platform("Unix/Linux")
                        .with_confidence("medium"),
                );
            }
        }
    }

    fn detect_pdp11_formats(&self, data: &[u8], signatures: &mut Vec<RecordSignature>) {
        const PDP11_SIGNATURES: &[(&[u8], &str, &str, &str, &str)] = &[
            (
                b"\x01\x07",
                "pdp11-omagic",
                "PDP-11 a.out executable (OMAGIC)",
                "PDP-11",
                "high",
            ),
            (
                b"\x01\x08",
                "pdp11-nmagic",
                "PDP-11 a.out executable (NMAGIC)",
                "PDP-11",
                "high",
            ),
            (
                b"\x01\x0b",
                "pdp11-zmagic",
                "PDP-11 a.out executable (ZMAGIC)",
                "PDP-11",
                "high",
            ),
            (
                b"\x01\x0c",
                "pdp11-qmagic",
                "PDP-11 a.out executable (QMAGIC)",
                "PDP-11",
                "high",
            ),
            (
                b"\x02\x07",
                "pdp11-archive",
                "PDP-11 archive/library",
                "PDP-11",
                "medium",
            ),
        ];

        for (magic, tag, description, platform, confidence) in PDP11_SIGNATURES {
            if data.starts_with(magic) {
                signatures.push(
                    RecordSignature::new(*tag, *description)
                        .with_format(*description)
                        .with_platform(*platform)
                        .with_confidence(*confidence),
                );
            }
        }
    }

    fn detect_vms_backup(&self, data: &[u8], signatures: &mut Vec<RecordSignature>) {
        if data.len() < 512 {
            return;
        }
        let header = &data[..512];
        if header[..120].windows(6).any(|w| w == b"BACKUP")
            && header[..256].windows(7).any(|w| w == b"SAVE SET")
        {
            signatures.push(
                RecordSignature::new("vms-backup", "VMS BACKUP save set block")
                    .with_format("VMS BACKUP save set")
                    .with_platform("OpenVMS / VAX/VMS")
                    .with_confidence("high"),
            );
            return;
        }

        if matches!(&header[..2], b"\x01\x00" | b"\x00\x01")
            && header[4..12].iter().all(|&b| b == 0x00)
        {
            signatures.push(
                RecordSignature::new("vms-backup-heur", "Possible VMS BACKUP block")
                    .with_format("VMS BACKUP save set")
                    .with_platform("OpenVMS / VAX/VMS")
                    .with_confidence("medium")
                    .with_details("Matches BACKUP block prologue pattern"),
            );
        }
    }

    fn detect_dec_bru(&self, data: &[u8], signatures: &mut Vec<RecordSignature>) {
        if data.len() >= 64 && data[..64].windows(3).any(|w| w == b"BRU") {
            signatures.push(
                RecordSignature::new("dec-bru", "DEC BRU save set block")
                    .with_format("DEC BRU save set")
                    .with_platform("RSX-11 / RSTS/E / VMS")
                    .with_confidence("low")
                    .with_details("\"BRU\" marker within first 64 bytes"),
            );
        }
    }

    fn detect_pdp11_backup(&self, data: &[u8], signatures: &mut Vec<RecordSignature>) {
        if data.len() >= 32 && matches!(data.get(0), Some(1..=4)) && data.get(1) == Some(&0x00) {
            signatures.push(
                RecordSignature::new("pdp11-backup", "PDP-11 BACKUP save set block")
                    .with_format("PDP-11 BACKUP save set")
                    .with_platform("RSTS/E or RT-11")
                    .with_confidence("medium"),
            );
        }
    }

    fn detect_unix_dump(&self, data: &[u8], signatures: &mut Vec<RecordSignature>) {
        if data.len() < 28 {
            return;
        }
        let magic = u32::from_le_bytes([data[24], data[25], data[26], data[27]]);
        const DUMP_MAGIC: [u32; 4] = [60011, 60012, 60013, 60014];
        if DUMP_MAGIC.contains(&magic) {
            signatures.push(
                RecordSignature::new("unix-dump", "Unix dump/restore tape format")
                    .with_format("Unix dump archive")
                    .with_platform("Unix/BSD")
                    .with_confidence("high")
                    .with_details(format!("Dump magic number: {magic}")),
            );
        }
    }

    fn detect_afio(&self, data: &[u8], signatures: &mut Vec<RecordSignature>) {
        if data.len() >= 5 && &data[0..5] == b"\x71\xc7\x00\x00\x00" {
            signatures.push(
                RecordSignature::new("afio", "AFIO archive format")
                    .with_format("AFIO archive")
                    .with_platform("Unix/Linux")
                    .with_confidence("high")
                    .with_details("Tape-optimized CPIO alternative"),
            );
        }
    }

    fn detect_qic(&self, data: &[u8], signatures: &mut Vec<RecordSignature>) {
        if data.len() >= 4 {
            if &data[0..4] == b"QIC\x00" || &data[0..4] == b"\x00QIC" {
                signatures.push(
                    RecordSignature::new("qic", "QIC tape format header")
                        .with_format("QIC tape format")
                        .with_platform("Quarter-Inch Cartridge")
                        .with_confidence("high"),
                );
            }
        }

        if data.len() >= 516 && &data[512..516] == b"QF\x00\x00" {
            signatures.push(
                RecordSignature::new("qic-113", "QIC-113 format tape")
                    .with_format("QIC-113")
                    .with_confidence("medium")
                    .with_details("Extended QIC format with file marks"),
            );
        }
    }

    fn detect_windows_backup(&self, data: &[u8], signatures: &mut Vec<RecordSignature>) {
        if data.len() >= 4 {
            if &data[0..4] == b"TAPE" {
                signatures.push(
                    RecordSignature::new("mtf", "Microsoft Tape Format (MTF)")
                        .with_format("Windows NT Backup")
                        .with_platform("Windows NT/2000/XP")
                        .with_confidence("high"),
                );
            } else if &data[0..4] == b"\x42\x54\x46\x00" {
                signatures.push(
                    RecordSignature::new("btf", "Backup Tape Format")
                        .with_format("Backup Tape Format")
                        .with_confidence("medium"),
                );
            }
        }
    }

    fn detect_novell_sms(&self, data: &[u8], signatures: &mut Vec<RecordSignature>) {
        if data.len() >= 4 && &data[0..4] == b"NWSM" {
            signatures.push(
                RecordSignature::new("novell-sms", "Novell SMS tape backup")
                    .with_format("Novell SMS backup")
                    .with_platform("NetWare")
                    .with_confidence("high"),
            );
        }
    }

    fn detect_ibm_standard_labels(&self, data: &[u8], signatures: &mut Vec<RecordSignature>) {
        if data.len() < 80 {
            return;
        }
        let prefix = &data[0..3];
        let digit = data[3];
        if matches!(prefix, b"VOL" | b"HDR" | b"EOF" | b"EOV") && digit.is_ascii_digit() {
            if let Ok(label) = std::str::from_utf8(&data[0..4]) {
                signatures.push(
                    RecordSignature::new("ibm-sl", format!("IBM Standard Label format ({label})"))
                        .with_format("IBM Standard Label")
                        .with_platform("IBM Mainframe")
                        .with_confidence("medium")
                        .with_details("IBM tape label structure detected"),
                );
            }
        }
    }

    fn detect_ltfs(&self, data: &[u8], signatures: &mut Vec<RecordSignature>) {
        if data.len() >= 5 && &data[0..5] == b"<?xml" {
            let window = min(512, data.len());
            if data[..window]
                .windows(9)
                .any(|w| w.eq_ignore_ascii_case(b"ltfsindex"))
            {
                signatures.push(
                    RecordSignature::new("ltfs", "LTFS (Linear Tape File System) index")
                        .with_format("LTFS")
                        .with_platform("LTO Tape")
                        .with_confidence("high")
                        .with_details("ltfsindex XML present"),
                );
                return;
            }
        }

        if data.len() >= 4 && &data[0..4] == b"LTFS" {
            signatures.push(
                RecordSignature::new("ltfs-label", "LTFS partition label")
                    .with_format("LTFS")
                    .with_platform("LTO Tape")
                    .with_confidence("high")
                    .with_details("Linear Tape File System metadata"),
            );
        }
    }

    fn detect_mxf(&self, data: &[u8], signatures: &mut Vec<RecordSignature>) {
        if data.len() < 16 {
            return;
        }
        if &data[0..4] == b"\x06\x0e\x2b\x34" {
            if &data[4..8] == b"\x02\x05\x01\x01" {
                signatures.push(
                    RecordSignature::new("mxf", "MXF (Material eXchange Format) file")
                        .with_format("MXF")
                        .with_platform("Professional Video/Broadcast")
                        .with_confidence("high")
                        .with_details("SMPTE partition pack"),
                );
            } else {
                signatures.push(
                    RecordSignature::new("mxf-klv", "MXF/KLV formatted data")
                        .with_format("MXF/KLV")
                        .with_platform("Professional Video/Broadcast")
                        .with_confidence("medium")
                        .with_details("SMPTE KLV key"),
                );
            }
        }
    }

    fn detect_block_size(&self, declared_length: u32, signatures: &mut Vec<RecordSignature>) {
        if signatures.is_empty() && matches!(declared_length, 2048 | 4096) {
            signatures.push(
                RecordSignature::new("rsx-block", "Block size matches common DEC utilities")
                    .with_platform("DEC operating systems")
                    .with_confidence("low"),
            );
        }
    }

    fn detect_text(&self, data: &[u8], signatures: &mut Vec<RecordSignature>) {
        if data.len() < 32 {
            return;
        }
        let printable = data
            .iter()
            .filter(|&&b| b == b'\n' || b == b'\r' || b == b'\t' || (32..=126).contains(&b))
            .count();
        if (printable as f32 / data.len() as f32) > 0.9 {
            let detail = if data.windows(2).any(|w| w == b"\r\n") {
                "DOS/Windows line endings"
            } else if data.contains(&b'\n') {
                "Unix line endings"
            } else if data.contains(&b'\r') {
                "classic Mac line endings"
            } else {
                "mixed line endings"
            };
            signatures.push(
                RecordSignature::new("ascii-text", format!("Plain text content ({detail})"))
                    .with_format("Text content")
                    .with_confidence("low"),
            );
        }
    }
}

fn push_signature(
    signatures: &mut Vec<RecordSignature>,
    mut sig: RecordSignature,
    fmt: Option<&'static str>,
    platform: Option<&'static str>,
) {
    if let Some(fmt) = fmt {
        if !fmt.is_empty() {
            sig = sig.with_format(fmt);
        }
    }
    if let Some(platform) = platform {
        if !platform.is_empty() {
            sig = sig.with_platform(platform);
        }
    }
    signatures.push(sig);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_gzip_magic() {
        let detector = SignatureDetector::default();
        let data = b"\x1f\x8brest";
        let signatures = detector.detect(data, 10);
        assert!(signatures.iter().any(|sig| sig.tag == "gzip"));
    }

    #[test]
    fn detects_tar_header() {
        let detector = SignatureDetector::default();
        let mut block = vec![0u8; 512];
        block[257..263].copy_from_slice(b"ustar\0");
        let signatures = detector.detect(&block, 512);
        assert!(signatures.iter().any(|sig| sig.tag == "tar-ustar"));
    }

    #[test]
    fn detects_plain_text_with_line_endings() {
        let detector = SignatureDetector::default();
        let data = b"First line of text\nSecond line of text\nThird line of text\n";
        let signatures = detector.detect(data, data.len() as u32);
        assert!(
            signatures
                .iter()
                .any(|sig| sig.tag == "ascii-text" && sig.description.contains("Unix"))
        );
    }

    #[test]
    fn block_size_hint_only_when_no_magic() {
        let detector = SignatureDetector::default();
        let data = vec![0u8; 100];
        let signatures = detector.detect(&data, 2048);
        assert!(signatures.iter().any(|sig| sig.tag == "rsx-block"));
    }

    #[test]
    fn detects_unix_compress_format() {
        let detector = SignatureDetector::default();
        let mut data = vec![0u8; 64];
        data[0] = 0x1f;
        data[1] = 0x9d;
        let signatures = detector.detect(&data, data.len() as u32);
        assert!(signatures.iter().any(|sig| sig.tag == "compress-z"));
    }

    #[test]
    fn detects_lha_archive_header() {
        let detector = SignatureDetector::default();
        let mut data = vec![0u8; 16];
        data[2..7].copy_from_slice(b"-lh5-");
        let signatures = detector.detect(&data, data.len() as u32);
        assert!(signatures.iter().any(|sig| sig.tag == "lha"));
    }

    #[test]
    fn detects_arc_header() {
        let detector = SignatureDetector::default();
        let mut data = vec![0u8; 32];
        data[0] = 0x1a;
        data[1] = 0x05;
        let signatures = detector.detect(&data, data.len() as u32);
        assert!(signatures.iter().any(|sig| sig.tag == "arc"));
    }

    #[test]
    fn detects_zoo_header() {
        let detector = SignatureDetector::default();
        let mut data = vec![0u8; 32];
        data[0..4].copy_from_slice(b"ZOO ");
        let signatures = detector.detect(&data, data.len() as u32);
        assert!(signatures.iter().any(|sig| sig.tag == "zoo"));
    }

    #[test]
    fn detects_arj_header() {
        let detector = SignatureDetector::default();
        let mut data = vec![0u8; 16];
        data[0] = 0x60;
        data[1] = 0xea;
        let signatures = detector.detect(&data, data.len() as u32);
        assert!(signatures.iter().any(|sig| sig.tag == "arj"));
    }

    #[test]
    fn detects_cabinet_file() {
        let detector = SignatureDetector::default();
        let mut data = vec![0u8; 32];
        data[0..4].copy_from_slice(b"MSCF");
        let signatures = detector.detect(&data, data.len() as u32);
        assert!(signatures.iter().any(|sig| sig.tag == "cab"));
    }

    #[test]
    fn detects_szdd_file() {
        let detector = SignatureDetector::default();
        let mut data = vec![0u8; 32];
        data[0..4].copy_from_slice(b"SZDD");
        let signatures = detector.detect(&data, data.len() as u32);
        assert!(signatures.iter().any(|sig| sig.tag == "szdd"));
    }
}
