fn main() {
    #[cfg(target_os = "windows")]
    {
        // Link Windows libraries required by V8
        println!("cargo:rustc-link-lib=advapi32");
        println!("cargo:rustc-link-lib=tdh");  // Event Tracing for Windows
        println!("cargo:rustc-link-lib=winmm");
        println!("cargo:rustc-link-lib=dbghelp");
    }
}
