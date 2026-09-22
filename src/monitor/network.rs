use std::time::Instant;
use sysinfo::Networks;

#[derive(Debug, Clone, Default)]
pub struct NetworkData {
    pub download_speed: u64, // bytes per second
    pub upload_speed: u64,   // bytes per second
}

pub struct NetworkMonitor {
    networks: Networks,
    prev_rx: u64,
    prev_tx: u64,
    last_sample: Option<Instant>,
}

impl NetworkMonitor {
    pub fn new() -> Self {
        let mut networks = Networks::new_with_refreshed_list();
        networks.refresh(true);

        let mut total_rx = 0u64;
        let mut total_tx = 0u64;
        for (_name, data) in &networks {
            total_rx += data.received();
            total_tx += data.transmitted();
        }

        Self {
            networks,
            prev_rx: total_rx,
            prev_tx: total_tx,
            last_sample: Some(Instant::now()),
        }
    }

    pub fn sample(&mut self) -> NetworkData {
        self.networks.refresh(true);

        let mut current_rx = 0u64;
        let mut current_tx = 0u64;
        for (_name, data) in &self.networks {
            current_rx += data.received();
            current_tx += data.transmitted();
        }

        let now = Instant::now();
        let mut download_speed = 0u64;
        let mut upload_speed = 0u64;

        if let Some(last_time) = self.last_sample {
            let elapsed = now.duration_since(last_time).as_secs_f64();
            if elapsed > 0.05 {
                let rx_diff = current_rx.saturating_sub(self.prev_rx);
                let tx_diff = current_tx.saturating_sub(self.prev_tx);

                download_speed = (rx_diff as f64 / elapsed) as u64;
                upload_speed = (tx_diff as f64 / elapsed) as u64;
            }
        }

        self.prev_rx = current_rx;
        self.prev_tx = current_tx;
        self.last_sample = Some(now);

        NetworkData {
            download_speed,
            upload_speed,
        }
    }
}

/// Helper function to format byte speed into human readable string (e.g. "2.4 MB/s")
pub fn format_speed(bytes_per_sec: u64) -> String {
    if bytes_per_sec >= 1024 * 1024 * 1024 {
        format!("{:.1} GB/s", bytes_per_sec as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes_per_sec >= 1024 * 1024 {
        format!("{:.1} MB/s", bytes_per_sec as f64 / (1024.0 * 1024.0))
    } else if bytes_per_sec >= 1024 {
        format!("{:.0} KB/s", bytes_per_sec as f64 / 1024.0)
    } else {
        format!("{} B/s", bytes_per_sec)
    }
}
