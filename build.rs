fn main() {
    let libjxl_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("libjxl");
    // SAFETY: No other threads can access the variable concurrently.
    unsafe {
        std::env::set_var("DEP_JXL_PATH", libjxl_path);
    }

    jpegxl_src::build();
    println!("cargo::rerun-if-changed=.git/modules/libjxl/HEAD");

    let out_path = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let include_path = out_path.join("include");

    let bindings = bindgen::builder()
        .header("wrapper.h")
        .clang_arg(format!("-I{}", include_path.display()))
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .generate()
        .expect("failed to generate bindings");

    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("failed to write bindings");

    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=wrapper.h");
}
