use anyhow::{Result, anyhow};
use serde_json::Value;
use std::sync::{Arc, Mutex};
use std::thread;
use std::sync::mpsc;
use super::executor::{ManaScriptExecutor, ExecutionTask};
use super::config::DEBUG_MODE;

/// V8 thread pool for concurrent script execution
pub struct V8ThreadPool {
    task_sender: mpsc::Sender<ExecutionTask>,
}

impl V8ThreadPool {
    /// Create a new thread pool with specified size
    pub fn new(pool_size: usize) -> Self {
        let (task_sender, task_receiver) = mpsc::channel::<ExecutionTask>();
        let task_receiver = Arc::new(Mutex::new(task_receiver));

        for thread_id in 0..pool_size {
            let receiver = Arc::clone(&task_receiver);
            thread::spawn(move || {
                let mut executor = ManaScriptExecutor::new(thread_id);
                
                let debug_enabled = DEBUG_MODE.get().copied().unwrap_or(false);
                if debug_enabled {
                    println!("🧵 Thread {} initialized with reusable V8 isolate + @mana-app/emulator", thread_id);
                }
                
                loop {
                    let task = {
                        let receiver = receiver.lock().unwrap();
                        receiver.recv()
                    };

                    match task {
                        Ok(task) => {
                            let dequeue_time = std::time::Instant::now();
                            let queue_wait_time = dequeue_time.duration_since(task.enqueue_time);
                            
                            let result = executor.execute_mana_script(
                                &task.js_code,
                                task.task_id,
                                queue_wait_time
                            );
                            let _ = task.response_sender.send(result);
                        }
                        Err(_) => break,
                    }
                }
            });
        }

        Self { task_sender }
    }

    /// Execute JavaScript code asynchronously in the thread pool
    pub async fn execute(&self, js_code: String, task_id: usize) -> Result<Value> {
        let (response_sender, response_receiver) = tokio::sync::oneshot::channel();
        let task = ExecutionTask {
            js_code,
            response_sender,
            task_id,
            enqueue_time: std::time::Instant::now(),
        };

        self.task_sender.send(task).map_err(|_| anyhow!("Thread pool is shut down"))?;
        response_receiver.await.map_err(|_| anyhow!("Worker thread disconnected"))?
    }
}

