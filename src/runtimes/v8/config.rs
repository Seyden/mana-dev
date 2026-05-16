use std::sync::OnceLock;
use super::emulator::get_emulator_code;
use super::thread_pool::V8ThreadPool;

/// Global configuration storage
pub static THREAD_POOL: OnceLock<V8ThreadPool> = OnceLock::new();
pub static TASK_COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
pub static THREAD_CONFIG: OnceLock<(usize, usize)> = OnceLock::new();
pub static DEBUG_MODE: OnceLock<bool> = OnceLock::new();

/// Initialize V8 platform with optimized settings
pub fn initialize_v8_platform(pool_size: usize) {
    let v8_background_threads = if pool_size > 12 {
        (pool_size / 2).min(12)
    } else {
        pool_size.max(2)
    };
    
    let platform = v8::new_default_platform(v8_background_threads as u32, false).make_shared();
    v8::V8::initialize_platform(platform);
    v8::V8::initialize();
    
    // Pre-load emulator code once during platform initialization
    get_emulator_code();
}

/// Get or create thread pool with specified size
pub fn get_thread_pool_with_size(requested_threads: usize, task_count: usize) -> &'static V8ThreadPool {
    THREAD_POOL.get_or_init(|| {
        let cpu_cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        
        let pool_size = requested_threads
            .min(task_count)
            .max(1);
            
        let v8_background_threads = if pool_size > 12 {
            (pool_size / 2).min(12)
        } else {
            pool_size.max(2)
        };
        
        initialize_v8_platform(pool_size);
        
        println!("🧵 Initializing V8 thread pool with {} threads + {} background threads (requested: {}, tasks: {}, CPU cores: {})", 
                 pool_size, v8_background_threads, requested_threads, task_count, cpu_cores);
        
        V8ThreadPool::new(pool_size)
    })
}

/// Get the configured thread pool
pub fn get_thread_pool() -> &'static V8ThreadPool {
    let (requested_threads, task_count) = THREAD_CONFIG.get().copied().unwrap_or((4, 1));
    get_thread_pool_with_size(requested_threads, task_count)
}

/// Configure thread pool parameters
pub fn configure_thread_pool(requested_threads: usize, task_count: usize) {
    THREAD_CONFIG.set((requested_threads, task_count)).ok();
}

/// Configure debug mode
pub fn configure_debug_mode(debug: bool) {
    DEBUG_MODE.set(debug).ok();
}

