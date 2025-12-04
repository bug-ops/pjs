// Comprehensive tests for timeout monitoring infrastructure
//
// CRITICAL SECURITY MODULE - Requires 100% test coverage
//
// This test suite covers:
// - TimeoutMonitor creation and configuration
// - Timeout detection and processing
// - Integration with ConnectionManager
// - Background task lifecycle
// - Error handling and edge cases
//
// Coverage target: 100%

use pjson_rs::domain::services::ConnectionManager;
use pjson_rs::domain::value_objects::SessionId;
use pjson_rs::infrastructure::services::timeout_monitor::TimeoutMonitor;
use std::sync::Arc;
use std::time::Duration;

// ============================================================================
// TimeoutMonitor Construction Tests
// ============================================================================

#[test]
fn test_timeout_monitor_new() {
    let manager = Arc::new(ConnectionManager::new(Duration::from_secs(60), 100));
    let monitor = TimeoutMonitor::new(manager);

    // Verify monitor created successfully
    drop(monitor);
}

#[test]
fn test_timeout_monitor_with_interval() {
    let manager = Arc::new(ConnectionManager::new(Duration::from_secs(60), 100));
    let custom_interval = Duration::from_millis(500);
    let monitor = TimeoutMonitor::with_interval(manager, custom_interval);

    // Verify custom interval constructor works
    drop(monitor);
}

#[test]
fn test_timeout_monitor_default_interval() {
    let manager = Arc::new(ConnectionManager::new(Duration::from_secs(60), 100));
    let monitor = TimeoutMonitor::new(manager);

    // Default interval should be 30 seconds
    // Since we can't inspect internal state, just verify construction
    drop(monitor);
}

#[test]
fn test_timeout_monitor_custom_intervals() {
    let manager = Arc::new(ConnectionManager::new(Duration::from_secs(60), 100));

    // Test various interval values
    let intervals = [
        Duration::from_millis(100),
        Duration::from_millis(500),
        Duration::from_secs(1),
        Duration::from_secs(5),
        Duration::from_secs(30),
        Duration::from_secs(60),
    ];

    for interval in intervals {
        let monitor = TimeoutMonitor::with_interval(Arc::clone(&manager), interval);
        drop(monitor);
    }
}

// ============================================================================
// Background Task Lifecycle Tests
// ============================================================================

#[tokio::test]
async fn test_timeout_monitor_start() {
    let manager = Arc::new(ConnectionManager::new(Duration::from_secs(60), 100));
    let monitor = TimeoutMonitor::with_interval(Arc::clone(&manager), Duration::from_millis(50));

    // Start the monitoring task
    let handle = monitor.start();

    // Task should be running
    assert!(!handle.is_finished());

    // Stop the task
    handle.abort();

    // Wait a bit for cleanup
    tokio::time::sleep(Duration::from_millis(100)).await;
}

#[tokio::test]
async fn test_timeout_monitor_multiple_starts() {
    let manager = Arc::new(ConnectionManager::new(Duration::from_secs(60), 100));

    // Start multiple monitors (independent instances)
    let monitor1 = TimeoutMonitor::with_interval(Arc::clone(&manager), Duration::from_millis(50));
    let monitor2 = TimeoutMonitor::with_interval(Arc::clone(&manager), Duration::from_millis(50));

    let handle1 = monitor1.start();
    let handle2 = monitor2.start();

    assert!(!handle1.is_finished());
    assert!(!handle2.is_finished());

    handle1.abort();
    handle2.abort();

    tokio::time::sleep(Duration::from_millis(100)).await;
}

#[tokio::test]
async fn test_timeout_monitor_stop_and_restart() {
    let manager = Arc::new(ConnectionManager::new(Duration::from_secs(60), 100));

    let monitor1 = TimeoutMonitor::with_interval(Arc::clone(&manager), Duration::from_millis(50));
    let handle1 = monitor1.start();

    // Stop first task
    handle1.abort();
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Start new task
    let monitor2 = TimeoutMonitor::with_interval(Arc::clone(&manager), Duration::from_millis(50));
    let handle2 = monitor2.start();

    assert!(!handle2.is_finished());

    handle2.abort();
    tokio::time::sleep(Duration::from_millis(100)).await;
}

// ============================================================================
// Timeout Detection Tests
// ============================================================================

