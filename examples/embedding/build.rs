#[cfg(feature = "rint")]
fn main() {
    rint_build::generate("src/main.rs")
        .expect("discover script bindings")
        .write_to_out_dir("rint_bindings.rs")
        .expect("write bindings");
}

#[cfg(not(feature = "rint"))]
fn main() {}
