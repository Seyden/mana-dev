use anyhow::{Result, anyhow};
use clap::{Parser, Subcommand};
use instant::Instant;
use std::sync::Arc;
use std::path::Path;
use tokio::sync::Semaphore;
use futures::future::join_all;

mod runtimes;

use runtimes::v8;

#[derive(Parser)]
#[command(author, version, about = "Mana Development Tools", long_about = None)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Build TypeScript/JavaScript sources in the project directory
    #[command(alias = "b")]
    Build {
        /// Skip generating supporting files for hosting this source repository
        #[arg(long)]
        no_repo: bool,
        
        /// Specify the output folder (relative to current directory)
        #[arg(short, long)]
        folder: Option<String>,
        
        /// Specify the full output path (absolute or relative)
        #[arg(short, long)]
        output: Option<String>,
        
        /// Source directory or file to build from (defaults to current directory)
        #[arg(value_name = "SOURCE")]
        source: Option<String>,
    },
    /// Benchmark V8 JavaScript runtime performance
    Benchmark {
        /// Path to the .mana (JavaScript) file to execute and evaluate, or directory to scan for .mana files
        #[arg(short, long)]
        file: String,
        
        /// Scan directory for all .mana files instead of testing single file
        #[arg(long)]
        scan_directory: bool,
        
        /// Number of iterations for benchmarking
        #[arg(short, long, default_value = "3")]
        iterations: usize,
        
        /// Number of concurrent threads for parallel execution (0 = auto-detect based on CPU cores)
        #[arg(long, default_value = "0")]
        threads: usize,

        /// Enable debug logging for detailed task execution information
        #[arg(long, default_value = "false")]
        debug: bool,
    },
}

#[derive(Debug, Clone)]
struct BenchmarkResult {
    runtime_name: String,
    file_name: String,
    execution_time: std::time::Duration,
    target_info: Option<serde_json::Value>,
    error: Option<String>,
}

#[derive(Debug)]
struct RuntimeStats {
    runtime_name: String,
    total_files: usize,
    successful_files: usize,
    failed_files: usize,
    total_time: std::time::Duration,
    average_time: std::time::Duration,
    min_time: std::time::Duration,
    max_time: std::time::Duration,
    fastest_file: String,
    slowest_file: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    
    match args.command {
        Commands::Build { no_repo, folder, output, source } => {
            build_command(no_repo, folder, output, source).await
        },
        Commands::Benchmark { file, scan_directory, iterations, mut threads, debug } => {
            // Auto-detect CPU cores if threads = 0
            if threads == 0 {
                threads = std::thread::available_parallelism()
                    .map(|n| n.get())
                    .unwrap_or(4); // Fallback to 4 if detection fails
                println!("🔍 Auto-detected {} CPU cores, using {} threads", threads, threads);
            }
            
            if scan_directory {
                scan_directory_benchmark(&file, iterations, threads, debug).await
            } else {
                single_file_benchmark(&file, iterations, threads, debug).await
            }
        }
    }
}

