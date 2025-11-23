use std::io::{self, Read, Seek, SeekFrom, Write};

pub const VERSION: &str = "1.0.0";
pub const AUTHOR: &str = "ACMS (Australia Computer Museum Society)";

pub const MAX_RECORD_LENGTH: u32 = 0x0120_0000; // ≈ 18 MiB safety ceiling

const CLASS_SHIFT: u32 = 28;
const CLASS_MASK: u32 = 0xF000_0000;
const VALUE_MASK: u32 = 0x0FFF_FFFF;

const TAPE_MARK_WORD: u32 = 0x0000_0000;
const ERASE_GAP_WORD: u32 = 0xFFFF_FFFE;
const END_OF_MEDIUM_WORD: u32 = 0xFFFF_FFFF;
const FORWARD_HALF_GAP_WORD: u32 = 0xFFFE_FFFF;
const FORWARD_HALF_GAP_ILLEGAL_START: u32 = 0xFFFE_0000;
const FORWARD_HALF_GAP_ILLEGAL_END: u32 = 0xFFFE_FFFE;
const REVERSE_HALF_GAP_START: u32 = 0xFFFF_0000;
const REVERSE_HALF_GAP_END: u32 = 0xFFFF_FFFD;

const PRIVATE_MARKER_CLASS: u8 = 0x7;
const RESERVED_MARKER_CLASS: u8 = 0xF;

fn encode_word(class: u8, value: u32) -> io::Result<u32> {
    if class > 0xF {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("SIMH class 0x{class:X} exceeds 4-bit limit"),
        ));
    }

    if value > VALUE_MASK {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("record length 0x{value:08X} exceeds SIMH extended limit 0x{VALUE_MASK:08X}"),
        ));
    }

    Ok(((class as u32) << CLASS_SHIFT) | (value & VALUE_MASK))
}

fn decode_word(word: u32) -> (u8, u32) {
    (
        ((word & CLASS_MASK) >> CLASS_SHIFT) as u8,
        word & VALUE_MASK,
    )
}

pub struct SimhTapeWriter<W: Write> {
    writer: W,
}

impl<W: Write> SimhTapeWriter<W> {
    pub fn new(writer: W) -> Self {
        Self { writer }
    }

