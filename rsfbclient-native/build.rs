fn main() {
    #[cfg(feature = "linking")]
    println!("cargo:rustc-link-lib=dylib=fbclient");
}
