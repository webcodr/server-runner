#![allow(dead_code)] // removed in Task 6 once server.rs and command.rs use these

use crate::core::state::RingBuffer;

use std::sync::{Arc, Mutex};

/// Max bytes buffered for a single in-progress line before it is force-flushed.
/// Bounds memory when a child emits a very long run of bytes with no newline.
const MAX_LINE_BYTES: usize = 64 * 1024;

/// Accumulates raw bytes and emits sanitized, UTF-8-correct lines.
///
/// - Splits on `\n`, trims a trailing `\r` (CRLF).
/// - Decodes only at line boundaries, so multibyte UTF-8 characters split
///   across read() chunks are never corrupted (INFO-1).
/// - Force-flushes a line once it reaches `max_line_bytes`, bounding memory
///   when a child never emits a newline (LOW-1).
/// - Strips control characters (except tab) from each emitted line.
pub struct LineAccumulator {
    buf: Vec<u8>,
    max_line_bytes: usize,
}

impl LineAccumulator {
    pub fn new(max_line_bytes: usize) -> Self {
        Self {
            buf: Vec::new(),
            max_line_bytes,
        }
    }

    pub fn push(&mut self, chunk: &[u8], mut emit: impl FnMut(String)) {
        for &byte in chunk {
            if byte == b'\n' {
                emit(self.take_line());
            } else {
                self.buf.push(byte);
                if self.buf.len() >= self.max_line_bytes {
                    emit(self.take_line());
                }
            }
        }
    }

    pub fn finish(&mut self, mut emit: impl FnMut(String)) {
        if !self.buf.is_empty() {
            emit(self.take_line());
        }
    }

    fn take_line(&mut self) -> String {
        let raw = std::mem::take(&mut self.buf);
        let decoded = String::from_utf8_lossy(&raw);
        sanitize_line(decoded.trim_end_matches('\r'))
    }
}

/// Drop control characters (except tab) from a captured log line.
fn sanitize_line(line: &str) -> String {
    line.chars()
        .filter(|&c| c == '\t' || !c.is_control())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect(acc: &mut LineAccumulator, chunks: &[&[u8]]) -> Vec<String> {
        let mut lines = Vec::new();
        for chunk in chunks {
            acc.push(chunk, |l| lines.push(l));
        }
        acc.finish(|l| lines.push(l));
        lines
    }

    #[test]
    fn splits_lines_and_trims_carriage_return() {
        let mut acc = LineAccumulator::new(1024);
        let lines = collect(&mut acc, &[b"one\r\ntwo\n", b"three"]);
        assert_eq!(
            lines,
            vec!["one".to_string(), "two".to_string(), "three".to_string()]
        );
    }

    #[test]
    fn decodes_multibyte_char_split_across_chunks() {
        let mut acc = LineAccumulator::new(1024);
        let euro = "€".as_bytes(); // [0xE2, 0x82, 0xAC]
        let lines = collect(&mut acc, &[&euro[..1], &euro[1..], b"\n"]);
        assert_eq!(lines, vec!["€".to_string()]);
    }

    #[test]
    fn caps_oversized_line_without_newline() {
        let mut acc = LineAccumulator::new(8);
        let mut lines = Vec::new();
        acc.push(&[b'a'; 20], |l| lines.push(l));
        assert!(lines.len() >= 2);
        assert!(lines.iter().all(|l| l.len() <= 8));
    }

    #[test]
    fn strips_control_chars_but_keeps_tab() {
        let mut acc = LineAccumulator::new(1024);
        let lines = collect(&mut acc, &[b"a\x1b[31mb\tc\n"]);
        assert_eq!(lines, vec!["a[31mb\tc".to_string()]);
    }
}
