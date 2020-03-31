#[cfg(feature = "stable")]
use datatest::test;

use serde::Deserialize;
use std::fmt;
use std::path::Path;

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
    input in r"^(.*)\.input\.txt" if !uses_symbolic_files,
    // Template defines a rule for deriving dependent file name based on captures of the pattern.
    output = r"${1}.output.txt",
})]
#[test]
fn files_test_strings(input: &str, output: &str) {
    assert_eq!(format!("Hello, {}!", input), output);
}

/// Same as above, but uses symbolic files, which is only tested on unix platforms
#[datatest::files("tests/test-cases", {
    input in r"^(.*)\.input\.txt",
    output = r"${1}.output.txt",
})]
#[test]
#[cfg(unix)]
fn symbolic_files_test_strings(input: &str, output: &str) {
    assert_eq!(format!("Hello, {}!", input), output);
}

/// Same as above, but always panics, so marked by `#[ignore]`
#[ignore]
#[datatest::files("tests/test-cases", {
    input in r"^(.*)\.input\.txt",
    output = r"${1}.output.txt",
})]
#[test]
fn files_tests_not_working_yet_and_never_will(input: &str, output: &str) {
    assert_eq!(input, output, "these two will never match!");
}

/// Can declare with `&std::path::Path` to get path instead of the content
#[datatest::files("tests/test-cases", {
    input in r"^(.*)\.input\.txt",
    output = r"${1}.output.txt",
})]
#[test]
fn files_test_paths(input: &Path, output: &Path) {
    let input = input.display().to_string();
    let output = output.display().to_string();
    // Check output path is indeed input path with `input` => `output`
    assert_eq!(input.replace("input", "output"), output);
}

/// Can also take slices
#[datatest::files("tests/test-cases", {
    input in r"^(.*)\.input\.txt",
    output = r"${1}.output.txt",
})]
#[test]
fn files_test_slices(input: &[u8], output: &[u8]) {
    let mut actual = b"Hello, ".to_vec();
    actual.extend(input);
    actual.push(b'!');
    assert_eq!(actual, output);
}

fn is_ignore(path: &Path) -> bool {
    path.display().to_string().ends_with("case-02.input.txt")
}

fn uses_symbolic_files(path: &Path) -> bool {
    path.display().to_string().ends_with("case-03.input.txt")
}

/// Ignore first test case!
#[datatest::files("tests/test-cases", {
    input in r"^(.*)\.input\.txt" if !is_ignore,
    output = r"${1}.output.txt",
})]
#[test]
fn files_test_ignore(input: &str) {
    assert_eq!(input, "Kylie");
}

/// Regular tests are also allowed!
#[test]
fn simple_test() {
    let palindrome = "never odd or even".replace(' ', "");
    let reversed = palindrome.chars().rev().collect::<String>();

    assert_eq!(palindrome, reversed)
}

/// Regular tests are also allowed! Also, could be ignored the same!
#[test]
#[ignore]
fn simple_test_ignored() {
    panic!("ignored test!")
}

/// Regular tests are also allowed! Also, could use `#[should_panic]`
#[test]
#[should_panic]
fn simple_test_panics() {
    panic!("panicking test!")
}

/// Regular tests are also allowed! Also, could use `#[should_panic]`
#[test]
#[should_panic(expected = "panicking test!")]
fn simple_test_panics_message() {
    panic!("panicking test!")
}

/// This test case item does not implement [`std::fmt::Display`], so only line number is shown in
/// the test name.
#[derive(Deserialize)]
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
#[test]
fn data_test_line_only(data: &GreeterTestCase) {
    assert_eq!(data.expected, format!("Hi, {}!", data.name));
}

/// Can take as value, too
#[datatest::data("tests/tests.yaml")]
#[test]
fn data_test_take_owned(mut data: GreeterTestCase) {
    data.expected += "boo!";
    data.name += "!boo";
    assert_eq!(data.expected, format!("Hi, {}!", data.name));
}

#[ignore]
#[datatest::data("tests/tests.yaml")]
#[test]
fn data_test_line_only_hoplessly_broken(_data: &GreeterTestCase) {
    panic!("this test always fails, but this is okay because we marked it as ignored!")
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
fn data_test_name_and_line(data: &GreeterTestCaseNamed) {
    assert_eq!(data.expected, format!("Hi, {}!", data.name));
}

/// Can also take string inputs
#[datatest::data("tests/strings.yaml")]
#[test]
fn data_test_string(data: String) {
    let half = data.len() / 2;
    assert_eq!(data[0..half], data[half..]);
}

/// Can also use `::datatest::yaml` explicitly
#[datatest::data(::datatest::yaml("tests/strings.yaml"))]
#[test]
fn data_test_yaml(data: String) {
    let half = data.len() / 2;
    assert_eq!(data[0..half], data[half..]);
}

// Experimental API: allow custom test cases

struct StringTestCase {
    input: String,
    output: String,
}

fn load_test_cases(path: &str) -> Vec<::datatest::DataTestCaseDesc<StringTestCase>> {
    let input = std::fs::read_to_string(path).unwrap();
    let lines = input.lines().collect::<Vec<_>>();
    lines
        .chunks(2)
        .enumerate()
        .map(|(idx, line)| ::datatest::DataTestCaseDesc {
            case: StringTestCase {
                input: line[0].to_string(),
                output: line[1].to_string(),
            },
            name: Some(line[0].to_string()),
            location: format!("line {}", idx * 2),
        })
        .collect()
}

/// Can have custom deserialization for data tests
#[datatest::data(load_test_cases("tests/cases.txt"))]
#[test]
fn data_test_custom(data: StringTestCase) {
    assert_eq!(data.output, format!("Hello, {}!", data.input));
}
