//! Priority Value Object with compile-time safety
//!
//! Provides type-safe priority system with validation rules
//! and compile-time constants for common priority levels.

use crate::{DomainError, DomainResult};
use std::fmt;
use std::num::NonZeroU8;

/// Type-safe priority value (1-255 range)
///
/// This is a pure domain object with no serialization concerns.
/// Custom serialization helpers are provided in `pjson_rs_domain::events::serde_priority`
/// for domain events that need serialization.
///
/// # Example
/// ```
/// use pjson_rs_domain::value_objects::Priority;
///
/// let priority = Priority::new(100).unwrap();
/// assert_eq!(priority.value(), 100);
/// assert_eq!(priority, Priority::CRITICAL);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Priority(NonZeroU8);

impl Priority {
    /// Critical priority - for essential data (IDs, status, core metadata)
    pub const CRITICAL: Self = Self::new_unchecked(100);

    /// High priority - for important visible data (names, titles)
    pub const HIGH: Self = Self::new_unchecked(80);

    /// Medium priority - for regular content
    pub const MEDIUM: Self = Self::new_unchecked(50);

    /// Low priority - for supplementary data
    pub const LOW: Self = Self::new_unchecked(25);

    /// Background priority - for analytics, logs, etc.
    pub const BACKGROUND: Self = Self::new_unchecked(10);

    /// Create priority with validation
    pub fn new(value: u8) -> DomainResult<Self> {
        NonZeroU8::new(value)
            .map(Self)
            .ok_or_else(|| DomainError::InvalidPriority("Priority cannot be zero".to_string()))
    }

    /// Create priority without validation (for const contexts)
    const fn new_unchecked(value: u8) -> Self {
        // Safety: We control all usage sites to ensure value > 0
        unsafe { Self(NonZeroU8::new_unchecked(value)) }
    }

    /// Get raw priority value
    pub fn value(self) -> u8 {
        self.0.get()
    }

    /// Increase priority by delta (saturating at max)
    pub fn increase_by(self, delta: u8) -> Self {
        let new_value = self.0.get().saturating_add(delta);
        Self(NonZeroU8::new(new_value).unwrap_or(NonZeroU8::MAX))
    }

    /// Decrease priority by delta (saturating at min)
    pub fn decrease_by(self, delta: u8) -> Self {
        let new_value = self.0.get().saturating_sub(delta);
        Self(NonZeroU8::new(new_value).unwrap_or(NonZeroU8::MIN))
    }

    /// Check if this is a critical priority
    pub fn is_critical(self) -> bool {
        self.0.get() >= Self::CRITICAL.0.get()
    }

    /// Check if this is high priority or above
    pub fn is_high_or_above(self) -> bool {
        self.0.get() >= Self::HIGH.0.get()
    }

    /// Create priority from percentage (0-100)
    pub fn from_percentage(percent: f32) -> DomainResult<Self> {
        if !(0.0..=100.0).contains(&percent) {
            return Err(DomainError::InvalidPriority(format!(
                "Percentage must be 0-100, got {percent}"
            )));
        }

        let value = (percent * 2.55).round() as u8;
        Self::new(value.max(1)) // Ensure non-zero
    }

    /// Convert to percentage (0-100)
    pub fn to_percentage(self) -> f32 {
        (self.0.get() as f32 / 255.0) * 100.0
    }

    /// Get priority value with fallback for compatibility
    pub fn unwrap_or(self, _default: u8) -> u8 {
        self.0.get()
    }
}

impl fmt::Display for Priority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::CRITICAL => {
                let val = self.0.get();
                write!(f, "Critical({val})")
            }
            Self::HIGH => {
                let val = self.0.get();
                write!(f, "High({val})")
            }
            Self::MEDIUM => {
                let val = self.0.get();
                write!(f, "Medium({val})")
            }
            Self::LOW => {
                let val = self.0.get();
                write!(f, "Low({val})")
            }
            Self::BACKGROUND => {
                let val = self.0.get();
                write!(f, "Background({val})")
            }
            _ => {
                let val = self.0.get();
                write!(f, "Priority({val})")
            }
        }
    }
}

impl From<Priority> for u8 {
    fn from(priority: Priority) -> Self {
        priority.0.get()
    }
}

