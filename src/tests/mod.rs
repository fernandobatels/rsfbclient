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

mod params;
mod row;
