use std::collections::VecDeque;
use std::fmt;
use std::ops::AddAssign;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServerStatus {
    Waiting,
    Running,
    #[allow(dead_code)] // used by TUI in Plan 2
    Failed,
    #[allow(dead_code)] // used by TUI in Plan 2
    Stopped,
}

#[derive(Copy, Clone, Debug)]
pub struct Attempts(pub u8);

impl AddAssign<u8> for Attempts {
    fn add_assign(&mut self, other: u8) {
        self.0 = self.0.saturating_add(other);
    }
}

impl fmt::Display for Attempts {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl PartialEq<u8> for Attempts {
    fn eq(&self, other: &u8) -> bool {
        self.0 == *other
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ServerName(pub String);

/// Bounded FIFO line buffer. Oldest lines drop once `capacity` is exceeded.
pub struct RingBuffer {
    lines: VecDeque<String>,
    capacity: usize,
}

impl RingBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            lines: VecDeque::with_capacity(capacity.min(1024)),
            capacity,
        }
    }

    pub fn push(&mut self, line: String) {
        if self.lines.len() == self.capacity {
            self.lines.pop_front();
        }
        self.lines.push_back(line);
    }

    #[allow(dead_code)] // used by AppState in Task 9
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    #[allow(dead_code)] // used by AppState in Task 9
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    #[allow(dead_code)] // used by AppState in Task 9
    pub fn iter(&self) -> impl Iterator<Item = &String> {
        self.lines.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attempts_saturate_at_u8_max() {
        let mut a = Attempts(254);
        a += 1;
        a += 1;
        a += 1;
        assert_eq!(a, 255u8);
    }

    #[test]
    fn ring_buffer_drops_oldest_when_full() {
        let mut rb = RingBuffer::new(2);
        rb.push("a".into());
        rb.push("b".into());
        rb.push("c".into());
        assert_eq!(rb.len(), 2);
        let got: Vec<_> = rb.iter().cloned().collect();
        assert_eq!(got, vec!["b".to_string(), "c".to_string()]);
    }
}
