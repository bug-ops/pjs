use serde::{Deserialize, Serialize};

/// Signal indicating client's receive buffer state for backpressure control
///
/// Clients send backpressure signals to inform the server about their processing
/// capacity. The server uses these signals to throttle or pause frame transmission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum BackpressureSignal {
    /// Client is ready for more data, no throttling needed
    #[default]
    Ok,

    /// Client's buffer is filling up, server should slow down transmission
    SlowDown,

    /// Client's buffer is full, server must pause transmission
    Pause,
}

impl BackpressureSignal {
    /// Returns true if this signal indicates the server should pause
    pub fn should_pause(&self) -> bool {
        matches!(self, BackpressureSignal::Pause)
    }

    /// Returns true if this signal indicates the server should slow down
    pub fn should_throttle(&self) -> bool {
        matches!(
            self,
            BackpressureSignal::SlowDown | BackpressureSignal::Pause
        )
    }

    /// Get suggested delay in milliseconds based on backpressure signal
    pub fn suggested_delay_ms(&self) -> u64 {
        match self {
            BackpressureSignal::Ok => 0,
            BackpressureSignal::SlowDown => 100,
            BackpressureSignal::Pause => u64::MAX, // Indefinite pause until resumed
        }
    }
}

impl std::fmt::Display for BackpressureSignal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BackpressureSignal::Ok => write!(f, "OK"),
            BackpressureSignal::SlowDown => write!(f, "SLOW_DOWN"),
            BackpressureSignal::Pause => write!(f, "PAUSE"),
        }
    }
}

/// Credit-based flow control state
///
/// Tracks available credits for frame transmission. Each frame consumes credits,
/// and the client replenishes credits by sending backpressure signals.
#[derive(Debug, Clone)]
pub struct FlowControlCredits {
    available: usize,
    max_credits: usize,
}

impl FlowControlCredits {
    /// Create new credit tracker with maximum credits
    pub fn new(max_credits: usize) -> Self {
        Self {
            available: max_credits,
            max_credits,
        }
    }

    /// Check if enough credits are available for a frame
    pub fn has_credits(&self, required: usize) -> bool {
        self.available >= required
    }

    /// Consume credits for frame transmission
    ///
    /// Returns an error if not enough credits are available
    pub fn consume(&mut self, amount: usize) -> Result<(), String> {
        if self.available < amount {
            return Err(format!(
                "Insufficient credits: needed {}, available {}",
                amount, self.available
            ));
        }
        self.available = self.available.saturating_sub(amount);
        Ok(())
    }

    /// Add credits back to the pool
    ///
    /// Credits are capped at max_credits
    pub fn add(&mut self, amount: usize) {
        self.available = (self.available.saturating_add(amount)).min(self.max_credits);
    }

    /// Get current available credits
    pub fn available(&self) -> usize {
        self.available
    }

    /// Get maximum allowed credits
    pub fn max_credits(&self) -> usize {
        self.max_credits
    }

    /// Check if credits are exhausted
    pub fn is_exhausted(&self) -> bool {
        self.available == 0
    }

    /// Reset credits to maximum
    pub fn reset(&mut self) {
        self.available = self.max_credits;
    }
}

