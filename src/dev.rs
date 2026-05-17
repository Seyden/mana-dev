use anyhow::{Result, anyhow};
use axum::{
    extract::State,
    http::{Request, StatusCode},
    middleware::{self, Next},
    response::Response,
    routing::post,
    Router,
};
use clap::{Parser, Subcommand};
use colored::*;
use std::{
    net::SocketAddr,
    path::PathBuf,
    process::Stdio,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
};
use tower_http::{
    cors::CorsLayer,
    services::ServeDir,
};

mod runtimes;
use runtimes::v8::emulator;

// ── Project config ────────────────────────────────────────────────────────────

/// Resolved build configuration for a mana project.
/// Priority: mana.json → package.json["mana"] key → CLI flags → built-in defaults.
/// Also holds project metadata read from package.json (repositoryName, thumbnail).
#[derive(Debug, Clone)]
pub(crate) struct ManaConfig {
    pub src: String,
    pub out: String,
    pub target: String,
    pub minify: bool,
    pub platform: String,
    /// Read from package.json "repositoryName" or "name" field. None if not provided.
    pub repository_name: Option<String>,
    /// Read from package.json "thumbnail" field. None if not provided.
    pub thumbnail: Option<String>,
}

/// Loaded once at startup via `ManaConfig::init`. Guaranteed single evaluation.
pub(crate) static MANA_CONFIG: std::sync::OnceLock<(ManaConfig, &'static str)> = std::sync::OnceLock::new();

impl ManaConfig {
    /// Initialise the global config exactly once. Call this from `main()` before
    /// anything else. Subsequent calls return the already-resolved value.
    fn init(cli_src: Option<&str>, cli_out: Option<&str>) -> &'static (ManaConfig, &'static str) {
        MANA_CONFIG.get_or_init(|| Self::resolve(cli_src, cli_out))
    }

    /// Read and resolve config from disk. Only called by `init` via `OnceLock`.
    fn resolve(cli_src: Option<&str>, cli_out: Option<&str>) -> (Self, &'static str) {
        let mut cfg = Self::defaults();
        let source;

        // The config root is the CLI source argument if provided, otherwise cwd.
        // This means `mana-dev serve E:\Wow\Mana-sources` reads package.json from
        // that directory, not from wherever the binary was launched.
        let config_root = std::path::Path::new(cli_src.unwrap_or("."));

        let read_file = |name: &str| -> Option<serde_json::Value> {
            std::fs::read_to_string(config_root.join(name))
                .ok()
                .and_then(|raw| serde_json::from_str(&raw).ok())
        };

        // Always read package.json for repository metadata.
        let pkg_json = read_file("package.json");

        // Base layer: top-level package.json fields (lowest priority for metadata).
        if let Some(ref pkg) = pkg_json {
            cfg.repository_name = pkg.get("repositoryName")
                .and_then(|v| v.as_str())
                .or_else(|| pkg.get("name").and_then(|v| v.as_str()))
                .map(|s| s.to_string());
            cfg.thumbnail = pkg.get("thumbnail")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
        }

        // 1. Try mana.json — highest priority, overwrites everything above.
        if let Some(v) = read_file("mana.json") {
            cfg.merge_build_json(&v);
            cfg.merge_metadata_json(&v);
            source = "mana.json";
        }
        // 2. Fall back to package.json["mana"] key — overrides top-level package.json.
        else if let Some(ref pkg) = pkg_json {
            if let Some(mana) = pkg.get("mana") {
                cfg.merge_build_json(mana);
                cfg.merge_metadata_json(mana);
                source = "package.json (\"mana\" key)";
            } else {
                source = "defaults (no \"mana\" key in package.json)";
            }
        } else {
            source = "defaults";
        }

        // 3. CLI flags override config file values
        if let Some(s) = cli_src { cfg.src = s.to_string(); }
        if let Some(o) = cli_out { cfg.out = o.to_string(); }

        (cfg, source)
    }

    fn defaults() -> Self {
        Self {
            src: "src".to_string(),
            out: "dist".to_string(),
            target: "esnext".to_string(),
            minify: true,
            platform: "browser".to_string(),
            repository_name: None,
            thumbnail: None,
        }
    }

    fn merge_build_json(&mut self, v: &serde_json::Value) {
        if let Some(s) = v.get("src").and_then(|x| x.as_str()) { self.src = s.to_string(); }
        if let Some(s) = v.get("out").and_then(|x| x.as_str()) { self.out = s.to_string(); }
        if let Some(s) = v.get("target").and_then(|x| x.as_str()) { self.target = s.to_string(); }
        if let Some(b) = v.get("minify").and_then(|x| x.as_bool()) { self.minify = b; }
        if let Some(s) = v.get("platform").and_then(|x| x.as_str()) { self.platform = s.to_string(); }
    }

    fn merge_metadata_json(&mut self, v: &serde_json::Value) {
        if let Some(s) = v.get("repositoryName").and_then(|x| x.as_str()) {
            self.repository_name = Some(s.to_string());
        }
        if let Some(s) = v.get("thumbnail").and_then(|x| x.as_str()) {
            self.thumbnail = Some(s.to_string());
        }
    }
}

