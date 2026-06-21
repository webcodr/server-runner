#![allow(dead_code, unused_imports)] // removed in Task 6 once server.rs and command.rs use these

use crate::core::state::RingBuffer;

use std::sync::{Arc, Mutex};

/// Max bytes buffered for a single in-progress line before it is force-flushed.
/// Bounds memory when a child emits a very long run of bytes with no newline.
const MAX_LINE_BYTES: usize = 64 * 1024;

/// Max bytes accumulated for a single CSI escape sequence before it is treated
/// as malformed and dropped. Bounds memory in the terminal filter.
const MAX_CSI_LEN: usize = 64;

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

#[derive(Clone, Copy, PartialEq, Eq)]
enum TeeState {
    Ground,
    Esc,
    Csi,
    Osc,
    OscEsc,
}

/// Streaming filter for bytes teed to the real terminal (LOW-2).
///
/// Keeps printable text, `\n`, `\r`, `\t`, UTF-8 bytes, and SGR colour/style
/// sequences (`ESC[ ... m`). Strips every other escape sequence — OSC (window
/// title, clipboard), cursor movement, screen/line clears — and stray C0
/// control bytes. State persists across chunks, so sequences split across
/// read() boundaries are still handled.
pub struct AnsiTeeFilter {
    state: TeeState,
    pending: Vec<u8>,
}

impl Default for AnsiTeeFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl AnsiTeeFilter {
    pub fn new() -> Self {
        Self {
            state: TeeState::Ground,
            pending: Vec::new(),
        }
    }

    pub fn filter(&mut self, input: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(input.len());
        for &b in input {
            match self.state {
                TeeState::Ground => {
                    if b == 0x1b {
                        self.state = TeeState::Esc;
                    } else if is_safe_ground_byte(b) {
                        out.push(b);
                    }
                }
                TeeState::Esc => match b {
                    b'[' => {
                        self.pending.clear();
                        self.pending.extend_from_slice(b"\x1b[");
                        self.state = TeeState::Csi;
                    }
                    // OSC (]), DCS (P), SOS (X), PM (^) and APC (_) all introduce
                    // a string payload terminated by ST (ESC \) or BEL. Route them
                    // all through the string-consuming Osc state so the payload
                    // text cannot leak to the terminal as visible characters.
                    b']' | b'P' | b'X' | b'^' | b'_' => self.state = TeeState::Osc,
                    _ => self.state = TeeState::Ground,
                },
                TeeState::Csi => {
                    if b == 0x1b {
                        // Resync on a fresh escape inside a malformed sequence.
                        self.pending.clear();
                        self.state = TeeState::Esc;
                        continue;
                    }
                    self.pending.push(b);
                    if (0x40..=0x7e).contains(&b) {
                        if b == b'm' {
                            out.extend_from_slice(&self.pending);
                        }
                        self.pending.clear();
                        self.state = TeeState::Ground;
                    } else if self.pending.len() > MAX_CSI_LEN {
                        self.pending.clear();
                        self.state = TeeState::Ground;
                    }
                }
                TeeState::Osc => {
                    if b == 0x07 {
                        self.state = TeeState::Ground;
                    } else if b == 0x1b {
                        self.state = TeeState::OscEsc;
                    }
                }
                TeeState::OscEsc => self.state = TeeState::Ground,
            }
        }
        out
    }
}

fn is_safe_ground_byte(b: u8) -> bool {
    // Bytes 0x80–0x9f are the 8-bit C1 control range (0x9b is CSI, 0x9d is OSC),
    // but they are ALSO valid UTF-8 continuation bytes — e.g. 0x82 in "€"
    // (E2 82 AC). This filter is byte-wise and not UTF-8-aware in Ground state,
    // so it cannot drop the C1 range without corrupting multibyte characters.
    // C1 introducers are therefore passed through; modern UTF-8 terminals treat
    // them as invalid sequence bytes rather than CSI/OSC, so the residual risk
    // is limited to legacy 8-bit / Latin-1 terminal modes.
    matches!(b, b'\n' | b'\r' | b'\t') || (0x20..=0x7e).contains(&b) || b >= 0x80
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
        let lines = collect(&mut acc, &[&[b'a'; 20]]);
        assert_eq!(lines.len(), 3);
        assert!(lines.iter().all(|l| l.len() <= 8));
        assert_eq!(lines[2].len(), 4);
    }

    #[test]
    fn strips_control_chars_but_keeps_tab() {
        let mut acc = LineAccumulator::new(1024);
        let lines = collect(&mut acc, &[b"a\x1b[31mb\tc\n"]);
        assert_eq!(lines, vec!["a[31mb\tc".to_string()]);
    }

    #[test]
    fn tee_keeps_sgr_colour() {
        let mut tee = AnsiTeeFilter::new();
        assert_eq!(tee.filter(b"\x1b[31mred\x1b[0m"), b"\x1b[31mred\x1b[0m");
    }

    #[test]
    fn tee_strips_osc_title_sequence() {
        let mut tee = AnsiTeeFilter::new();
        // OSC 0 ; pwned BEL  -> window-title / clipboard vector
        assert_eq!(tee.filter(b"a\x1b]0;pwned\x07b"), b"ab");
    }

    #[test]
    fn tee_strips_cursor_and_clear_sequences() {
        let mut tee = AnsiTeeFilter::new();
        assert_eq!(tee.filter(b"a\x1b[2J\x1b[Hb"), b"ab");
    }

    #[test]
    fn tee_strips_stray_control_keeps_newline_tab() {
        let mut tee = AnsiTeeFilter::new();
        assert_eq!(tee.filter(b"a\x07b\tc\n"), b"ab\tc\n");
    }

    #[test]
    fn tee_handles_sequence_split_across_chunks() {
        let mut tee = AnsiTeeFilter::new();
        let mut out = tee.filter(b"\x1b[3");
        out.extend(tee.filter(b"1mhi"));
        assert_eq!(out, b"\x1b[31mhi");
    }

    #[test]
    fn tee_drops_del_keeps_utf8() {
        let mut tee = AnsiTeeFilter::new();
        assert_eq!(tee.filter("a\x7f€".as_bytes()), "a€".as_bytes());
    }

    #[test]
    fn tee_strips_dcs_and_apc_payloads() {
        // DCS (ESC P) payload terminated by ST (ESC \)
        let mut tee = AnsiTeeFilter::new();
        assert_eq!(tee.filter(b"a\x1bPdata\x1b\\b"), b"ab");
        // APC (ESC _) payload terminated by BEL
        let mut tee = AnsiTeeFilter::new();
        assert_eq!(tee.filter(b"a\x1b_cmd\x07b"), b"ab");
    }

    #[test]
    fn tee_drops_overlong_csi_sequence() {
        let mut tee = AnsiTeeFilter::new();
        let mut input = vec![0x1b, b'['];
        input.extend(std::iter::repeat(b'9').take(MAX_CSI_LEN + 10));
        input.push(b'm');
        let out = tee.filter(&input);
        // An overlong CSI is treated as malformed and dropped: the ESC
        // introducer must never leak to the terminal.
        assert!(!out.contains(&0x1b));
    }
}
