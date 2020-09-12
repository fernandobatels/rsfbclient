//! Crate tests and test utils

/// Generate copies of tests for multiple client implementations
macro_rules! mk_tests {
    // Base case
    (
        tests {
            $( $tests:tt )*
        }
    ) => {};

    // Recurse for each module
    (
        tests {
            $( $tests:tt )*
        }

        $( #[$attr:meta] )*
        for $name:ident -> $type:ty {
            $( $cbuilder:tt )*
        }

        $( $tail:tt )*
    ) => {
        $( #[$attr] )*
        mod $name {
            $( $tests )*

            fn cbuilder() -> $type {
                $( $cbuilder )*
            }
        }

        mk_tests! {
            tests {
                $( $tests )*
            }
            $( $tail )*
        }
    };
}

/// Generate copies of tests for the default client implementations
macro_rules! mk_tests_default {
    ( $( $tests:tt )* ) => {
        mk_tests! {
            tests {
                $( $tests )*
            }

            #[cfg(all(feature = "linking", not(feature = "embedded_tests")))]
            for linking -> crate::ConnectionBuilder<rsfbclient_native::NativeFbClient> {
                crate::ConnectionBuilder::linked()
            }

            #[cfg(all(feature = "linking", feature = "embedded_tests"))]
            for linking_embedded -> crate::connection::ConnectionBuilderEmbedded<rsfbclient_native::NativeFbClient> {
                crate::ConnectionBuilder::linked()
                    .embedded()
                    .db_name("/tmp/embedded_tests.fdb")
                    .clone()
            }

            #[cfg(all(feature = "dynamic_loading", not(feature = "embedded_tests")))]
            for dynamic_loading -> crate::ConnectionBuilder<rsfbclient_native::NativeFbClient> {

                #[cfg(target_os = "linux")]
                let libfbclient = "libfbclient.so";
                #[cfg(target_os = "windows")]
                let libfbclient = "fbclient.dll";
                #[cfg(target_os = "macos")]
                let libfbclient = "libfbclient.dylib";

                crate::ConnectionBuilder::with_client(libfbclient)
            }

            #[cfg(all(feature = "dynamic_loading", feature = "embedded_tests"))]
            for dynamic_loading_embedded -> crate::connection::ConnectionBuilderEmbedded<rsfbclient_native::NativeFbClient> {

                #[cfg(target_os = "linux")]
                let libfbclient = "libfbclient.so";
                #[cfg(target_os = "windows")]
                let libfbclient = "fbclient.dll";
                #[cfg(target_os = "macos")]
                let libfbclient = "libfbclient.dylib";

                crate::ConnectionBuilder::with_client(libfbclient)
                    .embedded()
                    .db_name("/tmp/embedded_tests.fdb")
                    .clone()
            }

            #[cfg(feature = "pure_rust")]
            for pure_rust -> crate::ConnectionBuilder<rsfbclient_rust::RustFbClient> {
                crate::ConnectionBuilder::pure_rust()
            }
        }
    };
}

mod charset;
mod params;
mod row;