// ── Watcher binary location ───────────────────────────────────────────────────

/// Locate the mana-watcher binary. When installed via npm the watcher sits next
/// to the mana-dev executable. Falls back to `go run ./watcher` for local dev.
fn find_watcher_binary() -> WatcherInvocation {
    let watcher_name = if cfg!(windows) { "mana-watcher.exe" } else { "mana-watcher" };

    // Prefer a compiled binary next to the current executable (npm install layout).
    if let Ok(exe) = std::env::current_exe() {
        let candidate = exe.parent().unwrap_or(std::path::Path::new(".")).join(watcher_name);
        if candidate.exists() {
            return WatcherInvocation::Binary(candidate);
        }
    }

    // Local development fallback: `go run .` inside the watcher/ directory.
    WatcherInvocation::GoRun
}

enum WatcherInvocation {
    Binary(PathBuf),
    GoRun,
}

#[derive(Parser)]
#[command(author, version, about = "Mana Development Server", long_about = None)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Build sources using mana-watcher (esbuild) and generate JSON metadata
    Build {
        /// Source directory to build from (overrides mana.json / package.json)
        #[arg(value_name = "SOURCE")]
        source: Option<String>,
        
        /// Output directory for built files (overrides mana.json / package.json)
        #[arg(short, long)]
        output: Option<String>,
        
        /// Watch for changes and rebuild automatically
        #[arg(short, long)]
        watch: bool,
    },
    /// Start HTTP development server with build capability
    Serve {
        /// Source directory to build from (overrides mana.json / package.json)
        #[arg(value_name = "SOURCE")]
        source: Option<String>,
        
        /// Output directory for built files (overrides mana.json / package.json)
        #[arg(short, long)]
        output: Option<String>,
        
        /// Port to listen on
        #[arg(short, long, default_value = "8080")]
        port: u16,
        
        /// Watch for changes and rebuild automatically
        #[arg(short, long)]
        watch: bool,
    },
}

#[derive(Clone)]
struct AppState {
    logs: Arc<std::sync::Mutex<Vec<String>>>,
}

#[derive(Debug, serde::Deserialize)]
struct BuildResult {
    success: bool,
    errors: Option<Vec<String>>,
    warnings: Option<Vec<String>>,
    #[serde(default)]
    files: Vec<String>,
    timestamp: String,
    build_time_ms: i64,
}

struct Debouncer {
    last_trigger: Instant,
    delay: Duration,
    first_run: bool,
}

fn format_timestamp() -> String {
    use std::time::SystemTime;
    let now = SystemTime::now();
    let datetime = chrono::DateTime::<chrono::Local>::from(now);
    format!("[{}]", datetime.format("%H:%M:%S:%3f")).bright_black().to_string()
}

