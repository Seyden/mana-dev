//! V8 JavaScript runtime implementation
//! 
//! This module provides a high-performance V8 JavaScript engine implementation
//! with optimized thread pooling, emulator management, and script execution.

pub mod emulator;
pub mod engine;
pub mod executor;
pub mod thread_pool;
pub mod config;

// Re-export public API
pub use config::{configure_thread_pool, configure_debug_mode};

use anyhow::Result;
use serde_json::Value;
use config::{get_thread_pool, TASK_COUNTER};

/// Execute JavaScript code and return target info
/// 
/// This is the main entry point for executing .mana files using V8.
/// The execution is performed in a thread pool for optimal performance.
pub async fn execute_and_get_target_info(js_code: &str) -> Result<Value> {
    let task_id = TASK_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let pool = get_thread_pool();
    pool.execute(js_code.to_string(), task_id).await
}
