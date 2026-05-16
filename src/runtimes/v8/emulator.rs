static EMULATOR_CODE: &str = include_str!("../../../runtime/emulator.js");
static TARGET_PROCESSOR_CODE: &str = include_str!("../../../runtime/target_processor.js");

pub fn get_emulator_code() -> Option<&'static str> {
    Some(EMULATOR_CODE)
}

pub fn get_target_processor_code() -> Option<&'static str> {
    Some(TARGET_PROCESSOR_CODE)
}

/// Load emulator code and target processor into V8 scope
pub fn load_emulator_in_scope(scope: &mut v8::ContextScope<v8::HandleScope>) -> anyhow::Result<()> {
    // Load the base emulator first (required)
    if let Some(emulator_code) = get_emulator_code() {
        let code = v8::String::new(scope, emulator_code)
            .ok_or_else(|| anyhow::anyhow!("Failed to create V8 string from emulator code"))?;
        
        // Create a global object before loading the emulator since it expects one
        let create_global_pre_code = v8::String::new(scope,
            "if (typeof global === 'undefined') {
                 global = globalThis;
             }"
        ).unwrap();
        let pre_script = v8::Script::compile(scope, create_global_pre_code, None).unwrap();
        pre_script.run(scope);
        
        // Load as ES6 module (emulator.js is always a module)
        let script_name = v8::String::new(scope, "emulator.mjs")
            .ok_or_else(|| anyhow::anyhow!("Failed to create script name"))?;
        let origin = v8::ScriptOrigin::new(
            scope,
            script_name.into(),
            0, 0, false, 0, None, false, false,
            true, // is_module = true
            None,
        );
        let mut source = v8::script_compiler::Source::new(code, Some(&origin));
        let module = v8::script_compiler::compile_module(scope, &mut source)
            .ok_or_else(|| anyhow::anyhow!("Failed to compile emulator module"))?;
        
        // Instantiate and evaluate the module
        let instantiation_success = module.instantiate_module(scope, |_context, _specifier, _import_assertions, _referrer| {
            None // No imports expected
        });
        
        if instantiation_success != Some(true) {
            return Err(anyhow::anyhow!("Failed to instantiate emulator module"));
        }
        
        let evaluation_result = module.evaluate(scope);
        if evaluation_result.is_none() {
            return Err(anyhow::anyhow!("Failed to evaluate emulator module"));
        }
        
        // Module is loaded and will self-export its functions globally
        // The emulator.js handles its own global exports at the end of the file
    } else {
        return Err(anyhow::anyhow!("Emulator code not available - emulator.js is required"));
    }
    
    // Load the target processor if available (optional)
    if let Some(target_processor_code) = get_target_processor_code() {
        let code = v8::String::new(scope, target_processor_code)
            .ok_or_else(|| anyhow::anyhow!("Failed to create V8 string from target processor code"))?;
        let script = v8::Script::compile(scope, code, None)
            .ok_or_else(|| anyhow::anyhow!("Failed to compile target processor script"))?;
        script.run(scope)
            .ok_or_else(|| anyhow::anyhow!("Failed to execute target processor script"))?;
    }
    
    Ok(())
}


pub async fn bulk_build_emulator_native_standalone<F>(
    temp_files: Vec<(String, std::path::PathBuf)>,
    output_dir: &str,
    delegate: F,
) -> anyhow::Result<serde_json::Value>
where
    F: Fn(&str, &std::path::PathBuf, &serde_json::Value) -> Result<(), Box<dyn std::error::Error + Send + Sync>> + Send + Sync + Clone + 'static,
{
    use std::path::PathBuf;
    use futures::future::join_all;

    if temp_files.is_empty() {
        return Ok(serde_json::json!({}));
    }

    let output_path = PathBuf::from(output_dir);
    let sources_path = output_path.join("sources");
    std::fs::create_dir_all(&sources_path)?;

    let (repository_name, thumbnail) = crate::MANA_CONFIG
        .get()
        .map(|(cfg, _)| (cfg.repository_name.as_deref(), cfg.thumbnail.as_deref()))
        .unwrap_or((None, None));

    // Configure V8 thread pool exactly where it's executed
    let cpu_cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    
    // Use all available CPU cores, task count is exactly the number of files
    super::configure_thread_pool(cpu_cores, temp_files.len());
    super::configure_debug_mode(false);

    // Process all files in parallel using the original thread pool logic
    let tasks: Vec<_> = temp_files.iter().map(|(file_name, temp_path)| {
        let file_name = file_name.clone();
        let temp_path = temp_path.clone();
        let sources_path = sources_path.clone();
        let delegate = delegate.clone();
        
        async move {
            // Read the JavaScript file from temp location first
            match tokio::fs::read_to_string(&temp_path).await {
                Ok(js_content) => {
                    // Use the original execute_and_get_target_info function with thread pool
                    match super::execute_and_get_target_info(&js_content).await {
                        Ok(result) => {
                            // Call delegate with the V8 processing result
                            if let Err(e) = delegate(&file_name, &sources_path, &result) {
                                eprintln!("⚠️ Delegate failed for {}: {}", file_name, e);
                                return None;
                            }
                            Some((file_name, result))
                        },
                        Err(e) => {
                            eprintln!("⚠️ Failed to process {}: {}", file_name, e);
                            None
                        }
                    }
                },
                Err(e) => {
                    eprintln!("⚠️ Failed to read {}: {}", file_name, e);
                    None
                }
            }
        }
    }).collect();

    // Execute all tasks in parallel
    let results = join_all(tasks).await;
    
    // Collect successful results as an array for the sources property
    let mut all_metadata = Vec::new();
    for result in results {
        if let Some((_file_name, metadata)) = result {
            all_metadata.push(metadata);
        }
    }

    let mut final_metadata = serde_json::json!({});
    if let Some(name) = repository_name {
        final_metadata["repositoryName"] = serde_json::Value::String(name.to_string());
    }
    if let Some(thumb) = thumbnail {
        final_metadata["thumbnail"] = serde_json::Value::String(thumb.to_string());
    }
    final_metadata["sources"] = serde_json::Value::Array(all_metadata);
    
    // Save metadata to JSON file (preserving order)
    let metadata_path = output_path.join("sources.json");
    let metadata_json = serde_json::to_string_pretty(&final_metadata)?;
    tokio::fs::write(&metadata_path, metadata_json).await?;

    Ok(final_metadata)
}