fn format_duration(duration: Duration) -> String {
    let millis = duration.as_millis();
    if millis < 1000 {
        format!("{}ms", millis).cyan().to_string()
    } else if millis < 5000 {
        format!("{:.1}s", millis as f64 / 1000.0).cyan().to_string()
    } else {
        format!("{:.1}s", millis as f64 / 1000.0).yellow().to_string()
    }
}

fn log_build_time(file_count: usize, build_duration: Duration) {
    let files_text = if file_count == 1 { "file" } else { "files" };
    println!("{} {} {} {} {} • {}", 
             format_timestamp(),
             "🔨".bright_blue(),
             "Built".yellow(),
             file_count.to_string().bright_blue().bold(),
             files_text.yellow(),
             format_duration(build_duration));
}

fn log_repository_time(duration: Duration) {
    println!("{} {} {} • {}", 
             format_timestamp(),
             "📚".bright_purple(),
             "Repository indexed".yellow(),
             format_duration(duration));
}

fn log_total_time(duration: Duration) {
    println!("{} {} {} • {}", 
             format_timestamp(),
             "⚡".bright_green(),
             "Total time".yellow(),
             format_duration(duration));
}

fn log_build_error(error: &str) {
    println!("{} {} {}: {}", 
             format_timestamp(),
             "❌".bright_red(),
             "Build error".yellow(),
             error.red());
}

fn log_repository_error(error: &str) {
    println!("{} {} {}: {}", 
             format_timestamp(),
             "💥".bright_red(),
             "Repository generation failed".yellow(),
             error.red());
}

fn log_stderr_warning(message: &str) {
    println!("{}", 
             message.yellow());
}

fn log_info(message: &str) {
    println!("{} {} {}", 
             format_timestamp(),
             "ℹ️".bright_cyan(),
             message.yellow());
}

impl Debouncer {
    fn new(delay: Duration) -> Self {
        Self {
            last_trigger: Instant::now(),
            delay,
            first_run: true,
        }
    }

    fn should_process(&mut self) -> bool {
        if self.first_run {
            self.first_run = false;
            self.last_trigger = Instant::now();
            return true;
        }
        
        let now = Instant::now();
        if now.duration_since(self.last_trigger) >= self.delay {
            self.last_trigger = now;
            true
        } else {
            false
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    
    match args.command {
        Commands::Build { source, output, watch } => {
            let (cfg, cfg_source) = ManaConfig::init(source.as_deref(), output.as_deref());
            log_info(&format!("Config from {}", cfg_source));
            if watch {
                watch_build_command(cfg.clone(), false).await
            } else {
                build_command(cfg.clone()).await
            }
        },
        Commands::Serve { source, output, port, watch } => {
            let (cfg, cfg_source) = ManaConfig::init(source.as_deref(), output.as_deref());
            log_info(&format!("Config from {}", cfg_source));
            serve_command(cfg.clone(), port, watch).await
        },
    }
}

async fn build_command(cfg: ManaConfig) -> Result<()> {
    println!("{} {} {}", 
             format_timestamp(),
             "🔨".bright_blue(),
             "Starting build process...".yellow());

    run_esbuild(cfg, false, false).await
}

async fn watch_build_command(cfg: ManaConfig, skip_initial_build: bool) -> Result<()> {
    println!("{} {} {}", 
             format_timestamp(),
             "👀".bright_magenta(),
             "Starting watch mode...".bright_magenta());

    println!("{} {} {}", 
             format_timestamp(),
             "👁️".bright_cyan(),
             "Watching for changes...".bright_cyan());

    run_esbuild(cfg, true, skip_initial_build).await
}

async fn run_esbuild(cfg: ManaConfig, watch: bool, skip_initial_build: bool) -> Result<()> {
    let source = cfg.src.clone();
    let output = cfg.out.clone();

    // Temp dir lives at the project root (parent of src), not inside src/.
    // Using an absolute path ensures both processes agree regardless of cwd.
    let src_path = std::path::Path::new(&source);
    let src_abs = if src_path.is_absolute() {
        src_path.to_path_buf()
    } else {
        std::env::current_dir()?.join(src_path)
    };
    let project_root = src_abs.parent().unwrap_or(&src_abs).to_path_buf();
    let temp_dir = project_root.join(".mana-temp");

    // Build the argument list for mana-watcher
    let mut watcher_args: Vec<String> = vec![
        "--source".into(), source.clone(),
        "--output".into(), output.clone(),
        "--temp".into(), temp_dir.to_string_lossy().to_string(),
        "--target".into(), cfg.target.clone(),
        "--platform".into(), cfg.platform.clone(),
    ];
    if !cfg.minify {
        watcher_args.extend(["--minify=false".into()]);
    }
    if watch {
        watcher_args.push("--watch".into());
    }
    if skip_initial_build {
        watcher_args.push("--no-initial-build".into());
    }

    let mut esbuild_process = match find_watcher_binary() {
        WatcherInvocation::Binary(bin) => {
            Command::new(&bin)
                .args(&watcher_args)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| anyhow!("Failed to start mana-watcher ({}): {}", bin.display(), e))?
        }
        WatcherInvocation::GoRun => {
            // Local dev fallback: prepend "run ." for `go run .`
            let mut go_args: Vec<String> = vec!["run".into(), ".".into()];
            go_args.extend(watcher_args);
            Command::new("go")
                .args(&go_args)
                .current_dir("watcher")
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| anyhow!("Failed to start Go watcher (go run): {}", e))?
        }
    };

