#![cfg(all(feature = "rustc_is_nightly"))]
#![feature(custom_test_frameworks)]
#![test_runner(datatest::runner)]

// Make sure we can run tests

mod inner {
    mod another {
        use serde::Deserialize;

        #[derive(Deserialize)]
        struct GreeterTestCase {
            name: String,
            expected: String,
        }

        #[datatest::data("tests/tests.yaml")]
        #[test]
        fn data_test_line_only(data: &GreeterTestCase) {
            assert_eq!(data.expected, format!("Hi, {}!", data.name));
        }
    }
}
