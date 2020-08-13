fn main() {
    #[cfg(feature = "linking")]
    // println!("cargo:rustc-link-lib=dylib=fbclient");
    // println!("cargo:rustc-link-lib=fbclient");
    println!("cargo:rustc-link-search=native=C:\\Program Files\\Firebird\\Firebird_3_0\\lib\\");    
}