    let stdout = esbuild_process.stdout.take()
        .ok_or_else(|| anyhow!("Failed to get stdout from ESBUILD process"))?;
    let stderr = esbuild_process.stderr.take()
        .ok_or_else(|| anyhow!("Failed to get stderr from ESBUILD process"))?;

    let mut stdout_reader = BufReader::new(stdout);
    let mut stderr_reader = BufReader::new(stderr);
    let mut stdout_line = String::new();
    let mut stderr_line = String::new();
    let mut stderr_buffer = String::new();
    let mut debouncer = Debouncer::new(Duration::from_millis(500));

    // When using the compiled binary the output path is relative to cwd, not
    // the watcher/ subdirectory, so we always resolve relative to cwd.
    let output_path = std::env::current_dir()?.join(&output);

    loop {
        tokio::select! {
            result = stdout_reader.read_line(&mut stdout_line) => {
                match result {
                    Ok(0) => {
                        if watch {
                            println!("✅ ESBuild watcher stopped");
                        }
                        break;
                    },
                    Ok(_) => {
                        let trimmed_line = stdout_line.trim();

                        if trimmed_line.starts_with("{") && trimmed_line.ends_with("}") {
                            match serde_json::from_str::<BuildResult>(trimmed_line) {
                                Ok(build_result) => {
                                    if build_result.success {
                                        let esbuild_duration = Duration::from_millis(build_result.build_time_ms as u64);
                                        log_build_time(build_result.files.len(), esbuild_duration);

                                        if debouncer.should_process() {
                                            let process_start = Instant::now();
                                            match process_build_result(&output_path, &build_result).await {
                                                Ok(()) => {
                                                    let repo_duration = process_start.elapsed();
                                                    log_repository_time(repo_duration);
                                                    let total_duration = esbuild_duration + repo_duration;
                                                    log_total_time(total_duration);
                                                },
                                                Err(e) => log_repository_error(&e.to_string()),
                                            }
                                        }
                                    } else {
                                        if let Some(errors) = &build_result.errors {
                                            for error in errors {
                                                log_build_error(error);
                                            }
                                        }
                                    }
                                },
                                Err(e) => println!("⚠️  Failed to parse build result JSON: {}", e),
                            }
                        }

                        stdout_line.clear();
                    },
                    Err(e) => {
                        println!("❌ Error reading from ESBUILD stdout: {}", e);
                        break;
                    }
                }
            }

            result = stderr_reader.read_line(&mut stderr_line) => {
                match result {
                    Ok(0) => {
                        if !stderr_buffer.trim().is_empty() {
                            log_stderr_warning(stderr_buffer.trim());
                            stderr_buffer.clear();
                        }
                    },
                    Ok(_) => {
                        let trimmed_line = stderr_line.trim();
                        if !trimmed_line.is_empty() {
                            if !(watch && trimmed_line.starts_with("Done in ")) {
                                stderr_buffer.push_str(trimmed_line);
                                stderr_buffer.push('\n');
                            }
                        } else if !stderr_buffer.trim().is_empty() {
                            log_stderr_warning(stderr_buffer.trim());
                            stderr_buffer.clear();
                        }
                        stderr_line.clear();
                    },
                    Err(e) => println!("❌ Error reading from ESBUILD stderr: {}", e),
                }
            }
        }
    }