impl Default for FlowControlCredits {
    fn default() -> Self {
        Self::new(1000) // Default 1000 credits
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backpressure_signal_default() {
        let signal = BackpressureSignal::default();
        assert_eq!(signal, BackpressureSignal::Ok);
        assert!(!signal.should_pause());
        assert!(!signal.should_throttle());
    }

    #[test]
    fn test_backpressure_signal_should_pause() {
        assert!(BackpressureSignal::Pause.should_pause());
        assert!(!BackpressureSignal::SlowDown.should_pause());
        assert!(!BackpressureSignal::Ok.should_pause());
    }

    #[test]
    fn test_backpressure_signal_should_throttle() {
        assert!(BackpressureSignal::Pause.should_throttle());
        assert!(BackpressureSignal::SlowDown.should_throttle());
        assert!(!BackpressureSignal::Ok.should_throttle());
    }

    #[test]
    fn test_backpressure_signal_suggested_delay() {
        assert_eq!(BackpressureSignal::Ok.suggested_delay_ms(), 0);
        assert_eq!(BackpressureSignal::SlowDown.suggested_delay_ms(), 100);
        assert_eq!(BackpressureSignal::Pause.suggested_delay_ms(), u64::MAX);
    }

    #[test]
    fn test_backpressure_signal_display() {
        assert_eq!(BackpressureSignal::Ok.to_string(), "OK");
        assert_eq!(BackpressureSignal::SlowDown.to_string(), "SLOW_DOWN");
        assert_eq!(BackpressureSignal::Pause.to_string(), "PAUSE");
    }

    #[test]
    fn test_backpressure_signal_serialization() {
        let signal = BackpressureSignal::SlowDown;
        let json = serde_json::to_string(&signal).unwrap();
        let deserialized: BackpressureSignal = serde_json::from_str(&json).unwrap();
        assert_eq!(signal, deserialized);
    }

    #[test]
    fn test_flow_control_credits_new() {
        let credits = FlowControlCredits::new(100);
        assert_eq!(credits.available(), 100);
        assert_eq!(credits.max_credits(), 100);
        assert!(!credits.is_exhausted());
    }

    #[test]
    fn test_flow_control_credits_default() {
        let credits = FlowControlCredits::default();
        assert_eq!(credits.available(), 1000);
        assert_eq!(credits.max_credits(), 1000);
    }

    #[test]
    fn test_flow_control_credits_has_credits() {
        let credits = FlowControlCredits::new(100);
        assert!(credits.has_credits(50));
        assert!(credits.has_credits(100));
        assert!(!credits.has_credits(101));
    }

    #[test]
    fn test_flow_control_credits_consume() {
        let mut credits = FlowControlCredits::new(100);

        assert!(credits.consume(30).is_ok());
        assert_eq!(credits.available(), 70);

        assert!(credits.consume(70).is_ok());
        assert_eq!(credits.available(), 0);
        assert!(credits.is_exhausted());

        assert!(credits.consume(1).is_err());
    }

    #[test]
    fn test_flow_control_credits_add() {
        let mut credits = FlowControlCredits::new(100);

        credits.consume(50).unwrap();
        assert_eq!(credits.available(), 50);

        credits.add(30);
        assert_eq!(credits.available(), 80);

        // Cannot exceed max
        credits.add(100);
        assert_eq!(credits.available(), 100);
    }

    #[test]
    fn test_flow_control_credits_reset() {
        let mut credits = FlowControlCredits::new(100);

        credits.consume(80).unwrap();
        assert_eq!(credits.available(), 20);

        credits.reset();
        assert_eq!(credits.available(), 100);
        assert!(!credits.is_exhausted());
    }

    #[test]
    fn test_flow_control_credits_saturating_operations() {
        let mut credits = FlowControlCredits::new(100);

        // Consume all credits
        credits.consume(100).unwrap();

        // Try to consume more (should fail gracefully)
        assert!(credits.consume(1).is_err());
        assert_eq!(credits.available(), 0);

        // Add back more than max
        credits.add(usize::MAX);
        assert_eq!(credits.available(), 100); // Should cap at max
    }

    #[test]
    fn test_flow_control_credits_is_exhausted() {
        let mut credits = FlowControlCredits::new(10);

        assert!(!credits.is_exhausted());

        credits.consume(10).unwrap();
        assert!(credits.is_exhausted());

        credits.add(1);
        assert!(!credits.is_exhausted());
    }

    #[test]
    fn test_flow_control_credits_error_message() {
        let mut credits = FlowControlCredits::new(50);

        let result = credits.consume(100);
        assert!(result.is_err());

        let error_msg = result.unwrap_err();
        assert!(error_msg.contains("Insufficient credits"));
        assert!(error_msg.contains("needed 100"));
        assert!(error_msg.contains("available 50"));
    }
}