#[tokio::test]
async fn test_timeout_monitor_detects_timeout() {
    // Create manager with very short timeout for testing
    let manager = Arc::new(ConnectionManager::new(Duration::from_millis(100), 100));
    let session_id = SessionId::new();

    // Register a connection
    manager.register_connection(session_id).await.unwrap();

    // Verify connection is active
    let conn = manager.get_connection(&session_id).await;
    assert!(conn.is_some());
    assert!(conn.unwrap().is_active);

    // Start monitor with frequent checks
    let monitor = TimeoutMonitor::with_interval(Arc::clone(&manager), Duration::from_millis(50));
    let handle = monitor.start();

    // Wait for timeout to occur
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Connection should be closed
    let conn_after = manager.get_connection(&session_id).await;
    assert!(conn_after.is_some());
    assert!(!conn_after.unwrap().is_active);

    handle.abort();
}

#[tokio::test]
async fn test_timeout_monitor_multiple_connections() {
    let manager = Arc::new(ConnectionManager::new(Duration::from_millis(100), 100));

    // Register multiple connections
    let session_ids: Vec<SessionId> = (0..5).map(|_| SessionId::new()).collect();

    for session_id in &session_ids {
        manager.register_connection(*session_id).await.unwrap();
    }

    // Start monitor
    let monitor = TimeoutMonitor::with_interval(Arc::clone(&manager), Duration::from_millis(50));
    let handle = monitor.start();

    // Wait for timeouts
    tokio::time::sleep(Duration::from_millis(200)).await;

    // All connections should be closed
    for session_id in &session_ids {
        let conn = manager.get_connection(session_id).await;
        assert!(conn.is_some());
        assert!(!conn.unwrap().is_active);
    }

    handle.abort();
}

#[tokio::test]
async fn test_timeout_monitor_activity_prevents_timeout() {
    let manager = Arc::new(ConnectionManager::new(Duration::from_millis(200), 100));
    let session_id = SessionId::new();

    manager.register_connection(session_id).await.unwrap();

    // Start monitor
    let monitor = TimeoutMonitor::with_interval(Arc::clone(&manager), Duration::from_millis(50));
    let handle = monitor.start();

    // Periodically update activity
    for _ in 0..5 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        manager.update_activity(&session_id).await.unwrap();
    }

    // Connection should still be active
    let conn = manager.get_connection(&session_id).await;
    assert!(conn.is_some());
    assert!(conn.unwrap().is_active);

    handle.abort();
}

#[tokio::test]
async fn test_timeout_monitor_selective_timeouts() {
    let manager = Arc::new(ConnectionManager::new(Duration::from_millis(100), 100));

    // Register two connections
    let session1 = SessionId::new();
    let session2 = SessionId::new();

    manager.register_connection(session1).await.unwrap();
    manager.register_connection(session2).await.unwrap();

    // Start monitor
    let monitor = TimeoutMonitor::with_interval(Arc::clone(&manager), Duration::from_millis(50));
    let handle = monitor.start();

    // Keep session2 active
    tokio::time::sleep(Duration::from_millis(75)).await;
    manager.update_activity(&session2).await.unwrap();

    // Wait for session1 to timeout
    tokio::time::sleep(Duration::from_millis(100)).await;

    // session1 should be timed out, session2 should be active
    let conn1 = manager.get_connection(&session1).await;
    let conn2 = manager.get_connection(&session2).await;

    assert!(conn1.is_some());
    assert!(!conn1.unwrap().is_active);

    assert!(conn2.is_some());
    assert!(conn2.unwrap().is_active);

    handle.abort();
}

// ============================================================================
// Error Handling and Edge Cases
// ============================================================================

#[tokio::test]
async fn test_timeout_monitor_no_connections() {
    let manager = Arc::new(ConnectionManager::new(Duration::from_secs(60), 100));

    // Start monitor with no connections
    let monitor = TimeoutMonitor::with_interval(Arc::clone(&manager), Duration::from_millis(50));
    let handle = monitor.start();

    // Should run without errors
    tokio::time::sleep(Duration::from_millis(200)).await;

    handle.abort();
}