    let _exit_status = esbuild_process.wait().await?;
    Ok(())
}

async fn process_build_result(output_path: &PathBuf, build_result: &BuildResult) -> Result<()> {
    // Ensure final output directory exists
    tokio::fs::create_dir_all(output_path).await?;

    // Must match the temp path derived in run_esbuild.
    let source = MANA_CONFIG.get().map(|(cfg, _)| cfg.src.as_str()).unwrap_or(".");
    let temp_dir = {
        let src_abs = {
            let p = std::path::Path::new(source);
            if p.is_absolute() { p.to_path_buf() } else { std::env::current_dir()?.join(p) }
        };
        let project_root = src_abs.parent().unwrap_or(&src_abs).to_path_buf();
        project_root.join(".mana-temp")
    };
    
    // Prepare temp files for delegate processing (no bulk copying here)
    let mut temp_files = Vec::new();
    for file_name in &build_result.files {
        let temp_file = temp_dir.join(file_name);
        if temp_file.exists() {
            temp_files.push((file_name.clone(), temp_file));
        }
    }
    
    // Create delegate that copies and renames each file based on V8 result
    let temp_dir_clone = temp_dir.clone();
    let delegate = move |file_name: &str, output_dir: &std::path::PathBuf, v8_result: &serde_json::Value| -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let temp_file = temp_dir_clone.join(file_name);
        
        if temp_file.exists() {
            // Extract target name from V8 result
            let target_name = if let Some(name) = v8_result.get("name").and_then(|n| n.as_str()) {
                format!("{}.mana", name)
            } else {
                // Fallback to original filename with .mana extension
                file_name.replace(".js", ".mana")
            };
            
            let final_file = output_dir.join(&target_name);
            std::fs::copy(&temp_file, &final_file)?;
        }
        Ok(())
    };
    
    // Use bulk emulator with delegate - files are copied individually during processing
    match emulator::bulk_build_emulator_native_standalone(temp_files, output_path.to_str().unwrap(), delegate).await {
        Ok(_metadata) => {
            // Metadata has been saved to metadata.json by the bulk emulator
        },
        Err(e) => {
            return Err(anyhow!("Failed to process files with bulk emulator: {}", e));
        }
    }
    
    Ok(())
}

