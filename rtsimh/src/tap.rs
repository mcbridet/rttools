use std::io::{self, Read, Seek, SeekFrom};

pub const MAX_RECORD_LENGTH: u32 = 0x0120_0000; // ≈ 18 MiB safety ceiling

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimhTapeMark {
    Single,
    Double,
    EndOfTape,
}

#[derive(Debug, Clone, Copy)]
pub struct SimhTapeRecordHeader {
    pub offset: u64,
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
}

impl<R: Read + Seek> SimhTapeReader<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            safety_limit: MAX_RECORD_LENGTH,
        }
    }

    pub fn with_limit(mut self, limit: u32) -> Self {
        self.safety_limit = limit;
        self
    }

    fn read_length(&mut self) -> io::Result<Option<u32>> {
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
                        "truncated record length",
                    ))
                };
            }
            read += n;
        }
        Ok(Some(u32::from_le_bytes(buf)))
    }

    pub fn next_block(&mut self) -> io::Result<SimhTapeBlock> {
        let offset = self.reader.stream_position()?;
        let Some(length) = self.read_length()? else {
            return Ok(SimhTapeBlock::EndOfStream);
        };

        match length {
            0 => {
                let peek_pos = self.reader.stream_position()?;
                let next = self.read_length()?;
                let kind = match next {
                    Some(0) => SimhTapeMark::Double,
                    Some(_) => {
                        self.reader.seek(SeekFrom::Start(peek_pos))?;
                        SimhTapeMark::Single
                    }
                    None => SimhTapeMark::Single,
                };
                Ok(SimhTapeBlock::TapeMark { offset, kind })
            }
            0xFFFF_FFFF => Ok(SimhTapeBlock::TapeMark {
                offset,
                kind: SimhTapeMark::EndOfTape,
            }),
            len if len > self.safety_limit => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "record length 0x{len:08X} exceeds SIMH safety ceiling (0x{:08X})",
                    self.safety_limit
                ),
            )),
            len => {
                let mut data = vec![0u8; len as usize];
                self.reader.read_exact(&mut data)?;

                if len % 2 != 0 {
                    let mut pad = [0u8; 1];
                    self.reader.read_exact(&mut pad)?;
                }

                let trailing = self.read_length()?;
                Ok(SimhTapeBlock::Record(SimhTapeRecord {
                    header: SimhTapeRecordHeader {
                        offset,
                        length: len,
                        trailing_length: trailing,
                    },
                    data,
                }))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn emit_record(buf: &mut Vec<u8>, payload: &[u8]) {
        let len = payload.len() as u32;
        buf.extend_from_slice(&len.to_le_bytes());
        buf.extend_from_slice(payload);
        if len % 2 != 0 {
            buf.push(0);
        }
        buf.extend_from_slice(&len.to_le_bytes());
    }

    #[test]
    fn reads_record_and_marks() {
        let mut tape = Vec::new();
        emit_record(&mut tape, &[0x01, 0x02, 0x03]);
        tape.extend_from_slice(&0u32.to_le_bytes()); // single tape mark
        tape.extend_from_slice(&0u32.to_le_bytes()); // second zero -> double
        tape.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes()); // end of tape

        let mut reader = SimhTapeReader::new(Cursor::new(tape));

        match reader.next_block().unwrap() {
            SimhTapeBlock::Record(record) => {
                assert_eq!(record.header.length, 3);
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