    fn normalize_length(&self, data_len: usize) -> io::Result<u32> {
        let len = u32::try_from(data_len).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("record length {} does not fit in 32 bits", data_len),
            )
        })?;

        if len > MAX_RECORD_LENGTH {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "record length 0x{len:08X} exceeds safety ceiling 0x{MAX_RECORD_LENGTH:08X}"
                ),
            ));
        }

        Ok(len)
    }

    pub fn write_record(&mut self, data: &[u8]) -> io::Result<()> {
        self.write_record_with_class(0, data)
    }

    pub fn write_bad_record(&mut self, data: &[u8]) -> io::Result<()> {
        self.write_record_with_class(0x8, data)
    }

    pub fn write_record_with_class(&mut self, class: u8, data: &[u8]) -> io::Result<()> {
        let len = self.normalize_length(data.len())?;
        let word = encode_word(class, len)?;

        self.writer.write_all(&word.to_le_bytes())?;
        self.writer.write_all(data)?;

        if len % 2 != 0 {
            self.writer.write_all(&[0])?;
        }

        self.writer.write_all(&word.to_le_bytes())?;
        Ok(())
    }

    pub fn write_tape_mark(&mut self) -> io::Result<()> {
        self.writer.write_all(&TAPE_MARK_WORD.to_le_bytes())
    }

    pub fn write_end_of_medium(&mut self) -> io::Result<()> {
        self.writer.write_all(&END_OF_MEDIUM_WORD.to_le_bytes())
    }

    pub fn write_erase_gap_markers(&mut self, count: usize) -> io::Result<()> {
        for _ in 0..count {
            self.writer.write_all(&ERASE_GAP_WORD.to_le_bytes())?;
        }
        Ok(())
    }

    pub fn write_private_marker(&mut self, value: u32) -> io::Result<()> {
        let word = encode_word(PRIVATE_MARKER_CLASS, value)?;
        self.writer.write_all(&word.to_le_bytes())
    }

    pub fn into_inner(self) -> W {
        self.writer
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimhTapeMark {
    Single,
    Double,
    EndOfTape,
    EraseGap,
    HalfGapForward,
    HalfGapReverse,
    Private { class: u8, value: u32 },
    Reserved { class: u8, value: u32 },
}

#[derive(Debug, Clone, Copy)]
pub struct SimhTapeRecordHeader {
    pub offset: u64,
    pub class: u8,
    pub length: u32,
    pub trailing_length: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct SimhTapeRecord {
    pub header: SimhTapeRecordHeader,
    pub data: Vec<u8>,
}

#[derive(Debug)]
pub enum SimhTapeBlock {
    Record(SimhTapeRecord),
    TapeMark { offset: u64, kind: SimhTapeMark },
    EndOfStream,
}

pub struct SimhTapeReader<R> {
    reader: R,
    safety_limit: u32,
    pending_double: bool,
}

impl<R: Read + Seek> SimhTapeReader<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            safety_limit: MAX_RECORD_LENGTH,
            pending_double: false,
        }
    }

    pub fn with_limit(mut self, limit: u32) -> Self {
        self.safety_limit = limit.min(VALUE_MASK);
        self
    }

    fn read_word(&mut self) -> io::Result<Option<u32>> {
        let mut buf = [0u8; 4];
        let mut read = 0;
        while read < buf.len() {
            let n = self.reader.read(&mut buf[read..])?;
            if n == 0 {
                return if read == 0 {
                    Ok(None)
                } else {
                    Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "truncated SIMH word",
                    ))
                };
            }
            read += n;
        }
        Ok(Some(u32::from_le_bytes(buf)))
    }

    fn ensure_length_within_bounds(&self, length: u32) -> io::Result<()> {
        if length > self.safety_limit {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "record length 0x{length:08X} exceeds SIMH safety ceiling (0x{:08X})",
                    self.safety_limit
                ),
            ));
        }
        Ok(())
    }

    fn consume_tape_mark_kind(&mut self) -> io::Result<SimhTapeMark> {
        if self.pending_double {
            self.pending_double = false;
            return Ok(SimhTapeMark::Double);
        }

        let peek_pos = self.reader.stream_position()?;
        match self.read_word()? {
            Some(word) if word == TAPE_MARK_WORD => {
                self.pending_double = true;
                self.reader.seek(SeekFrom::Start(peek_pos))?;
                Ok(SimhTapeMark::Single)
            }
            Some(_) => {
                self.reader.seek(SeekFrom::Start(peek_pos))?;
                Ok(SimhTapeMark::Single)
            }
            None => Ok(SimhTapeMark::Single),
        }
    }

    fn try_parse_marker(&mut self, raw: u32) -> io::Result<Option<SimhTapeMark>> {
        match raw {
            END_OF_MEDIUM_WORD => return Ok(Some(SimhTapeMark::EndOfTape)),
            ERASE_GAP_WORD => return Ok(Some(SimhTapeMark::EraseGap)),
            FORWARD_HALF_GAP_WORD => {
                return Ok(Some(SimhTapeMark::HalfGapForward));
            }
            word if (FORWARD_HALF_GAP_ILLEGAL_START..=FORWARD_HALF_GAP_ILLEGAL_END)
                .contains(&word) =>
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("illegal forward half-gap marker 0x{word:08X}"),
                ));
            }
            word if (REVERSE_HALF_GAP_START..=REVERSE_HALF_GAP_END).contains(&word) => {
                return Ok(Some(SimhTapeMark::HalfGapReverse));
            }
            _ => {}
        }

        let (class, value) = decode_word(raw);

        if class == PRIVATE_MARKER_CLASS {
            return Ok(Some(SimhTapeMark::Private { class, value }));
        }

        if class == RESERVED_MARKER_CLASS {
            return Ok(Some(SimhTapeMark::Reserved { class, value }));
        }

        Ok(None)
    }

    pub fn next_block(&mut self) -> io::Result<SimhTapeBlock> {
        loop {
            let offset = self.reader.stream_position()?;
            let Some(word) = self.read_word()? else {
                return Ok(SimhTapeBlock::EndOfStream);
            };

            if word == TAPE_MARK_WORD {
                let kind = self.consume_tape_mark_kind()?;
                return Ok(SimhTapeBlock::TapeMark { offset, kind });
            }

            if let Some(kind) = self.try_parse_marker(word)? {
                return Ok(SimhTapeBlock::TapeMark { offset, kind });
            }

            let (class, length) = decode_word(word);
            self.ensure_length_within_bounds(length)?;

            let mut data = vec![0u8; length as usize];
            self.reader.read_exact(&mut data)?;

            if length % 2 != 0 {
                let mut pad = [0u8; 1];
                self.reader.read_exact(&mut pad)?;
            }

            let trailing = self.read_word()?.ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "missing trailing record length",
                )
            })?;

            if trailing != word {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "trailing length 0x{trailing:08X} does not match leading length 0x{word:08X}"
                    ),
                ));
            }

            return Ok(SimhTapeBlock::Record(SimhTapeRecord {
                header: SimhTapeRecordHeader {
                    offset,
                    class,
                    length,
                    trailing_length: Some(length),
                },
                data,
            }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_write_record_even() {
        let cursor = Cursor::new(Vec::new());
        let mut writer = SimhTapeWriter::new(cursor);

        let data = vec![0x01, 0x02];
        writer.write_record(&data).unwrap();

        let expected = vec![0x02, 0x00, 0x00, 0x00, 0x01, 0x02, 0x02, 0x00, 0x00, 0x00];
        let buf = writer.into_inner().into_inner();
        assert_eq!(buf, expected);
    }

    #[test]
    fn test_write_record_odd() {
        let cursor = Cursor::new(Vec::new());
        let mut writer = SimhTapeWriter::new(cursor);

        let data = vec![0x01, 0x02, 0x03];
        writer.write_record(&data).unwrap();

        let expected = vec![
            0x03, 0x00, 0x00, 0x00, 0x01, 0x02, 0x03, 0x00, 0x03, 0x00, 0x00, 0x00,
        ];
        let buf = writer.into_inner().into_inner();
        assert_eq!(buf, expected);
    }

    #[test]
    fn test_write_tape_mark() {
        let cursor = Cursor::new(Vec::new());
        let mut writer = SimhTapeWriter::new(cursor);

        writer.write_tape_mark().unwrap();

        let expected = vec![0x00, 0x00, 0x00, 0x00];
        let buf = writer.into_inner().into_inner();
        assert_eq!(buf, expected);
    }

    fn emit_record(buf: &mut Vec<u8>, class: u8, payload: &[u8]) {
        let len = payload.len() as u32;
        let word = encode_word(class, len).unwrap();
        buf.extend_from_slice(&word.to_le_bytes());
        buf.extend_from_slice(payload);
        if len % 2 != 0 {
            buf.push(0);
        }
        buf.extend_from_slice(&word.to_le_bytes());
    }

    #[test]
    fn reads_record_and_marks() {
        let mut tape = Vec::new();
        emit_record(&mut tape, 0, &[0x01, 0x02, 0x03]);
        tape.extend_from_slice(&TAPE_MARK_WORD.to_le_bytes());
        tape.extend_from_slice(&TAPE_MARK_WORD.to_le_bytes());
        tape.extend_from_slice(&END_OF_MEDIUM_WORD.to_le_bytes());

        let mut reader = SimhTapeReader::new(Cursor::new(tape));

        match reader.next_block().unwrap() {
            SimhTapeBlock::Record(record) => {
                assert_eq!(record.header.length, 3);
                assert_eq!(record.header.class, 0);
                assert_eq!(record.data, vec![1, 2, 3]);
                assert_eq!(record.header.trailing_length, Some(3));
            }
            other => panic!("expected record, got {:?}", other),
        }

        match reader.next_block().unwrap() {
            SimhTapeBlock::TapeMark { kind, .. } => assert_eq!(kind, SimhTapeMark::Single),
            other => panic!("expected tape mark, got {:?}", other),
        }

        match reader.next_block().unwrap() {
            SimhTapeBlock::TapeMark { kind, .. } => assert_eq!(kind, SimhTapeMark::Double),
            other => panic!("expected double tape mark, got {:?}", other),
        }

        match reader.next_block().unwrap() {
            SimhTapeBlock::TapeMark { kind, .. } => assert_eq!(kind, SimhTapeMark::EndOfTape),
            other => panic!("expected end-of-tape mark, got {:?}", other),
        }

        match reader.next_block().unwrap() {
            SimhTapeBlock::EndOfStream => {}
            other => panic!("expected end of stream, got {:?}", other),
        }
    }

    #[test]
    fn reads_bad_record_class() {
        let mut tape = Vec::new();
        emit_record(&mut tape, 0x8, &[0xAA, 0xBB]);
        tape.extend_from_slice(&TAPE_MARK_WORD.to_le_bytes());

        let mut reader = SimhTapeReader::new(Cursor::new(tape));
        match reader.next_block().unwrap() {
            SimhTapeBlock::Record(record) => {
                assert_eq!(record.header.class, 0x8);
                assert_eq!(record.data, vec![0xAA, 0xBB]);
            }
            other => panic!("expected record, got {:?}", other),
        }
    }

    #[test]
    fn handles_half_gap_and_erase_markers() {
        let mut tape = Vec::new();
        tape.extend_from_slice(&FORWARD_HALF_GAP_WORD.to_le_bytes());
        tape.extend_from_slice(&ERASE_GAP_WORD.to_le_bytes());

        let mut reader = SimhTapeReader::new(Cursor::new(tape));

        match reader.next_block().unwrap() {
            SimhTapeBlock::TapeMark { kind, .. } => assert_eq!(kind, SimhTapeMark::HalfGapForward),
            other => panic!("expected half-gap marker, got {:?}", other),
        }

        match reader.next_block().unwrap() {
            SimhTapeBlock::TapeMark { kind, .. } => assert_eq!(kind, SimhTapeMark::EraseGap),
            other => panic!("expected erase-gap marker, got {:?}", other),
        }
    }

    #[test]
    fn rejects_giant_records() {
        let mut tape = Vec::new();
        let len = MAX_RECORD_LENGTH + 1;
        tape.extend_from_slice(&len.to_le_bytes());
        tape.resize(tape.len() + len as usize, 0);

        let mut reader = SimhTapeReader::new(Cursor::new(tape));
        let err = reader.next_block().unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }
}
