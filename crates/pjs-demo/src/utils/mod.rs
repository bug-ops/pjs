//! Utility functions for PJS demonstrations

use std::time::{Duration, Instant};
use tokio::time::sleep;

/// Simulate network latency for different connection types
pub async fn simulate_network_latency(connection_type: NetworkType) {
    let delay = match connection_type {
        NetworkType::Fiber => Duration::from_millis(5),
        NetworkType::Cable => Duration::from_millis(20),
        NetworkType::Wifi => Duration::from_millis(50),
        NetworkType::Mobile4G => Duration::from_millis(100),
        NetworkType::Mobile3G => Duration::from_millis(300),
        NetworkType::Satellite => Duration::from_millis(600),
    };
    
    sleep(delay).await;
}

/// Network connection types for simulation
#[derive(Debug, Clone, Copy)]
pub enum NetworkType {
    Fiber,
    Cable, 
    Wifi,
    Mobile4G,
    Mobile3G,
    Satellite,
}

impl std::str::FromStr for NetworkType {
    type Err = String;
    
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "fiber" | "f" => Ok(NetworkType::Fiber),
            "cable" | "c" => Ok(NetworkType::Cable),
            "wifi" | "w" => Ok(NetworkType::Wifi),
            "4g" | "mobile4g" => Ok(NetworkType::Mobile4G),
            "3g" | "mobile3g" => Ok(NetworkType::Mobile3G),
            "satellite" | "s" => Ok(NetworkType::Satellite),
            _ => Err(format!("Unknown network type: {}", s)),
        }
    }
}

/// Performance timer for measuring operation duration
pub struct PerfTimer {
    start: Instant,
    name: String,
}

impl PerfTimer {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            start: Instant::now(),
            name: name.into(),
        }
    }
    
    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }
    
    pub fn lap(&self, label: &str) {
        println!("  ⏱️  {}: {} - {:?}", self.name, label, self.elapsed());
    }
}

impl Drop for PerfTimer {
    fn drop(&mut self) {
        println!("  ✅ {} completed in {:?}", self.name, self.elapsed());
    }
}

/// Format bytes to human readable format
pub fn format_bytes(bytes: usize) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB"];
    let mut size = bytes as f64;
    let mut unit_index = 0;
    
    while size >= 1024.0 && unit_index < UNITS.len() - 1 {
        size /= 1024.0;
        unit_index += 1;
    }
    
    if unit_index == 0 {
        format!("{} {}", bytes, UNITS[unit_index])
    } else {
        format!("{:.1} {}", size, UNITS[unit_index])
    }
}

/// Format duration to human readable format
pub fn format_duration(duration: Duration) -> String {
    let millis = duration.as_millis();
    if millis < 1000 {
        format!("{}ms", millis)
    } else {
        format!("{:.1}s", duration.as_secs_f64())
    }
}