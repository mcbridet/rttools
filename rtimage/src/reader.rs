use crossbeam_channel::Sender;
use std::io::Read;
use std::thread;

// Default buffer size from timage.c (120KB)
const MAXSIZE: usize = 120 * 1024;

pub enum TapeEvent {
    Data(Vec<u8>),
    TapeMark, // 0-byte read
    Error(String),
}

pub fn start_reader_thread(
    mut reader: Box<dyn Read + Send>,
    sender: Sender<TapeEvent>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut buffer = vec![0u8; MAXSIZE];

        loop {
            match reader.read(&mut buffer) {
                Ok(0) => {
                    // Tape Mark or EOF
                    // In timage.c, if count == 0 (no data read at all), it retries.
                    // "Drive reported 0 bytes on last read, we will try again."
                    // This usually happens if the drive is not ready or empty initially.

                    // However, if we are in the middle of reading, 0 means Tape Mark.
                    // timage.c logic:
                    // if (nc == 0) break; -> child terminates.
                    // Parent sees EOF.
                    // If count == 0L (parent read 0 blocks), it calls wait_for_more_data().

                    // We need to distinguish "Start of Stream" 0-read vs "Middle of Stream" 0-read.
                    // But the reader thread doesn't know "count".

                    // Let's send the TapeMark event. The consumer (main thread) tracks the count.
                    // If the consumer sees a TapeMark and count is 0, it can decide to retry.
                    // But the reader thread has already sent the event and is waiting for next instruction?
                    // No, the reader loop continues?

                    // If we send TapeMark, we should probably pause or stop?
                    // If it's a real tape, a 0-read is a delimiter. The next read might be data (next file).
                    // If it's EOF (EOT), we get two 0-reads.

                    // Let's just send the event.
                    if sender.send(TapeEvent::TapeMark).is_err() {
                        break;
                    }

                    // If we just loop immediately, we might flood with TapeMarks if it's a real EOF.
                    // timage.c child terminates on 0 read.
                    // Parent re-spawns child for next file?
                    // "for ( ; ; ) { /* for each input tape file: */ ... fork() ... }"
                    // Yes! timage.c forks a NEW child for each file on the tape.
                    // So when a child hits 0 bytes (Tape Mark), it exits.
                    // The parent then handles the Tape Mark, and loops back to fork a NEW child.

                    // So our Reader Thread should EXIT on 0 bytes.
                    // The Main Thread will then decide whether to spawn a NEW Reader Thread.
                    break;
                }
                Ok(n) => {
                    // Send data
                    let data = buffer[0..n].to_vec();
                    if sender.send(TapeEvent::Data(data)).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    // Check for retryable errors?
                    // timage.c checks ENOENT, ENXIO, ENODEV, EIO and exits with error.
                    let _ = sender.send(TapeEvent::Error(e.to_string()));
                    break;
                }
            }
        }
    })
}
