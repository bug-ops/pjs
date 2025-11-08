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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adjustment_urgency_as_level() {
        assert_eq!(AdjustmentUrgency::Low.as_level(), 1);
        assert_eq!(AdjustmentUrgency::Medium.as_level(), 2);
        assert_eq!(AdjustmentUrgency::High.as_level(), 3);
        assert_eq!(AdjustmentUrgency::Critical.as_level(), 4);
    }

    #[test]
    fn test_adjustment_urgency_is_immediate() {
        assert!(!AdjustmentUrgency::Low.is_immediate());
        assert!(!AdjustmentUrgency::Medium.is_immediate());
        assert!(AdjustmentUrgency::High.is_immediate());
        assert!(AdjustmentUrgency::Critical.is_immediate());
    }

    #[test]
    fn test_adjustment_urgency_ordering() {
        assert!(AdjustmentUrgency::Low < AdjustmentUrgency::Medium);
        assert!(AdjustmentUrgency::Medium < AdjustmentUrgency::High);
        assert!(AdjustmentUrgency::High < AdjustmentUrgency::Critical);
    }

    #[test]
    fn test_adjustment_urgency_equality() {
        assert_eq!(AdjustmentUrgency::Low, AdjustmentUrgency::Low);
        assert_eq!(AdjustmentUrgency::Critical, AdjustmentUrgency::Critical);
        assert_ne!(AdjustmentUrgency::Low, AdjustmentUrgency::High);
    }

    #[test]
    fn test_adjustment_urgency_partial_ord() {
        assert_eq!(
            AdjustmentUrgency::Low.partial_cmp(&AdjustmentUrgency::Medium),
            Some(std::cmp::Ordering::Less)
        );
        assert_eq!(
            AdjustmentUrgency::High.partial_cmp(&AdjustmentUrgency::High),
            Some(std::cmp::Ordering::Equal)
        );
        assert_eq!(
            AdjustmentUrgency::Critical.partial_cmp(&AdjustmentUrgency::Low),
            Some(std::cmp::Ordering::Greater)
        );
    }

    #[test]
    fn test_adjustment_urgency_clone() {
        let urgency = AdjustmentUrgency::High;
        let cloned = urgency.clone();
        assert_eq!(urgency, cloned);
    }

    #[test]
    fn test_adjustment_urgency_debug() {
        let urgency = AdjustmentUrgency::Medium;
        let debug_str = format!("{:?}", urgency);
        assert!(debug_str.contains("Medium"));
    }
}
