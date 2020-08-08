fn main() {
    #[cfg(not(feature = "dynamic_loading"))]
    println!("cargo:rustc-link-lib=dylib=fbclient");
}