impl TryFrom<u8> for Priority {
    type Error = DomainError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

/// Priority validation rules (reserved for future use)
#[allow(dead_code)]
pub trait PriorityRule {
    fn validate(&self, priority: Priority) -> bool;
    fn name(&self) -> &'static str;
}

/// Rule: Priority must be at least minimum value (reserved for future use)
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct MinimumPriority(pub Priority);

#[allow(dead_code)]
impl PriorityRule for MinimumPriority {
    fn validate(&self, priority: Priority) -> bool {
        priority >= self.0
    }

    fn name(&self) -> &'static str {
        "minimum_priority"
    }
}

/// Rule: Priority must be within range (reserved for future use)
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct PriorityRange {
    pub min: Priority,
    pub max: Priority,
}

#[allow(dead_code)]
impl PriorityRule for PriorityRange {
    fn validate(&self, priority: Priority) -> bool {
        priority >= self.min && priority <= self.max
    }

    fn name(&self) -> &'static str {
        "priority_range"
    }
}

/// Collection of priority rules for validation (reserved for future use)
#[allow(dead_code)]
pub struct PriorityRules {
    rules: Vec<Box<dyn PriorityRule + Send + Sync>>,
}

#[allow(dead_code)]
impl PriorityRules {
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    pub fn add_rule(mut self, rule: impl PriorityRule + Send + Sync + 'static) -> Self {
        self.rules.push(Box::new(rule));
        self
    }

    pub fn validate(&self, priority: Priority) -> DomainResult<()> {
        for rule in &self.rules {
            if !rule.validate(priority) {
                return Err(DomainError::InvalidPriority(format!(
                    "Priority {priority} violates rule: {}",
                    rule.name()
                )));
            }
        }
        Ok(())
    }
}

impl Default for PriorityRules {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_priority_constants() {
        assert_eq!(Priority::CRITICAL.value(), 100);
        assert_eq!(Priority::HIGH.value(), 80);
        assert_eq!(Priority::MEDIUM.value(), 50);
        assert_eq!(Priority::LOW.value(), 25);
        assert_eq!(Priority::BACKGROUND.value(), 10);
    }

    #[test]
    fn test_priority_validation() {
        assert!(Priority::new(1).is_ok());
        assert!(Priority::new(255).is_ok());
        assert!(Priority::new(0).is_err());
    }

    #[test]
    fn test_priority_ordering() {
        assert!(Priority::CRITICAL > Priority::HIGH);
        assert!(Priority::HIGH > Priority::MEDIUM);
        assert!(Priority::MEDIUM > Priority::LOW);
        assert!(Priority::LOW > Priority::BACKGROUND);
    }

    #[test]
    fn test_priority_arithmetic() {
        let p = Priority::MEDIUM;
        assert_eq!(p.increase_by(10).value(), 60);
        assert_eq!(p.decrease_by(10).value(), 40);

        // Test saturation
        let max_p = Priority::new(255).unwrap();
        assert_eq!(max_p.increase_by(10).value(), 255);

        let min_p = Priority::new(1).unwrap();
        assert_eq!(min_p.decrease_by(10).value(), 1);
    }

    #[test]
    fn test_priority_percentage() {
        let p = Priority::from_percentage(50.0).unwrap();
        assert!(p.to_percentage() >= 49.0 && p.to_percentage() <= 51.0);

        assert!(Priority::from_percentage(101.0).is_err());
        assert!(Priority::from_percentage(-1.0).is_err());
    }

    #[test]
    fn test_priority_rules() {
        let rules = PriorityRules::new()
            .add_rule(MinimumPriority(Priority::LOW))
            .add_rule(PriorityRange {
                min: Priority::LOW,
                max: Priority::CRITICAL,
            });

        assert!(rules.validate(Priority::MEDIUM).is_ok());
        assert!(rules.validate(Priority::new(5).unwrap()).is_err());
        assert!(rules.validate(Priority::new(200).unwrap()).is_err());
    }

    #[test]
    fn test_priority_display() {
        assert_eq!(Priority::CRITICAL.to_string(), "Critical(100)");
        assert_eq!(Priority::HIGH.to_string(), "High(80)");
        assert_eq!(Priority::new(42).unwrap().to_string(), "Priority(42)");
    }
}