async fn build_command(no_repo: bool, folder: Option<String>, output: Option<String>, source: Option<String>) -> Result<()> {
    use std::path::Path;
    
    println!("🔨 Building TypeScript/JavaScript sources...");
    
    let base_path = std::env::current_dir()?;
    
    // Determine source directory - default to current directory
    let source_path = source.unwrap_or_else(|| ".".to_string());
    
    // Determine output directory - priority: output > folder > default
    let out_dir = if let Some(output) = output {
        // If output is specified, use it as-is (can be absolute or relative)
        if Path::new(&output).is_absolute() {
            output
        } else {
            base_path.join(&output).to_string_lossy().to_string()
        }
    } else {
        // Fallback to folder option (relative to current directory) or default
        base_path.join(folder.unwrap_or_else(|| "dist".to_string())).to_string_lossy().to_string()
    };
    
    println!("📁 Source: {}", source_path);
    println!("📁 Output: {}", out_dir);
    
    // Create output directory
    std::fs::create_dir_all(&out_dir)?;
    println!("✓ Created output directory: {}", out_dir);
    
    // Find and bundle TypeScript entry points
    let entry_points = find_source_entry_points(&source_path).await?;
    
    if entry_points.is_empty() {
        return Err(anyhow!("No valid TypeScript entry points found with 'class Target' in: {}", source_path));
    }
    
    println!("🔍 Found {} entry point(s):", entry_points.len());
    for entry in &entry_points {
        println!("  - {}", entry.display());
    }
    
    // Use native rolldown bulk bundling with self-contained IIFE configuration
    v8::bulk_build_emulator_native_standalone(entry_points, &out_dir).await?;
    
    // Copy assets if they exist
    let assets_dir = base_path.join("assets");
    if assets_dir.exists() {
        let dest_assets = Path::new(&out_dir).join("assets");
        copy_dir_recursive(&assets_dir, &dest_assets)?;
        println!("📋 Copied assets directory");
    }
    
    if !no_repo {
        println!("📝 Repository generation skipped (not implemented yet)");
    }
    
        println!("✅ Build completed successfully!");
    Ok(())
}

// Test function to demonstrate in-memory bundling
pub async fn test_in_memory_bundling() -> Result<()> {
    println!("🧪 Testing in-memory bundling...");
    
    // Bundle a TypeScript file directly into memory
    match v8::build_emulator_in_memory("test_in_memory.ts", Some("test_bundle")).await {
        Ok(bundled_code) => {
            println!("✅ Successfully bundled in memory!");
            println!("📄 Bundled code size: {} bytes", bundled_code.len());
            println!("📋 Code preview (first 200 chars): {}", 
                &bundled_code.chars().take(200).collect::<String>());
        }
        Err(e) => {
            println!("❌ In-memory bundling failed: {}", e);
        }
    }
    
    Ok(())
}

async fn find_source_entry_points(source_path: &str) -> Result<Vec<std::path::PathBuf>> {
    use std::path::Path;
    
    let mut entry_points = Vec::new();
    let source_dir = Path::new(source_path);
    
    if source_dir.is_file() {
        // Single file - check if it contains 'class Target'
        let content = tokio::fs::read_to_string(source_dir).await?;
        if content.contains("class Target") {
            entry_points.push(source_dir.to_path_buf());
        }
    } else {
        // Directory - recursively find all TypeScript files with 'class Target'
        visit_dir_for_targets(source_dir, &mut entry_points)?;
    }
    
    Ok(entry_points)
}

fn visit_dir_for_targets(dir: &Path, entry_points: &mut Vec<std::path::PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        
        if path.is_dir() {
            // Skip common ignore directories
            if let Some(name) = path.file_name() {
                let name = name.to_string_lossy();
                if name == "node_modules" || name == ".git" || name == "dist" || name == "build" {
                    continue;
                }
            }
            visit_dir_for_targets(&path, entry_points)?;
        } else if let Some(ext) = path.extension() {
            if ext == "ts" || ext == "tsx" {
                let content = std::fs::read_to_string(&path)?;
                if content.contains("class Target") {
                    entry_points.push(path);
                }
            }
        }
    }
    
    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let dest = dst.join(entry.file_name());
        
        if path.is_dir() {
            copy_dir_recursive(&path, &dest)?;
        } else {
            std::fs::copy(&path, &dest)?;
        }
    }
    
    Ok(())
}

fn generate_uuid() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    
    // Generate a simple UUID-like string using timestamp and random component
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    
    // Create a simple hash-like string
    format!("{:x}", timestamp % 0xFFFFFFFF)
}

