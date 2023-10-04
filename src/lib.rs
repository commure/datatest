#![feature(test)]
#![allow(incomplete_features)]
#![feature(specialization)]
#![feature(unboxed_closures)]
#![feature(fn_traits)]
//! Crate for supporting data-driven tests.
//!
//! Data-driven tests are tests where individual cases are defined via data rather than in code.
//! This crate implements a custom test runner that adds support for additional test types.
//!
//! # Files-driven test
//!
//! First type of data-driven tests are "file-driven" tests. These tests define a directory to
//! scan for test data, a pattern (a regular expression) to match and, optionally, a set of
//! templates to derive other file paths based on the matched file name. For each matched file,
//! a new test instance is created, with test function arguments derived based on the specified
//! mappings.
//!
//! Each argument of the test function must be mapped either to the pattern or to the template.
//! See the example below for the syntax.
//!
//! The following argument types are supported:
//! * `&str`, `String`: capture file contents as string and pass it to the test function
//! * `&[u8]`, `Vec<u8>`: capture file contents and pass it to the test function
//! * `&Path`: pass file path as-is
//!
//! ### Note
//!
//! Each test could also be marked with `#[test]` attribute, to allow running test from IDEs which
//! have built-in support for `#[test]` tests. However, if such attribute is used, it should go
//! after `#[datatest::files]` attribute, so `datatest` attribute is handled earlier and `#[test]`
//! attribute is removed.
//!
//! ## Example
//!
//! ```rust
//! #![feature(custom_test_frameworks)]
//! #![test_runner(datatest::runner)]
//!
//! #[datatest::files("tests/test-cases", {
//!   input in r"^(.*).input\.txt",
//!   output = r"${1}.output.txt",
//! })]
//! fn sample_test(input: &str, output: &str) {
//!   assert_eq!(format!("Hello, {}!", input), output);
//! }
//! ```
//!
//! ### Ignoring individual tests
//!
//! Individual tests could be ignored by specifying a function of signature
//! `fn(&std::path::Path) -> bool` using the following syntax on the pattern (`if !<func_name>`):
//!
//! ```rust
//! #![feature(custom_test_frameworks)]
//! #![test_runner(datatest::runner)]
//!
//! fn is_ignore(path: &std::path::Path) -> bool {
//!   true // some condition
//! }
//!
//! #[datatest::files("tests/test-cases", {
//!   input in r"^(.*).input\.txt" if !is_ignore,
//!   output = r"${1}.output.txt",
//! })]
//! fn sample_test(input: &str, output: &str) {
//!   assert_eq!(format!("Hello, {}!", input), output);
//! }
//! ```
//!
//! # Data-driven tests
//!
//! Second type of tests supported by this crate are "data-driven" tests. These tests define a
//! YAML file with a list of test cases (via `#[datatest::data(..)]` attribute, see example below).
//! Each test case in this file (the file contents must be an array) is deserialized into the
//! argument type of the test function and a separate test instance is created for it.
//!
//! Test function must take exactly one argument and the type of this argument must implement
//! [`serde::Deserialize`]. Optionally, if this implements [`ToString`] (or [`std::fmt::Display`]),
//! it's [`ToString::to_string`] result is used to generate test name.
//!
//! ### `#[test]` attribute
//!
//! Each test could also be marked with `#[test]` attribute, to allow running test from IDEs which
//! have built-in support for `#[test]` tests. However, if such attribute is used, it should go
//! after `#[datatest::files]` attribute, so `datatest` attribute is handled earlier and `#[test]`
//! attribute is removed.
//!
//! ## Example
//!
//! ```rust
//! #![feature(custom_test_frameworks)]
//! #![test_runner(datatest::runner)]
//!
//! use serde::Deserialize;
//!
//! #[derive(Deserialize)]
//! struct TestCase {
//!   name: String,
//!   expected: String,
//! }
//!
//! #[datatest::data("tests/tests.yaml")]
//! fn sample_test(case: TestCase) {
//!   assert_eq!(case.expected, format!("Hi, {}!", case.name));
//! }
//!
//! # fn main() {}
//! ```
//!
//! ## More examples
//!
//! For more examples, check the [tests](https://github.com/commure/datatest/blob/master/tests/datatest.rs).
extern crate test as rustc_test;

