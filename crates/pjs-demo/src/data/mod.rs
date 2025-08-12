//! Data generation utilities for PJS demonstrations

pub mod ecommerce;
pub mod social;
pub mod analytics;
pub mod realtime;

pub use ecommerce::*;
pub use social::*;
pub use analytics::*;
// pub use realtime::*; // TODO: Enable when realtime module is used

use serde_json::Value;
use std::collections::HashMap;

/// Dataset types for demonstrations
#[derive(Debug, Clone, Copy)]
pub enum DatasetType {
    /// E-commerce store with products, reviews, analytics
    ECommerce,
    /// Social media feed with posts, users, interactions
    SocialMedia,
    /// Analytics dashboard with metrics, charts, reports
    Analytics,
    /// Real-time monitoring with live metrics, alerts
    RealTime,
}

/// Dataset size variants
#[derive(Debug, Clone, Copy)]
pub enum DatasetSize {
    /// Small dataset (~1-10KB) - fast networks, simple demos
    Small,
    /// Medium dataset (~10-100KB) - typical web API responses
    Medium,
    /// Large dataset (~100KB-1MB) - complex dashboards
    Large,
    /// Huge dataset (~1-10MB) - data analysis, reporting
    Huge,
}

impl DatasetSize {
    /// Get typical size range for this dataset size
    pub fn size_range_kb(&self) -> (f64, f64) {
        match self {
            DatasetSize::Small => (1.0, 10.0),
            DatasetSize::Medium => (10.0, 100.0),
            DatasetSize::Large => (100.0, 1000.0),
            DatasetSize::Huge => (1000.0, 10000.0),
        }
    }

    /// Get item count for given dataset type
    pub fn item_count(&self, dataset_type: DatasetType) -> usize {
        match (self, dataset_type) {
            (DatasetSize::Small, _) => 10,
            (DatasetSize::Medium, DatasetType::ECommerce) => 50,
            (DatasetSize::Medium, DatasetType::SocialMedia) => 25,
            (DatasetSize::Medium, _) => 30,
            (DatasetSize::Large, DatasetType::ECommerce) => 200,
            (DatasetSize::Large, DatasetType::SocialMedia) => 100,
            (DatasetSize::Large, _) => 150,
            (DatasetSize::Huge, DatasetType::ECommerce) => 1000,
            (DatasetSize::Huge, DatasetType::SocialMedia) => 500,
            (DatasetSize::Huge, _) => 750,
        }
    }
}

impl std::str::FromStr for DatasetSize {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "small" | "s" => Ok(DatasetSize::Small),
            "medium" | "m" => Ok(DatasetSize::Medium),
            "large" | "l" => Ok(DatasetSize::Large),
            "huge" | "h" => Ok(DatasetSize::Huge),
            _ => Err(format!("Unknown dataset size: {s}")),
        }
    }
}

impl std::str::FromStr for DatasetType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "ecommerce" | "e" | "store" => Ok(DatasetType::ECommerce),
            "social" | "s" | "feed" => Ok(DatasetType::SocialMedia),
            "analytics" | "a" | "dashboard" => Ok(DatasetType::Analytics),
            "realtime" | "r" | "live" => Ok(DatasetType::RealTime),
            _ => Err(format!("Unknown dataset type: {s}")),
        }
    }
}

/// Generate dataset based on type and size
pub fn generate_dataset(dataset_type: DatasetType, size: DatasetSize) -> Value {
    match dataset_type {
        DatasetType::ECommerce => generate_ecommerce_data(size),
        DatasetType::SocialMedia => generate_social_data(size),
        DatasetType::Analytics => generate_analytics_data(size),
        DatasetType::RealTime => generate_realtime_data(size),
    }
}

/// Generate metadata for dataset
pub fn generate_metadata(dataset_type: DatasetType, size: DatasetSize, data: &Value) -> HashMap<String, Value> {
    let mut metadata = HashMap::new();
    
    metadata.insert("type".to_string(), serde_json::json!(format!("{dataset_type:?}").to_lowercase()));
    metadata.insert("size".to_string(), serde_json::json!(format!("{size:?}").to_lowercase()));
    
    let json_str = serde_json::to_string(data).unwrap_or_default();
    metadata.insert("bytes".to_string(), serde_json::json!(json_str.len()));
    metadata.insert("size_kb".to_string(), serde_json::json!((json_str.len() as f64 / 1024.0).round() / 10.0 * 10.0));
    
    metadata.insert("generated_at".to_string(), 
                   serde_json::json!(chrono::Utc::now().to_rfc3339()));
    
    let item_count = size.item_count(dataset_type);
    metadata.insert("items".to_string(), serde_json::json!(item_count));
    
    metadata
}