#[tokio::test]
async fn test_timeout_monitor_connection_removed() {
    let manager = Arc::new(ConnectionManager::new(Duration::from_millis(100), 100));
    let session_id = SessionId::new();

    manager.register_connection(session_id).await.unwrap();

    // Start monitor
    let monitor = TimeoutMonitor::with_interval(Arc::clone(&manager), Duration::from_millis(50));
    let handle = monitor.start();

    // Remove connection manually
    manager.remove_connection(&session_id).await.unwrap();

    // Monitor should handle missing connection gracefully
    tokio::time::sleep(Duration::from_millis(200)).await;

    handle.abort();
}

#[tokio::test]
async fn test_timeout_monitor_concurrent_access() {
    let manager = Arc::new(ConnectionManager::new(Duration::from_millis(100), 100));
    let session_id = SessionId::new();

    manager.register_connection(session_id).await.unwrap();

    // Start monitor
    let monitor = TimeoutMonitor::with_interval(Arc::clone(&manager), Duration::from_millis(30));
    let handle = monitor.start();

    // Concurrent access from multiple tasks
    let manager_clone1 = Arc::clone(&manager);
    let manager_clone2 = Arc::clone(&manager);

    let task1 = tokio::spawn(async move {
        for _ in 0..10 {
            let _ = manager_clone1.update_activity(&session_id).await;
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    });

    let task2 = tokio::spawn(async move {
        for _ in 0..10 {
            let _ = manager_clone2.get_connection(&session_id).await;
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    });

    task1.await.unwrap();
    task2.await.unwrap();

    handle.abort();
}

#[tokio::test]
async fn test_timeout_monitor_rapid_registration() {
    let manager = Arc::new(ConnectionManager::new(Duration::from_millis(100), 100));

    // Start monitor
    let monitor = TimeoutMonitor::with_interval(Arc::clone(&manager), Duration::from_millis(50));
    let handle = monitor.start();

    // Rapidly register and remove connections
    for _ in 0..20 {
        let session_id = SessionId::new();
        manager.register_connection(session_id).await.unwrap();
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    tokio::time::sleep(Duration::from_millis(200)).await;

    handle.abort();
}

// ============================================================================
// Performance and Stress Tests
// ============================================================================

#[tokio::test]
async fn test_timeout_monitor_high_frequency_checks() {
    let manager = Arc::new(ConnectionManager::new(Duration::from_secs(60), 100));

    // Very frequent checks (every 10ms)
    let monitor = TimeoutMonitor::with_interval(Arc::clone(&manager), Duration::from_millis(10));
    let handle = monitor.start();

    // Run for a short period
    tokio::time::sleep(Duration::from_millis(200)).await;

    handle.abort();
}

#[tokio::test]
async fn test_timeout_monitor_many_connections() {
    let manager = Arc::new(ConnectionManager::new(Duration::from_secs(60), 1000));

    // Register many connections
    for _ in 0..100 {
        let session_id = SessionId::new();
        manager.register_connection(session_id).await.unwrap();
    }

    // Start monitor
    let monitor = TimeoutMonitor::with_interval(Arc::clone(&manager), Duration::from_millis(50));
    let handle = monitor.start();

    // Monitor should handle many connections efficiently
    tokio::time::sleep(Duration::from_millis(200)).await;

    handle.abort();
}

#[tokio::test]
async fn test_timeout_monitor_long_running() {
    let manager = Arc::new(ConnectionManager::new(Duration::from_millis(100), 100));

    // Start monitor for extended period
    let monitor = TimeoutMonitor::with_interval(Arc::clone(&manager), Duration::from_millis(50));
    let handle = monitor.start();

    // Run for longer duration
    tokio::time::sleep(Duration::from_millis(500)).await;

    handle.abort();
}

// ============================================================================
// Integration Tests
// ============================================================================

#[tokio::test]
async fn test_timeout_monitor_full_lifecycle() {
    let manager = Arc::new(ConnectionManager::new(Duration::from_millis(150), 100));

    // Register connections
    let session1 = SessionId::new();
    let session2 = SessionId::new();

    manager.register_connection(session1).await.unwrap();
    manager.register_connection(session2).await.unwrap();

    // Start monitor
    let monitor = TimeoutMonitor::with_interval(Arc::clone(&manager), Duration::from_millis(50));
    let handle = monitor.start();

    // Session 1: Let it timeout
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Session 1 should be closed
    let conn1 = manager.get_connection(&session1).await;
    assert!(!conn1.unwrap().is_active);

    // Session 2: Keep active, then let timeout
    manager.update_activity(&session2).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Session 2 should now be closed too
    let conn2 = manager.get_connection(&session2).await;
    assert!(!conn2.unwrap().is_active);

    handle.abort();
}

#[tokio::test]
async fn test_timeout_monitor_with_metrics_updates() {
    let manager = Arc::new(ConnectionManager::new(Duration::from_millis(200), 100));
    let session_id = SessionId::new();

    manager.register_connection(session_id).await.unwrap();

    // Start monitor
    let monitor = TimeoutMonitor::with_interval(Arc::clone(&manager), Duration::from_millis(50));
    let handle = monitor.start();

    // Update metrics (which also updates activity)
    for _ in 0..5 {
        manager.update_metrics(&session_id, 100, 50).await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // Connection should still be active
    let conn = manager.get_connection(&session_id).await;
    assert!(conn.unwrap().is_active);

    handle.abort();
}

// ============================================================================
// Edge Cases and Boundary Conditions
// ============================================================================

#[tokio::test]
async fn test_timeout_monitor_zero_interval() {
    let manager = Arc::new(ConnectionManager::new(Duration::from_secs(60), 100));

    // Zero interval - should still work
    let monitor = TimeoutMonitor::with_interval(Arc::clone(&manager), Duration::from_millis(0));
    let handle = monitor.start();

    tokio::time::sleep(Duration::from_millis(100)).await;

    handle.abort();
}

#[tokio::test]
async fn test_timeout_monitor_very_long_interval() {
    let manager = Arc::new(ConnectionManager::new(Duration::from_secs(60), 100));

    // Very long interval
    let monitor = TimeoutMonitor::with_interval(Arc::clone(&manager), Duration::from_secs(3600));
    let handle = monitor.start();

    // Should start without issues
    tokio::time::sleep(Duration::from_millis(100)).await;

    handle.abort();
}

#[tokio::test]
async fn test_timeout_monitor_immediate_abort() {
    let manager = Arc::new(ConnectionManager::new(Duration::from_secs(60), 100));
    let monitor = TimeoutMonitor::with_interval(Arc::clone(&manager), Duration::from_millis(50));

    let handle = monitor.start();

    // Abort immediately
    handle.abort();
}

#[tokio::test]
async fn test_timeout_monitor_with_closed_connections() {
    let manager = Arc::new(ConnectionManager::new(Duration::from_millis(100), 100));
    let session_id = SessionId::new();

    manager.register_connection(session_id).await.unwrap();

    // Close connection manually
    manager.close_connection(&session_id).await.unwrap();

    // Start monitor
    let monitor = TimeoutMonitor::with_interval(Arc::clone(&manager), Duration::from_millis(50));
    let handle = monitor.start();

    // Should handle already-closed connections
    tokio::time::sleep(Duration::from_millis(200)).await;

    handle.abort();
}

#[tokio::test]
async fn test_timeout_monitor_task_cancellation_safe() {
    let manager = Arc::new(ConnectionManager::new(Duration::from_secs(60), 100));
    let monitor = TimeoutMonitor::with_interval(Arc::clone(&manager), Duration::from_millis(50));

    let handle = monitor.start();

    // Cancel task multiple times
    handle.abort();
    handle.abort();
    handle.abort();
}

// ============================================================================
// Documentation and Usage Pattern Tests
// ============================================================================

#[tokio::test]
async fn test_timeout_monitor_typical_usage() {
    // Typical usage pattern from documentation
    let connection_manager = Arc::new(ConnectionManager::new(Duration::from_secs(300), 1000));

    let timeout_monitor = TimeoutMonitor::new(Arc::clone(&connection_manager));

    let _monitor_handle = timeout_monitor.start();

    // Monitor runs in background checking for timeouts
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Cleanup
    _monitor_handle.abort();
}

#[tokio::test]
async fn test_timeout_monitor_custom_interval_usage() {
    // Custom interval usage pattern
    let connection_manager = Arc::new(ConnectionManager::new(Duration::from_secs(300), 1000));

    let timeout_monitor =
        TimeoutMonitor::with_interval(Arc::clone(&connection_manager), Duration::from_secs(60));

    let _monitor_handle = timeout_monitor.start();

    tokio::time::sleep(Duration::from_millis(100)).await;

    _monitor_handle.abort();
}
