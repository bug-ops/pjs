use serde::{Deserialize, Serialize};

/// Signal indicating client's receive buffer state for backpressure control
///
/// Clients send backpressure signals to inform the server about their processing
/// capacity. The server uses these signals to throttle or pause frame transmission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[non_exhaustive]
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
}
