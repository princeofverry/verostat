use sysinfo::System;

#[derive(Debug, Clone, Default)]
pub struct MemoryData {
    pub used: u64,
    pub total: u64,
    pub percentage: f32,
}

pub struct MemoryMonitor;

impl MemoryMonitor {
    pub fn new() -> Self {
        Self
    }

    pub fn sample(&self, sys: &System) -> MemoryData {
        let used = sys.used_memory();
        let total = sys.total_memory();
        let percentage = if total > 0 {
            (used as f32 / total as f32) * 100.0
        } else {
            0.0
        };

        MemoryData {
            used,
            total,
            percentage,
        }
    }
}
