// build.rs

fn main() {
    if search_on_environment_var() {
        #[cfg(all(feature = "linking", target_os = "linux"))]
        search_on_linux();

        #[cfg(all(feature = "linking", target_os = "windows"))]
        search_on_windows();
    }
    println!("cargo:rerun-if-changed=build.rs");
}

fn search_on_environment_var() -> bool {
    // https://doc.rust-lang.org/cargo/reference/build-scripts.html#cargorustc-link-searchkindpath

    if let Ok(user_specified_dir) = std::env::var("FBCLIENT_LIB_DIR") {
        println!("cargo:rustc-link-search={}", user_specified_dir);
        return false;
    }
    return true;
}

#[cfg(all(feature = "linking", target_os = "linux"))]
fn search_on_linux() {
    // https://doc.rust-lang.org/rustc/command-line-arguments.html#option-l-link-lib

    println!("cargo:rustc-link-lib=dylib=fbclient");
}

#[cfg(all(feature = "linking", target_os = "windows"))]
fn search_on_windows() {
    let fbclient_lib_names: [&str; 2] = ["fbclient.lib", "fbclient_ms.lib"];
    for fbclient_lib in &fbclient_lib_names {
        if let Some(found) = search_for_file(fbclient_lib) {
            println!("cargo:rustc-link-lib=dylib={}", found);
            return;
        }
    }
}

#[cfg(all(feature = "linking", target_os = "windows"))]
use glob::glob;

#[cfg(all(feature = "linking", target_os = "windows"))]
fn search_for_file(filename: &str) -> Option<String> {
    // https://kornel.ski/rust-sys-crate#find

    let firebird_install_dirs: [&str; 5] = [
        "C:\\Program Files\\Firebird\\Firebird_3_0",
        "C:\\Program Files\\Firebird\\Firebird*",
        "C:\\Firebird*",
        "D:\\Firebird*",
        "C:\\Windows\\System*",
    ];

    for install_dir in &firebird_install_dirs {
        let pattern = format!("{}\\**\\{}", install_dir, filename);
        let found = glob(&pattern).expect("Failed to read glob pattern");

        for entry in found {
            if let Ok(path) = entry {
                if path.is_file() {
                    let fp = path.to_str().unwrap();
                    return Some(fp.to_string());
                }
            }
        }
    }
    return None;
}

// end of code
