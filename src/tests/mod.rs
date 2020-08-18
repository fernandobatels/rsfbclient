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
            $( $connect:tt )*
        }

        $( $tail:tt )*
    ) => {
        $( #[$attr] )*
        mod $name {
            $( $tests )*

            fn connect() -> $type {
                $( $connect )*
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
            for linking -> Connection<rsfbclient_native::NativeFbClient> {
                crate::ConnectionBuilder::linked()
                        .connect()
                        .expect("Error on connect the test database")
            }

            #[cfg(all(feature = "linking", feature = "embedded_tests"))]
            for linking_embedded -> Connection<rsfbclient_native::NativeFbClient> {
                crate::ConnectionBuilder::linked()
                        .embedded()
                        .db_name("/tmp/embedded_tests.fdb")
                        .connect()
                        .expect("Error on connect the test database")
            }

            #[cfg(all(feature = "dynamic_loading", not(feature = "embedded_tests")))]
            for dynamic_loading -> Connection<rsfbclient_native::NativeFbClient> {

                #[cfg(target_os = "linux")]
                let libfbclient = "libfbclient.so";
                #[cfg(target_os = "windows")]
                let libfbclient = "fbclient.dll";

                crate::ConnectionBuilder::with_client(libfbclient)
                        .connect()
                        .expect("Error on connect the test database")
            }

            #[cfg(all(feature = "dynamic_loading", feature = "embedded_tests"))]
            for dynamic_loading_embedded -> Connection<rsfbclient_native::NativeFbClient> {

                #[cfg(target_os = "linux")]
                let libfbclient = "libfbclient.so";
                #[cfg(target_os = "windows")]
                let libfbclient = "fbclient.dll";

                crate::ConnectionBuilder::with_client(libfbclient)
                        .embedded()
                        .db_name("/tmp/embedded_tests.fdb")
                        .connect()
                        .expect("Error on connect the test database")
            }

            #[cfg(feature = "pure_rust")]
            for pure_rust -> Connection<rsfbclient_rust::RustFbClient> {
                crate::ConnectionBuilder::pure_rust()
                        .connect()
                        .expect("Error on connect the test database")
            }
        }
    };
}

mod params;
mod row;
