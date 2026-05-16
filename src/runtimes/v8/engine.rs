use anyhow::{Result, anyhow};
use serde_json::Value;

/// V8 engine wrapper managing isolate and context
pub struct V8Engine {
    isolate: v8::OwnedIsolate,
}

impl V8Engine {
    /// Create a new V8 engine with optimized parameters
    pub fn new(_thread_id: usize) -> Self {
        let create_params = v8::CreateParams::default()
            .heap_limits(64 * 1024 * 1024, 512 * 1024 * 1024) // 64MB initial, 512MB max heap per isolate
            .allow_atomics_wait(false); // Disable atomic wait for better multi-threading
        let mut isolate = v8::Isolate::new(create_params);
        isolate.set_microtasks_policy(v8::MicrotasksPolicy::Explicit); // Explicit microtasks policy
        
        Self { isolate }
    }

    /// Execute JavaScript code and return the result
    pub fn execute_script(&mut self, js_code: &str) -> Result<Value> {
        let handle_scope = &mut v8::HandleScope::new(&mut self.isolate);
        
        // Create a fresh global context for each execution
        let global_template = v8::ObjectTemplate::new(handle_scope);
        global_template.set_internal_field_count(0);
        
        let mut context_options = v8::ContextOptions::default();
        context_options.global_template = Some(global_template);
        let fresh_context = v8::Context::new(handle_scope, context_options);
        let scope = &mut v8::ContextScope::new(handle_scope, fresh_context);

        // Execute as regular script
        let final_js_code = js_code.to_string();
        
        let code = v8::String::new(scope, &final_js_code)
            .ok_or_else(|| anyhow!("Failed to create V8 string from JS code"))?;
        let script = v8::Script::compile(scope, code, None)
            .ok_or_else(|| anyhow!("Failed to compile script"))?;
        script.run(scope)
            .ok_or_else(|| anyhow!("Failed to execute script"))?;

        // Load emulator and target processor for this script execution context
        match super::emulator::load_emulator_in_scope(scope) {
            Ok(_) => {
                // Verify core functions are available
                let test_code = v8::String::new(scope, "typeof emulate !== 'undefined' && typeof processTarget !== 'undefined'").unwrap();
                let test_script = v8::Script::compile(scope, test_code, None).unwrap();
                let test_result = test_script.run(scope).unwrap();
                if !test_result.is_true() {
                    return Err(anyhow!("Emulator functions not available after loading"));
                }
            },
            Err(e) => {
                return Err(e);
            }
        }

        // Extract target info - use processTarget if available, otherwise fallback to emulate
        let result_code = 
            // In script context, try multiple approaches for self-contained bundles
            v8::String::new(scope, 
                "
                // Try to find Target in different scopes
                var targetClass = globalThis.Target || (typeof Target !== 'undefined' ? Target : null);
                
                if (!targetClass) {
                    throw new Error('No Target class found in self-contained bundle');
                }
                
                (function() {
                    let result = (typeof processTarget !== 'undefined' ? processTarget : emulate)(targetClass);
                    return JSON.stringify(result.info || result);
                })()
                "
            )
        .ok_or_else(|| anyhow!("Failed to create result extraction code"))?;
        
        let result_script = v8::Script::compile(scope, result_code, None)
            .ok_or_else(|| anyhow!("Failed to compile result extraction script"))?;
        let result = result_script.run(scope)
            .ok_or_else(|| anyhow!("Failed to execute result extraction"))?;

        // Convert V8 value to JSON
        let json_str = result.to_rust_string_lossy(scope);
        let parsed_result = serde_json::from_str(&json_str)
            .map_err(|e| anyhow!("Failed to parse evaluation result as JSON: {}", e))?;

        Ok(parsed_result)
    }
}
