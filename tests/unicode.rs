#![feature(non_ascii_idents)]
#![feature(custom_test_frameworks)]
#![test_runner(datatest::runner)]

use serde::Deserialize;
use std::fmt;
use std::path::Path;

#[datatest::files("tests/test-cases", {
  // Pattern is defined via `in` operator. Every file from the `directory` above will be matched
  // against this regular expression and every matched file will produce a separate test.
  input in r"^(.*)\.input\.txt",
  // Template defines a rule for deriving dependent file name based on captures of the pattern.
  output = r"${1}.output.txt",
})]
#[test]
fn files_testsome_unicode_привет(input: &str, output: &str) {
    assert_eq!(format!("Hello, {}!", input), output);
}

/// This test case item implements [`std::fmt::Display`], which is used to generate test name
#[derive(Deserialize)]
struct GreeterTestCaseNamed {
    name: String,
    expected: String,
}

impl fmt::Display for GreeterTestCaseNamed {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.name)
    }
}

#[datatest::data("tests/tests.yaml")]
#[test]
fn data_test_with_some_unicode_привет(data: &GreeterTestCaseNamed) {
    assert_eq!(data.expected, format!("Hi, {}!", data.name));
}