async fn single_file_benchmark(file: &str, iterations: usize, threads: usize, debug: bool) -> Result<()> {
    // Read the JavaScript file using tokio async read_to_string
    let file_read_start = Instant::now();
    let js_content = tokio::fs::read_to_string(file).await
        .map_err(|e| anyhow!("Failed to read file '{}': {}", file, e))?;
    let file_read_time = file_read_start.elapsed();
    
    println!("Loaded file: {}", file);
    println!("File size: {} bytes", js_content.len());
    println!("File read time: {:.1}ms (TOKIO_ASYNC)", file_read_time.as_secs_f64() * 1000.0);
    
    let file_name = std::path::Path::new(file)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    
    let mut results = Vec::new();
    
    for runtime_name in "v8".split(',') {
        let runtime_name = runtime_name.trim();
        println!("\n=== Testing {} Runtime ===", runtime_name.to_uppercase());
        
        // Configure V8 thread pool for single file (1 task)
        match runtime_name {
            "v8" => {
                v8::configure_thread_pool(threads, 1);
                v8::configure_debug_mode(debug);
            }
            _ => {}
        }
        
        for i in 0..iterations {
            print!("Iteration {}/{} ... ", i + 1, iterations);
            
            let start = Instant::now();
            let result = execute_runtime(runtime_name, &js_content).await;
            let execution_time = start.elapsed();
            
            let benchmark_result = match result {
                Ok(target_info) => {
                    BenchmarkResult {
                        runtime_name: runtime_name.to_string(),
                        file_name: file_name.clone(),
                        execution_time,
                        target_info: Some(target_info),
                        error: None,
                    }
                }
                Err(e) => {
                    println!("✗ Error: {}", e);
                    BenchmarkResult {
                        runtime_name: runtime_name.to_string(),
                        file_name: file_name.clone(),
                        execution_time,
                        target_info: None,
                        error: Some(e.to_string()),
                    }
                }
            };
            
            results.push(benchmark_result);
        }
    }
    
    print_single_file_summary(&results);
    Ok(())
}

async fn scan_directory_benchmark(file: &str, iterations: usize, threads: usize, debug: bool) -> Result<()> {
    // Find all .mana files in the directory
    let mana_files = find_mana_files(file)?;
    let runtimes: Vec<&str> = "v8".split(',').map(|s| s.trim()).collect();
    
    println!("🔍 Found {} .mana files in directory: {}", mana_files.len(), file);
    println!("🧪 Testing {} iterations per file per runtime", iterations);
    println!("🚀 Using {} concurrent threads per runtime (sequential runtime execution)", threads);
    println!("📊 Total tests: {} files × {} runtimes × {} iterations = {} tests\n", 
             mana_files.len(), 
             runtimes.len(), 
             iterations,
             mana_files.len() * runtimes.len() * iterations);
    
    
    let mut all_results = Vec::new();
    let mut runtime_durations = std::collections::HashMap::new();
    let overall_start_time = Instant::now();
    
    // Pre-warm V8 cache if V8 is being used
    
    // Process each runtime sequentially
    for (runtime_idx, runtime_name) in runtimes.iter().enumerate() {
        println!("🏁 === RUNTIME {}/{}: {} ===", runtime_idx + 1, runtimes.len(), runtime_name.to_uppercase());
        
        // Configure V8 thread pool based on workload and user preference
        match runtime_name.as_ref() {
            "v8" => {
                v8::configure_thread_pool(threads, mana_files.len());
                v8::configure_debug_mode(debug);
            }
            _ => {}
        }
        
        let runtime_start_time = Instant::now();
        
        // Create semaphore to limit concurrent tasks for this runtime  
        let runtime_semaphore = Arc::new(Semaphore::new(threads)); // Properly limit to specified thread count
        
        if debug {
            println!("  📂 Files will be loaded using tokio::fs::read_to_string (async, no caching)");
        }
        
        let mut runtime_tasks = Vec::new();
        
        // Create tasks for all files for this runtime
        for (file_index, mana_file) in mana_files.iter().enumerate() {
            let file_name = std::path::Path::new(mana_file)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            
            let task = create_runtime_specific_task(
                runtime_semaphore.clone(),
                file_index + 1,
                mana_files.len(),
                file_name.clone(),
                mana_file.clone(), // Pass file path instead of content
                runtime_name.to_string(),
                iterations,
                debug,
            );
            runtime_tasks.push(task);
        }
        
        println!("  🏃 Processing {} files with {} threads...", runtime_tasks.len(), threads);
        
        // Execute all tasks for this runtime in parallel
        let parallel_start = Instant::now();
        let parallel_start_ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let task_count = runtime_tasks.len();
        
        let runtime_results = join_all(runtime_tasks).await;
        
        let parallel_time = parallel_start.elapsed();
        let parallel_end_ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        
        if debug {
            println!("  📊 Parallel execution [{}→{}] completed in {:.1}ms ({} tasks)", 
                     parallel_start_ts,
                     parallel_end_ts,
                     parallel_time.as_secs_f64() * 1000.0, 
                     task_count);
        }
        
        // Flatten results for this runtime
        let mut runtime_flattened_results = Vec::new();
        for task_results in runtime_results {
            match task_results {
                Ok(results) => runtime_flattened_results.extend(results),
                Err(e) => eprintln!("  Task failed: {}", e),
            }
        }
        
        let runtime_duration = runtime_start_time.elapsed();
        let successful_files = runtime_flattened_results.iter()
            .filter(|r| r.error.is_none())
            .map(|r| r.file_name.clone())
            .collect::<std::collections::HashSet<_>>()
            .len();
        
        println!("  ✅ {} completed in {:.2}s ({}/{} files successful)", 
                 runtime_name.to_uppercase(), 
                 runtime_duration.as_secs_f64(),
                 successful_files,
                 mana_files.len());
        
        if successful_files > 0 {
            let avg_time_per_file = runtime_duration.as_secs_f64() / successful_files as f64;
            if debug {
                println!("  📊 Average time per file: {:.1}ms", avg_time_per_file * 1000.0);
            }
        }
        
        println!();
        
        // Store the actual runtime duration
        runtime_durations.insert(runtime_name.to_string(), runtime_duration);
        
        all_results.extend(runtime_flattened_results);
    }
    
    let total_time = overall_start_time.elapsed();
    println!("⏱️  Overall execution time: {:.2}s", total_time.as_secs_f64());
    
    print_directory_summary(&all_results, "v8", &runtime_durations);
    Ok(())
}

