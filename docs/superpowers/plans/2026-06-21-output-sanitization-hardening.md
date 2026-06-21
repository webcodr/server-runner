# Output Sanitization & Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the three security-audit findings in subprocess output handling — unbounded per-line memory (LOW-1), terminal-injection via teed escape sequences (LOW-2), and UTF-8 chunk-boundary corruption (INFO-1) — by extracting the duplicated output-capture code in `core/server.rs` and `core/command.rs` into one hardened, tested module.

**Architecture:** Today `core/server.rs` and `core/command.rs` each carry a near-identical copy of `spawn_reader` / `capture_lines` / `write_output`. We replace both copies with a single `core/output.rs` that fixes all three findings in one place:
- `LineAccumulator` buffers **raw bytes** and only decodes at `\n` boundaries (fixes INFO-1), force-flushes a line at a byte cap (fixes LOW-1), and strips control characters except tab from each captured line.
- `AnsiTeeFilter` is a streaming state machine that, for bytes teed to the real terminal, **keeps SGR colour/style sequences** (`ESC[…m`) but strips OSC (title/clipboard), cursor-movement, screen/line-clear, and stray C0 control bytes (fixes LOW-2, per the chosen "strip only dangerous sequences" option). The TUI render path is already safe because ratatui renders into cells.
- `spawn_reader` ties them together; `server.rs` and `command.rs` call it and delete their local copies.

**Tech Stack:** Rust 2024, tokio (`process`, `io-util`), existing `RingBuffer` in `core/state.rs`. No new dependencies.

---

## File Structure

- **Create:** `src/core/output.rs` — all subprocess-output handling: `LineAccumulator`, `AnsiTeeFilter`, `sanitize_line`, `write_output`, `spawn_reader`, plus unit tests. Single responsibility: turning raw child-process byte streams into safe captured lines and safe teed terminal output.
- **Modify:** `src/core/mod.rs` — register `pub mod output;`.
- **Modify:** `src/core/server.rs` — delete local `spawn_reader` / `capture_lines` / `write_output`; call `crate::core::output::spawn_reader`; clean up now-unused imports.
- **Modify:** `src/core/command.rs` — same refactor as `server.rs`.
- **Modify:** `README.md` — extend the Security Model section to document output handling.

