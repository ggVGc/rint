#[cfg(feature = "rint")]
fn main() {
    rint_build::generate_with_native_modules("src/main.rs", &["terminal_ui"])
        .expect("discover script bindings")
        .write_to_out_dir("rint_bindings.rs")
        .expect("write bindings");
}

#[cfg(not(feature = "rint"))]
fn main() {}