async fn create_runtime_specific_task(
    semaphore: Arc<Semaphore>,
    file_index: usize,
    total_files: usize,
    file_name: String,
    file_path: String,
    runtime_name: String,
    iterations: usize,
    debug: bool,
) -> Result<Vec<BenchmarkResult>> {
    let task_start = Instant::now();
    let _task_start_ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    
    let _permit = semaphore.acquire().await.unwrap();
    let _permit_acquired_ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    
    // Original permit log removed - now using completion log with timing metrics
    
    let mut results = Vec::new();
    let mut runtime_times = Vec::new();
    let mut _successful = 0;
    
    for _i in 0..iterations {
        // Load file using tokio async read_to_string
        let file_read_start = Instant::now();
        let js_content = match tokio::fs::read_to_string(&file_path).await {
            Ok(content) => content,
            Err(e) => {
                eprintln!("Failed to read file {}: {}", file_path, e);
                String::new()
            }
        };
        
        if js_content.is_empty() {
            let benchmark_result = BenchmarkResult {
                runtime_name: runtime_name.clone(),
                file_name: file_name.clone(),
                execution_time: std::time::Duration::from_millis(0),
                target_info: None,
                error: Some(format!("Failed to read file: {}", file_path)),
            };
            results.push(benchmark_result);
            continue;
        }
        let file_read_time = file_read_start.elapsed();
        
        let runtime_start = Instant::now();
        let result = execute_runtime(&runtime_name, &js_content).await;
        let runtime_execution_time = runtime_start.elapsed();
        let total_execution_time = task_start.elapsed();
        
        // Log detailed timing in the 📁 format with all metrics
        let runtime_end_ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        
        if debug {
            println!("  📁 {}/{} {} [{}ms] FileRead={:.1}ms | Execution={:.1}ms | Total={:.1}ms | Size={}bytes",
                     file_index,
                     total_files,
                     file_name,
                     runtime_end_ts,
                     file_read_time.as_secs_f64() * 1000.0,
                     runtime_execution_time.as_secs_f64() * 1000.0,
                     total_execution_time.as_secs_f64() * 1000.0,
                     js_content.len());
        }
        
        let benchmark_result = match result {
            Ok(target_info) => {
                _successful += 1;
                runtime_times.push(runtime_execution_time);
                BenchmarkResult {
                    runtime_name: runtime_name.clone(),
                    file_name: file_name.clone(),
                    execution_time: runtime_execution_time,
                    target_info: Some(target_info),
                    error: None,
                }
            }
            Err(e) => {
                BenchmarkResult {
                    runtime_name: runtime_name.clone(),
                    file_name: file_name.clone(),
                    execution_time: runtime_execution_time,
                    target_info: None,
                    error: Some(e.to_string()),
                }
            }
        };
        
        results.push(benchmark_result);
    }
    
    // Removed success output to reduce console noise
    
    Ok(results)
}


