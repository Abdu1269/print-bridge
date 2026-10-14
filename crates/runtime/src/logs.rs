use std::collections::VecDeque;

pub use print_bridge_core::activity::TaskLogEntry;

/// Default number of task logs retained in memory.
pub const DEFAULT_LOG_CAPACITY: usize = 500;

/// Fixed-capacity in-memory log store retaining only recent task records.
#[derive(Debug, Clone)]
pub struct LogStore {
    capacity: usize,
    entries: VecDeque<TaskLogEntry>,
}

impl Default for LogStore {
    /// Create log store using default capacity.
    fn default() -> Self {
        Self::with_capacity(DEFAULT_LOG_CAPACITY)
    }
}

impl LogStore {
    /// Create log store with specified maximum entry count.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            capacity,
            entries: VecDeque::new(),
        }
    }

    /// Append log entry, dropping oldest entry when capacity is exceeded.
    pub fn push(&mut self, entry: TaskLogEntry) {
        self.entries.push_back(entry);

        while self.entries.len() > self.capacity {
            self.entries.pop_front();
        }
    }

    /// Return log entries in chronological order from oldest to newest.
    pub fn recent(&self) -> Vec<TaskLogEntry> {
        self.entries.iter().cloned().collect()
    }

    /// Clear currently retained in-memory logs.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}
