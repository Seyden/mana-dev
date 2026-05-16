use anyhow::Result;
use serde_json::Value;
use super::engine::V8Engine;
use super::config::DEBUG_MODE;

/// High-level script executor that manages V8Engine
pub struct ManaScriptExecutor {
    engine: V8Engine,
}

impl ManaScriptExecutor {
    /// Create a new script executor
    pub fn new(thread_id: usize) -> Self {
        let engine = V8Engine::new(thread_id);
        Self { engine }
    }

    /// Execute a mana script and return the target info
    pub fn execute_mana_script(
        &mut self, 
        js_code: &str, 
        task_id: usize, 
        queue_wait_time: std::time::Duration
    ) -> Result<Value> {
        let execution_start = std::time::Instant::now();
        let execution_start_ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        
        let result = self.engine.execute_script(js_code);
        
        let execution_time = execution_start.elapsed();
        let execution_end_ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        
        let debug_enabled = DEBUG_MODE.get().copied().unwrap_or(false);
        if debug_enabled {
            println!("🔍 Task #{}: [{}→{}] Queue={:.1}ms | Execution={:.1}ms | Total={:.1}ms", 
                     task_id,
                     execution_start_ts,
                     execution_end_ts,
                     queue_wait_time.as_secs_f64() * 1000.0,
                     execution_time.as_secs_f64() * 1000.0,
                     (queue_wait_time + execution_time).as_secs_f64() * 1000.0);
        }
        
        result
    }
}

/// Task data structure for thread pool communication
pub struct ExecutionTask {
    pub js_code: String,
    pub response_sender: tokio::sync::oneshot::Sender<Result<Value>>,
    pub task_id: usize,
    pub enqueue_time: std::time::Instant,
}