async fn serve_command(cfg: ManaConfig, port: u16, watch: bool) -> Result<()> {
    println!("{} {} {}", 
             format_timestamp(),
             "🔨".bright_blue(),
             "Building before serving...".yellow());
    
    // Convert paths to absolute to avoid path mismatch issues
    let absolute_source = std::env::current_dir()?.join(&cfg.src).canonicalize()
        .map_err(|_| anyhow!("Source directory does not exist: {}", cfg.src))?;
    let absolute_output = std::env::current_dir()?.join(&cfg.out);

    let abs_cfg = ManaConfig {
        src: absolute_source.to_string_lossy().to_string(),
        out: absolute_output.to_string_lossy().to_string(),
        ..cfg.clone()
    };
    
    // First, build the project using absolute paths
    build_command(abs_cfg.clone()).await?;
    
    let serve_path = absolute_output.clone();
    
    if !serve_path.exists() {
        return Err(anyhow!("Output directory does not exist: {}", serve_path.display()));
    }
    
    println!("{} {} {}", 
             format_timestamp(),
             "🚀".bright_green(),
             "Starting Mana Development Server...".yellow());
    println!("{} {} Serving directory: {}", 
             format_timestamp(),
             "📁".bright_blue(),
             serve_path.display().to_string().bright_blue());

    
    if watch {
        println!("{} {} {}", 
                 format_timestamp(),
                 "👀".bright_magenta(),
                 "Watch mode enabled - will monitor for changes".yellow());
    }
    
    // Get local IP address
    let local_ip = local_ip_address::local_ip()
        .unwrap_or_else(|_| "127.0.0.1".parse().unwrap());
    
    let app_state = AppState {
        logs: Arc::new(std::sync::Mutex::new(Vec::new())),
    };
    
    // Create the router
    let app = Router::new()
        .route("/log", post(handle_log))
        .nest_service("/", ServeDir::new(serve_path.clone()))
        .layer(middleware::from_fn(log_requests))
        .layer(CorsLayer::permissive())
        .with_state(app_state);
    
    // Bind to all interfaces (0.0.0.0) to allow both localhost and network access
    let bind_addr = SocketAddr::from(([0, 0, 0, 0], port));
    
    println!("{} {} {}", 
             format_timestamp(),
             "🌐".bright_cyan(),
             "Server addresses:".bright_cyan());
    println!("   {} {}", 
             "Local:".bright_white().bold(), 
             format!("http://127.0.0.1:{}", port).bright_blue().underline());
    println!("   {} {}", 
             "Network:".bright_white().bold(), 
             format!("http://{}:{}", local_ip, port).bright_blue().underline());
    println!();
    println!("{} {} {}", 
             format_timestamp(),
             "📝".bright_yellow(),
             "Log endpoints:".bright_yellow());
    println!("   {} {}", 
             "Local:".bright_white().bold(), 
             format!("POST http://127.0.0.1:{}/log", port).bright_green().underline());
    println!("   {} {}", 
             "Network:".bright_white().bold(), 
             format!("POST http://{}:{}/log", local_ip, port).bright_green().underline());
    println!();
    println!("{} {} {} Press {} to stop", 
             format_timestamp(),
             "🎯".bright_green(),
             "Ready!".bright_green(),
             "Ctrl+C".bright_red().bold());
    
    // Start the server
    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;

    if watch {
        tokio::spawn(watch_build_command(abs_cfg, true));
    }

    axum::serve(listener, app).await?;
    
    Ok(())
}

async fn log_requests(req: Request<axum::body::Body>, next: Next) -> Response {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let start = Instant::now();
    let response = next.run(req).await;
    let status = response.status();
    let elapsed = start.elapsed();
    let status_colored = if status.is_success() {
        status.as_str().bright_green().to_string()
    } else if status.is_client_error() {
        status.as_str().yellow().to_string()
    } else if status.is_server_error() {
        status.as_str().bright_red().to_string()
    } else {
        status.as_str().white().to_string()
    };
    println!("{} {} {} {} • {}",
             format_timestamp(),
             method.to_string().bright_cyan().bold(),
             uri.to_string().white(),
             status_colored,
             format_duration(elapsed));
    response
}

async fn handle_log(
    State(state): State<AppState>,
    body: String,
) -> Result<StatusCode, StatusCode> {
    if let Ok(mut logs) = state.logs.lock() {
        logs.push(format!("[{}] {}", chrono::Utc::now().format("%Y-%m-%d %H:%M:%S"), body));
        if logs.len() > 1000 {
            let len = logs.len();
            logs.drain(0..len - 1000);
        }
    }
    println!("{} {} {}", 
             format_timestamp(),
             "📝".bright_yellow(),
             body.bright_cyan());
    Ok(StatusCode::OK)
}


