//! Shared application layer types
//!
//! This module contains types that are shared across multiple application services
//! to avoid code duplication.

/// Urgency level for priority adjustments across all application services
#[derive(Debug, Clone, PartialEq)]
pub enum AdjustmentUrgency {
    Low,
    Medium,
    High,
    Critical,
}

impl AdjustmentUrgency {
    /// Convert to numeric urgency level for comparison
    pub fn as_level(&self) -> u8 {
        match self {
            Self::Low => 1,
            Self::Medium => 2,
            Self::High => 3,
            Self::Critical => 4,
        }
    }

    /// Check if this urgency requires immediate action
    pub fn is_immediate(&self) -> bool {
        matches!(self, Self::High | Self::Critical)
    }
}

impl PartialOrd for AdjustmentUrgency {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for AdjustmentUrgency {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.as_level().cmp(&other.as_level())
    }
}

impl Eq for AdjustmentUrgency {}