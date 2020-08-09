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

            #[cfg(feature = "linking")]
            for linking -> Connection<rsfbclient_native::NativeFbClient> {
                crate::ConnectionBuilder::linked()
                        .connect()
                        .expect("Error on connect the test database")
            }

            #[cfg(feature = "linking")]
            for linking_embedded -> Connection<rsfbclient_native::NativeFbClient> {
                crate::ConnectionBuilder::linked()
                        .embedded()
                        .db_name("/tmp/embedded_tests.fdb")
                        .connect()
                        .expect("Error on connect the test database")
            }

            #[cfg(feature = "dynamic_loading")]
            for dynamic_loading -> Connection<rsfbclient_native::NativeFbClient> {
                crate::ConnectionBuilder::with_client("libfbclient.so")
                        .connect()
                        .expect("Error on connect the test database")
            }

            #[cfg(feature = "dynamic_loading")]
            for dynamic_loading_embedded -> Connection<rsfbclient_native::NativeFbClient> {
                crate::ConnectionBuilder::with_client("libfbclient.so")
                        .embedded()
                        .db_name("/tmp/embedded_tests.fdb")
                        .connect()
                        .expect("Error on connect the test database")
            }
        }
    };
}

mod params;
mod row;
