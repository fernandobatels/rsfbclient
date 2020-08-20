// build.rs

fn main() {
    if search_on_environment_var() {
        #[cfg(all(feature = "linking", target_os = "linux"))]
        search_on_linux();

        #[cfg(all(feature = "linking", target_os = "macos"))]
        search_on_macos();

        #[cfg(all(feature = "linking", target_os = "windows"))]
        search_on_windows();
    }
    println!("cargo:rerun-if-env-changed=PROFILE");
    println!("cargo:rerun-if-env-changed=TARGET");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/tests/mod.rs");
}

fn search_on_environment_var() -> bool {
    // https://doc.rust-lang.org/cargo/reference/build-scripts.html#cargorustc-link-searchkindpath

    if let Ok(user_specified_dir) = std::env::var("FBCLIENT_LIB_DIR") {
        println!("cargo:rustc-link-search={}", user_specified_dir);
        return false;
    }
    true
}

#[cfg(all(feature = "linking", target_os = "linux"))]
fn search_on_linux() {
    // https://doc.rust-lang.org/rustc/command-line-arguments.html#option-l-link-lib

    println!("cargo:rustc-link-lib=dylib=fbclient");
}

#[cfg(all(feature = "linking", target_os = "macos"))]
fn search_on_macos() {
    let def_fbclient_sys = "/usr/local/lib/libfbclient.dylib";
    let fb3_lib_path_sys = std::path::Path::new(def_fbclient_sys);
    if fb3_lib_path_sys.exists() {
        println!("cargo:rustc-link-search=/usr/local/lib/");
    }
    let def_fbclient_lib =
        "/Library/Frameworks/Firebird.framework/Versions/A/Libraries/libfbclient.dylib";
    let fb3_lib_path_lib = std::path::Path::new(def_fbclient_lib);
    if fb3_lib_path_lib.exists() {
        println!(
            "cargo:rustc-link-search=/Library/Frameworks/Firebird.framework/Versions/A/Libraries/"
        );
    }
    println!("cargo:rustc-link-lib=dylib=libfbclient");
    // println!("cargo:rustc-link-lib=dylib=libfbclient.dylib");
    // println!("cargo:rustc-link-lib=framework=Firebird.framework");
}

#[cfg(all(feature = "linking", target_os = "windows"))]
use glob::glob;

#[cfg(all(feature = "linking", target_os = "windows"))]
fn search_on_windows() {
    let def_fbclient_lib = "C:\\Program Files\\Firebird\\Firebird_3_0\\lib\\fbclient_ms.lib";
    let fb3_lib_path = std::path::Path::new(def_fbclient_lib);
    if fb3_lib_path.exists() {
        println!("cargo:rustc-link-search=C:\\Program Files\\Firebird\\Firebird_3_0\\lib");
        println!("cargo:rustc-link-lib=dylib=fbclient_ms");
    } else if search_on_windows_for_lib("fbclient", "fbclient.lib") {
        if search_on_windows_for_lib("fbclient_ms", "fbclient_ms.lib") {
            println!("warning:fbclient.lib not found!");
        }
    }
}

#[cfg(all(feature = "linking", target_os = "windows"))]
fn search_on_windows_for_lib(libname: &str, filename: &str) -> bool {
    if let Some(fbclient_lib) = search_for_file(filename) {
        let dir = fbclient_lib.parent().unwrap().to_str().unwrap();
        println!("cargo:rustc-link-search={}", dir);
        println!("cargo:rustc-link-lib=dylib={}", libname);
        return true;
    }
    false
}

#[cfg(all(feature = "linking", target_os = "windows"))]
fn search_for_file(filename: &str) -> Option<std::path::PathBuf> {
    // https://kornel.ski/rust-sys-crate#find

    let firebird_install_dirs: [&str; 5] = [
        "C:\\Program Files\\Firebird\\Firebird_3_0\\lib",
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
                    return Some(path);
                }
            }
        }
    }
    None
}

// end of code
