use sysinfo::System;

#[derive(Debug, Clone)]
pub struct CpuData {
    pub usage: f32,
    pub model: String,
    pub frequency_ghz: Option<f32>,
}

pub struct CpuMonitor {
    model_cached: Option<String>,
}

impl CpuMonitor {
    pub fn new() -> Self {
        Self { model_cached: None }
    }

    pub fn sample(&mut self, sys: &System) -> CpuData {
        let usage = sys.global_cpu_usage();

        // Cache the CPU brand since it doesn't change
        let model = if let Some(m) = &self.model_cached {
            m.clone()
        } else {
            let brand = sys
                .cpus()
                .first()
                .map(|c| c.brand().trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "Unknown CPU".to_string());
            self.model_cached = Some(brand.clone());
            brand
        };

        // Average frequency across cores (MHz -> GHz)
        let cpus = sys.cpus();
        let frequency_ghz = if !cpus.is_empty() {
            let sum_freq: u64 = cpus.iter().map(|c| c.frequency()).sum();
            let avg_freq = sum_freq as f32 / cpus.len() as f32;
            if avg_freq > 0.0 {
                Some(avg_freq / 1000.0)
            } else {
                None
            }
        } else {
            None
        };

        CpuData {
            usage,
            model,
            frequency_ghz,
        }
    }
}