async fn execute_runtime(runtime_name: &str, js_content: &str) -> Result<serde_json::Value> {
    match runtime_name {
        "v8" => v8::execute_and_get_target_info(js_content).await,
        _ => Err(anyhow!("Unknown runtime: {}. Supported runtimes: 'v8'", runtime_name))
    }
}

fn find_mana_files(directory: &str) -> Result<Vec<String>> {
    let mut mana_files = Vec::new();
    
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        
        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("js") {
            mana_files.push(path.to_string_lossy().to_string());
        }
    }
    
    mana_files.sort();
    Ok(mana_files)
}

fn print_single_file_summary(results: &[BenchmarkResult]) {
    // Group results by runtime
    let mut runtime_results: std::collections::HashMap<String, Vec<&BenchmarkResult>> = std::collections::HashMap::new();
    for result in results {
        runtime_results.entry(result.runtime_name.clone()).or_insert_with(Vec::new).push(result);
    }
    
    // Print summary
    println!("\n=== BENCHMARK SUMMARY ===");
    for (runtime_name, runtime_results) in &runtime_results {
        let successful = runtime_results.iter().filter(|r| r.error.is_none()).count();
        let total = runtime_results.len();
        
        if successful == 0 {
            println!("{}: All executions failed", runtime_name.to_uppercase());
        } else {
            let times: Vec<std::time::Duration> = runtime_results.iter()
                .filter(|r| r.error.is_none())
                .map(|r| r.execution_time)
                .collect();
            
            let avg_time = times.iter().sum::<std::time::Duration>() / times.len() as u32;
            let min_time = times.iter().min().unwrap();
            let max_time = times.iter().max().unwrap();
            
            println!("{}: {}/{} successful", runtime_name.to_uppercase(), successful, total);
            println!("  Average: {:.2}ms", avg_time.as_secs_f64() * 1000.0);
            println!("  Min: {:.2}ms", min_time.as_secs_f64() * 1000.0);
            println!("  Max: {:.2}ms", max_time.as_secs_f64() * 1000.0);
        }
    }
    
    // Show target info from first successful result
    println!("\n=== TARGET.INFO ===");
    for result in results.iter() {
        if let Some(target_info) = &result.target_info {
            println!("{}", serde_json::to_string_pretty(target_info).unwrap_or_else(|_| "Invalid JSON".to_string()));
            break;
        }
    }
}