**Design notes / decisions locked in here:**
- `MAX_LINE_BYTES = 64 * 1024`: a single captured line is force-flushed once it reaches 64 KiB without a newline.
- `MAX_CSI_LEN = 64`: a CSI escape sequence longer than 64 bytes is treated as malformed and dropped (bounds the filter's own buffer).
- C1 control bytes (0x80–0x9F) are **not** stripped at the byte level because they are indistinguishable from UTF-8 continuation/lead bytes; the injection vector we close is the ESC-introduced (0x1B) sequence, and ESC is always consumed by the state machine.
- `LOG_CAPACITY` and `build_command` stay in `core/server.rs` (other modules import them); only the output helpers move.

---

### Task 1: `LineAccumulator` — byte-correct, bounded, sanitized line capture (LOW-1 + INFO-1)

**Files:**
- Create: `src/core/output.rs`
- Modify: `src/core/mod.rs` (add module declaration)
- Test: inline `#[cfg(test)] mod tests` in `src/core/output.rs`

- [ ] **Step 1: Register the module and create the file with failing tests**

Add the module declaration in `src/core/mod.rs` (insert alphabetically among the existing `pub mod` lines, after `pub mod health;`):

```rust
pub mod health;
pub mod output;
pub mod server;
```

Create `src/core/output.rs` with the consts, a stub, and the tests (the stub makes the file compile-fail against the tests, which is the intended failing state):

```rust
#![allow(dead_code)] // removed in Task 6 once server.rs and command.rs use these

use crate::core::state::RingBuffer;

use std::sync::{Arc, Mutex};

/// Max bytes buffered for a single in-progress line before it is force-flushed.
/// Bounds memory when a child emits a very long run of bytes with no newline.
const MAX_LINE_BYTES: usize = 64 * 1024;

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
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib core::output 2>&1 | tail -20`
Expected: FAIL — compile error, `cannot find type LineAccumulator in this scope`.

- [ ] **Step 3: Implement `LineAccumulator` and `sanitize_line`**

Insert this above the `#[cfg(test)]` block in `src/core/output.rs`:

```rust
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
```

Note: `RingBuffer`, `Arc`, `Mutex` are imported now but only used from Task 3 onward; the file-level `#![allow(dead_code)]` keeps clippy quiet until Task 6.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib core::output 2>&1 | tail -20`
Expected: PASS — 4 passed (`splits_lines_and_trims_carriage_return`, `decodes_multibyte_char_split_across_chunks`, `caps_oversized_line_without_newline`, `strips_control_chars_but_keeps_tab`).

- [ ] **Step 5: Commit**

```bash
git add src/core/output.rs src/core/mod.rs
git commit -m "feat(output): add bounded UTF-8-correct LineAccumulator"
```

---

### Task 2: `AnsiTeeFilter` — keep colour, strip dangerous sequences (LOW-2)

**Files:**
- Modify: `src/core/output.rs`
- Test: inline `#[cfg(test)] mod tests` in `src/core/output.rs`

- [ ] **Step 1: Add the failing filter tests**

Append these tests inside the existing `mod tests` block in `src/core/output.rs`:

```rust
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
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib core::output 2>&1 | tail -20`
Expected: FAIL — compile error, `cannot find type AnsiTeeFilter in this scope`.

- [ ] **Step 3: Implement `AnsiTeeFilter`**

Add the `MAX_CSI_LEN` const directly under the existing `MAX_LINE_BYTES` const near the top of `src/core/output.rs`:

```rust
/// Max bytes accumulated for a single CSI escape sequence before it is treated
/// as malformed and dropped. Bounds memory in the terminal filter.
const MAX_CSI_LEN: usize = 64;
```

Then add the filter above the `#[cfg(test)]` block:

```rust
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
/// sequences (`ESC[ … m`). Strips every other escape sequence — OSC (window
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
                    b']' => self.state = TeeState::Osc,
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
    matches!(b, b'\n' | b'\r' | b'\t') || (0x20..=0x7e).contains(&b) || b >= 0x80
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib core::output 2>&1 | tail -20`
Expected: PASS — 10 passed (4 from Task 1 + 6 new filter tests).

- [ ] **Step 5: Commit**

```bash
git add src/core/output.rs
git commit -m "feat(output): add AnsiTeeFilter that keeps colour, strips control sequences"
```

---

### Task 3: Shared `write_output` and `spawn_reader`

**Files:**
- Modify: `src/core/output.rs`

- [ ] **Step 1: Add the imports needed by the reader**

At the top of `src/core/output.rs`, replace the existing import block:

```rust
use crate::core::state::RingBuffer;

use std::sync::{Arc, Mutex};
```

with:

```rust
use tokio::io::AsyncReadExt;
use tokio::task::JoinHandle;

use std::io::{self, Write};
use std::sync::{Arc, Mutex};

use crate::core::state::RingBuffer;
```

- [ ] **Step 2: Add `write_output` and `spawn_reader`**

Insert above the `#[cfg(test)]` block in `src/core/output.rs`:

```rust
fn write_output(bytes: &[u8], stderr: bool) {
    if stderr {
        let mut stream = io::stderr().lock();
        let _ = stream.write_all(bytes);
        let _ = stream.flush();
    } else {
        let mut stream = io::stdout().lock();
        let _ = stream.write_all(bytes);
        let _ = stream.flush();
    }
}

/// Spawn a task that drains `stream`, capturing sanitized lines into `log` and,
/// when `tee_output` is set, writing colour-preserving, escape-filtered bytes to
/// the real stdout/stderr.
pub fn spawn_reader<R>(
    mut stream: R,
    log: Arc<Mutex<RingBuffer>>,
    stderr: bool,
    tee_output: bool,
) -> JoinHandle<()>
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut buf = [0u8; 8192];
        let mut acc = LineAccumulator::new(MAX_LINE_BYTES);
        let mut tee = AnsiTeeFilter::new();

        while let Ok(n) = stream.read(&mut buf).await {
            if n == 0 {
                break;
            }
            if tee_output {
                write_output(&tee.filter(&buf[..n]), stderr);
            }
            acc.push(&buf[..n], |line| {
                if let Ok(mut buf) = log.lock() {
                    buf.push(line);
                }
            });
        }

        acc.finish(|line| {
            if let Ok(mut buf) = log.lock() {
                buf.push(line);
            }
        });
    })
}
```

- [ ] **Step 3: Verify the crate still builds and all output tests pass**

Run: `cargo test --lib core::output 2>&1 | tail -20`
Expected: PASS — 10 passed; no compile errors. (`write_output` / `spawn_reader` are still unused outside the module but covered by `#![allow(dead_code)]` until Task 6.)

- [ ] **Step 4: Commit**

```bash
git add src/core/output.rs
git commit -m "feat(output): add shared spawn_reader and sanitized tee writer"
```

---

### Task 4: Refactor `core/server.rs` onto the shared reader

**Files:**
- Modify: `src/core/server.rs`

- [ ] **Step 1: Replace the imports**

In `src/core/server.rs`, replace the top import block:

```rust
use anyhow::bail;
use command_group::{AsyncCommandGroup, AsyncGroupChild};
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use std::io::{self, Write};
use std::sync::{Arc, Mutex};

use crate::core::state::RingBuffer;
```

with (drops `AsyncReadExt` and `std::io`, adds the shared reader):

```rust
use anyhow::bail;
use command_group::{AsyncCommandGroup, AsyncGroupChild};
use tokio::process::Command;

use std::sync::{Arc, Mutex};

use crate::core::output::spawn_reader;
use crate::core::state::RingBuffer;
```

- [ ] **Step 2: Delete the local helpers**

In `src/core/server.rs`, delete the entire local `spawn_reader` function (the `fn spawn_reader<R>(...) { ... }` block), the `fn write_output(...)` block, and the `fn capture_lines(...)` block. These now live in `core/output.rs`.

The two call sites inside `spawn_inner` stay exactly as written — they already read `spawn_reader(stdout, Arc::clone(&log), false, tee_output);` and resolve to the imported function. The returned `JoinHandle` is intentionally dropped here (fire-and-forget), matching the previous behaviour.

- [ ] **Step 3: Run the server tests to verify behaviour is unchanged**

Run: `cargo test --lib core::server 2>&1 | tail -20`
Expected: PASS — `spawn_captured_server_records_output_lines` still passes; no unused-import or dead-code warnings from `server.rs`.

- [ ] **Step 4: Commit**

```bash
git add src/core/server.rs
git commit -m "refactor(server): use shared core::output reader"
```

---

### Task 5: Refactor `core/command.rs` onto the shared reader

**Files:**
- Modify: `src/core/command.rs`

- [ ] **Step 1: Replace the imports**

In `src/core/command.rs`, replace the top import block:

```rust
use command_group::{AsyncCommandGroup, AsyncGroupChild};
use tokio::io::AsyncReadExt;
use tokio::process::Child;
use tokio::task::JoinHandle;

use std::io::{self, Write};
use std::sync::{Arc, Mutex};

use crate::core::server::{LOG_CAPACITY, build_command};
use crate::core::state::RingBuffer;
```

with (drops `AsyncReadExt` and `std::io`, adds the shared reader; keeps `JoinHandle`, which the structs still use):

```rust
use command_group::{AsyncCommandGroup, AsyncGroupChild};
use tokio::process::Child;
use tokio::task::JoinHandle;

use std::sync::{Arc, Mutex};

use crate::core::output::spawn_reader;
use crate::core::server::{LOG_CAPACITY, build_command};
use crate::core::state::RingBuffer;
```

- [ ] **Step 2: Delete the local helpers**

In `src/core/command.rs`, delete the entire local `fn spawn_reader<R>(...)`, `fn write_output(...)`, and `fn capture_lines(...)` blocks. The call sites in `spawn_captured_group` and `spawn_inner` already read `readers.push(spawn_reader(...))` and resolve to the imported function, which returns the same `JoinHandle<()>`.

- [ ] **Step 3: Run the command tests to verify behaviour is unchanged**

Run: `cargo test --lib core::command 2>&1 | tail -20`
Expected: PASS — `spawn_captured_records_output_lines` still passes; no unused-import or dead-code warnings from `command.rs`.

- [ ] **Step 4: Commit**

```bash
git add src/core/command.rs
git commit -m "refactor(command): use shared core::output reader"
```

---

### Task 6: Remove the dead-code allow, document, and run the full gate

**Files:**
- Modify: `src/core/output.rs`
- Modify: `README.md`

- [ ] **Step 1: Remove the temporary allow**

In `src/core/output.rs`, delete the first line:

```rust
#![allow(dead_code) // removed in Task 6 once server.rs and command.rs use these
```

(Delete the whole `#![allow(dead_code)] // …` line.) Every item is now reachable: `spawn_reader` is called from `server.rs` and `command.rs`; `write_output`, `AnsiTeeFilter`, `LineAccumulator`, and `sanitize_line` are reachable from `spawn_reader`.

- [ ] **Step 2: Document the output security model**

In `README.md`, inside the `### Security Model` section, add this paragraph immediately after the "Readiness URLs are intended for local test services…" paragraph:

```markdown
Server Runner captures and forwards child-process output defensively. Captured
log lines are bounded in length and have control characters stripped. When
output is echoed to your terminal, ANSI colour and style codes are preserved
while window-title, clipboard, cursor-movement, and screen-clearing escape
sequences are removed, so untrusted data flowing through a server's logs cannot
manipulate your terminal.
```

- [ ] **Step 3: Run the full verification gate**

Run: `cargo fmt -- --check && cargo clippy --all-targets -- -D warnings && cargo test`
Expected: PASS — formatting clean, no clippy warnings, all tests green (the new `core::output` tests plus the existing suite).

- [ ] **Step 4: Commit**

```bash
git add src/core/output.rs README.md
git commit -m "docs: document output sanitization; drop dead-code allow"
```

---

## Self-Review

**Spec coverage (audit findings → tasks):**
- LOW-1 (unbounded line buffer) → Task 1 (`MAX_LINE_BYTES` force-flush in `LineAccumulator`, test `caps_oversized_line_without_newline`).
- LOW-2 (terminal-injection in tee, "strip only dangerous sequences") → Task 2 (`AnsiTeeFilter`, tests for SGR-kept / OSC-stripped / cursor-clear-stripped / split-across-chunks) wired into the tee path in Task 3.
- INFO-1 (UTF-8 boundary corruption) → Task 1 (raw-byte buffering, decode at line boundary, test `decodes_multibyte_char_split_across_chunks`).
- DRY cleanup of duplicated helpers → Tasks 4–5.
- User-facing documentation → Task 6.

**Placeholder scan:** No TBD/TODO/"add error handling"/"similar to Task N" — every code step shows full code.

**Type consistency:** `LineAccumulator::new(max_line_bytes)`, `.push(&[u8], impl FnMut(String))`, `.finish(impl FnMut(String))`; `AnsiTeeFilter::new()`, `.filter(&[u8]) -> Vec<u8>`; `spawn_reader<R>(R, Arc<Mutex<RingBuffer>>, bool, bool) -> JoinHandle<()>` — names and signatures match across the module definition (Tasks 1–3) and both call sites (Tasks 4–5). `MAX_LINE_BYTES` / `MAX_CSI_LEN` defined in Tasks 1/2 and consumed in Tasks 2/3.
