use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;

use crate::domain::services::ConnectionManager;

/// Infrastructure service for monitoring connection timeouts
pub struct TimeoutMonitor {
    connection_manager: Arc<ConnectionManager>,
    check_interval: Duration,
}

impl TimeoutMonitor {
    pub fn new(connection_manager: Arc<ConnectionManager>) -> Self {
        Self {
            connection_manager,
            check_interval: Duration::from_secs(30),
        }
    }

    pub fn with_interval(connection_manager: Arc<ConnectionManager>, interval: Duration) -> Self {
        Self {
            connection_manager,
            check_interval: interval,
        }
    }

    /// Start the timeout monitoring task
    pub fn start(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut ticker = interval(self.check_interval);

            loop {
                ticker.tick().await;
                self.connection_manager.process_timeouts().await;
            }
        })
    }
}