fn print_directory_summary(results: &[BenchmarkResult], runtimes_str: &str, runtime_durations: &std::collections::HashMap<String, std::time::Duration>) {
    let mut runtime_stats = std::collections::HashMap::new();
    
    // Calculate stats for each runtime
    for runtime_name in runtimes_str.split(',') {
        let runtime_name = runtime_name.trim();
        let runtime_results: Vec<&BenchmarkResult> = results.iter()
            .filter(|r| r.runtime_name == runtime_name)
            .collect();
        
        if runtime_results.is_empty() {
            continue;
        }
        
        let successful_results: Vec<&BenchmarkResult> = runtime_results.iter()
            .filter(|r| r.error.is_none())
            .cloned()
            .collect();
        
        if successful_results.is_empty() {
            runtime_stats.insert(runtime_name.to_string(), RuntimeStats {
                runtime_name: runtime_name.to_string(),
                total_files: runtime_results.len(),
                successful_files: 0,
                failed_files: runtime_results.len(),
                total_time: std::time::Duration::ZERO,
                average_time: std::time::Duration::ZERO,
                min_time: std::time::Duration::ZERO,
                max_time: std::time::Duration::ZERO,
                fastest_file: "N/A".to_string(),
                slowest_file: "N/A".to_string(),
            });
            continue;
        }
        
        let times: Vec<std::time::Duration> = successful_results.iter().map(|r| r.execution_time).collect();
        let total_time = runtime_durations.get(runtime_name).copied().unwrap_or_else(|| times.iter().sum::<std::time::Duration>());
        let average_time = times.iter().sum::<std::time::Duration>() / times.len() as u32;
        let min_time = *times.iter().min().unwrap();
        let max_time = *times.iter().max().unwrap();
        
        let fastest_result = successful_results.iter().min_by_key(|r| r.execution_time).unwrap();
        let slowest_result = successful_results.iter().max_by_key(|r| r.execution_time).unwrap();
        
        runtime_stats.insert(runtime_name.to_string(), RuntimeStats {
            runtime_name: runtime_name.to_string(),
            total_files: runtime_results.len(),
            successful_files: successful_results.len(),
            failed_files: runtime_results.len() - successful_results.len(),
            total_time,
            average_time,
            min_time,
            max_time,
            fastest_file: fastest_result.file_name.clone(),
            slowest_file: slowest_result.file_name.clone(),
        });
    }
    
    // Print comprehensive summary
    println!("\n🏆 === COMPREHENSIVE BENCHMARK SUMMARY ===");
    
    // Sort runtimes by average time
    let mut sorted_runtimes: Vec<_> = runtime_stats.values().collect();
    sorted_runtimes.sort_by_key(|stats| stats.average_time);
    
    for (rank, stats) in sorted_runtimes.iter().enumerate() {
        let medal = match rank {
            0 => "🥇",
            1 => "🥈", 
            2 => "🥉",
            _ => "  ",
        };
        
        println!("\n{} {} Runtime:", medal, stats.runtime_name.to_uppercase());
        println!("  📊 Success Rate: {}/{} files ({:.1}%)", 
                 stats.successful_files, 
                 stats.total_files,
                 (stats.successful_files as f64 / stats.total_files as f64) * 100.0);
        
        if stats.successful_files > 0 {
            println!("  ⚡ Average Time: {:.2}ms", stats.average_time.as_secs_f64() * 1000.0);
            println!("  🚀 Fastest: {:.2}ms ({})", stats.min_time.as_secs_f64() * 1000.0, stats.fastest_file);
            println!("  🐌 Slowest: {:.2}ms ({})", stats.max_time.as_secs_f64() * 1000.0, stats.slowest_file);
            println!("  🕒 Total Time: {:.2}s", stats.total_time.as_secs_f64());
        }
        
        if stats.failed_files > 0 {
            println!("  ❌ Failed Files: {}", stats.failed_files);
        }
    }
    
    // Performance comparison table
    if sorted_runtimes.len() > 1 {
        println!("\n📈 === PERFORMANCE COMPARISON ===");
        let fastest = &sorted_runtimes[0];
        
        for stats in &sorted_runtimes {
            if stats.successful_files > 0 {
                let multiplier = stats.average_time.as_secs_f64() / fastest.average_time.as_secs_f64();
                println!("{}: {:.2}ms ({:.1}x {})", 
                         stats.runtime_name.to_uppercase(),
                         stats.average_time.as_secs_f64() * 1000.0,
                         multiplier,
                         if multiplier == 1.0 { "baseline".to_string() } else { "slower".to_string() });
            }
        }
    }
}