mod data;
mod files;
mod runner;

#[cfg(feature = "unsafe_test_runner")]
mod interceptor;

#[cfg(not(feature = "unsafe_test_runner"))]
mod interceptor {
    pub fn install_interceptor() {}
}

/// Internal re-exports for the procedural macro to use.
#[doc(hidden)]
pub mod __internal {
    pub use crate::data::{DataBenchFn, DataTestDesc, DataTestFn};
    pub use crate::files::{DeriveArg, FilesTestDesc, FilesTestFn, TakeArg};
    pub use crate::runner::assert_test_result;
    pub use crate::rustc_test::Bencher;
    pub use ctor::{ctor, dtor};

    // To maintain registry on stable channel
    pub use crate::runner::{
        check_test_runner, register, RegistrationNode, RegularShouldPanic, RegularTestDesc,
    };
    // i.e. no TCR, use ctor instead
    #[cfg(not(all(feature = "rustc_is_nightly", feature = "test_case_registration")))]
    pub use datatest_derive::{data_ctor_internal, files_ctor_internal};
    // i.e. use TCR
    #[cfg(all(feature = "rustc_is_nightly", feature = "test_case_registration"))]
    pub use datatest_derive::{data_test_case_internal, files_test_case_internal};
}

pub use crate::runner::runner;

// i.e. no TCR, use ctor instead
#[cfg(not(all(feature = "rustc_is_nightly", feature = "test_case_registration")))]
pub use datatest_derive::{
    data_ctor_registration as data, files_ctor_registration as files,
    test_ctor_registration as test,
};

// i.e. use TCR
#[cfg(all(feature = "rustc_is_nightly", feature = "test_case_registration"))]
pub use datatest_derive::{
    data_test_case_registration as data, files_test_case_registration as files,
};

/// Experimental functionality.
#[doc(hidden)]
pub use crate::data::{yaml, DataTestCaseDesc};

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

/// `datatest` test harness entry point. Should be declared in the test module, like in the
/// following snippet:
/// ```rust,no_run
/// datatest::harness!();
/// ```
///
/// Also, `harness` should be set to `false` for that test module in `Cargo.toml` (see [Configuring a target](https://doc.rust-lang.org/cargo/reference/manifest.html#configuring-a-target)).
#[macro_export]
macro_rules! harness {
    () => {
        #[cfg(test)]
        fn main() {
            ::datatest::runner(&[]);
        }
    };
}

/// Helper function used internally.
fn read_to_string(path: &Path) -> String {
    let mut input = String::new();
    File::open(path)
        .map(BufReader::new)
        .and_then(|mut f| f.read_to_string(&mut input))
        .unwrap_or_else(|e| panic!("cannot read test input at '{}': {}", path.display(), e));
    input
}

/// Helper function used internally.
fn read_to_end(path: &Path) -> Vec<u8> {
    let mut input = Vec::new();
    File::open(path)
        .map(BufReader::new)
        .and_then(|mut f| f.read_to_end(&mut input))
        .unwrap_or_else(|e| panic!("cannot read test input at '{}': {}", path.display(), e));
    input
}

use crate::rustc_test::TestType;

/// Helper function used internally, to mirror how rustc_test chooses a TestType.
/// Must be called with the result of `file!()` (called in macro output) to be meaningful.
pub fn test_type(path: &'static str) -> TestType {
    if path.starts_with("src") {
        // `/src` folder contains unit-tests.
        TestType::UnitTest
    } else if path.starts_with("tests") {
        // `/tests` folder contains integration tests.
        TestType::IntegrationTest
    } else {
        // Crate layout doesn't match expected one, test type is unknown.
        TestType::Unknown
    }
}
