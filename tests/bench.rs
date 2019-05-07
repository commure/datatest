#![feature(custom_test_frameworks)]
#![test_runner(datatest::runner)]
#![feature(test)]
extern crate test;

use serde::Deserialize;
use std::path::Path;
use test::Bencher;

/// File-driven tests are defined via `#[files(...)]` attribute.
///
/// The first argument to the attribute is the path to the test data (relative to the crate root
/// directory).
///
/// The second argument is a block of mappings, each mapping defines the rules of deriving  test
/// function arguments.
///
/// Exactly one mapping should be a "pattern" mapping, defined as `<arg> in "<regex>"`. `<regex>`
/// is a regular expression applied to every file found in the test directory. For each file path
/// matching the regular expression, test runner will create a new test instance.
///
/// Other mappings are "template" mappings, they define the template to use for deriving the file
/// paths. Each template have a syntax of a [replacement string] from [`regex`] crate.
///
/// [replacement string]: https://docs.rs/regex/*/regex/struct.Regex.html#method.replace
/// [regex]: https://docs.rs/regex/*/regex/
#[datatest::files("tests/test-cases", {
  // Pattern is defined via `in` operator. Every file from the `directory` above will be matched
  // against this regular expression and every matched file will produce a separate test.
  input in r"^(.*)\.input\.txt",
  // Template defines a rule for deriving dependent file name based on captures of the pattern.
  output = r"${1}.output.txt",
})]
#[bench]
fn files_test_strings(bencher: &mut Bencher, input: &str, output: &str) {
    bencher.iter(|| {
        assert_eq!(format!("Hello, {}!", input), output);
    });
}

/// Regular tests are also allowed!
#[bench]
fn simple_test(bencher: &mut Bencher) {
    bencher.iter(|| {
        let palindrome = "never odd or even".replace(' ', "");
        let reversed = palindrome.chars().rev().collect::<String>();

        assert_eq!(palindrome, reversed)
    })
}

/// This test case item does not implement [`std::fmt::Display`], so only line number is shown in
/// the test name.
#[derive(Deserialize, Clone)]
struct GreeterTestCase {
    name: String,
    expected: String,
}

/// Data-driven tests are defined via `#[datatest::data(..)]` attribute.
///
/// This attribute specifies a test file with test cases. Currently, the test file have to be in
/// YAML format. This file is deserialized into `Vec<T>`, where `T` is the type of the test function
/// argument (which must implement `serde::Deserialize`). Then, for each element of the vector, a
/// separate test instance is created and executed.
///
/// Name of each test is derived from the test function module path, test case line number and,
/// optionall, from the [`ToString`] implementation of the test case data (if either [`ToString`]
/// or [`std::fmt::Display`] is implemented).
#[datatest::data("tests/tests.yaml")]
#[bench]
fn data_test_line_only(bencher: &mut Bencher, data: &GreeterTestCase) {
    bencher.iter(|| {
        assert_eq!(data.expected, format!("Hi, {}!", data.name));
    })
}
