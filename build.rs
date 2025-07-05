fn main() {
    jpegxl_src::build();

    let out_path = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let include_path = out_path.join("include");

    let bindings = bindgen::builder()
        .header("wrapper.h")
        .clang_arg(format!("-I{}", include_path.display()))
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .generate()
        .expect("failed to generate bindings");

    bindings.write_to_file(out_path.join("bindings.rs")).expect("failed to write bindings");

    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=wrapper.h");
}
