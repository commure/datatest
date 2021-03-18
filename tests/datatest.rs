//! cargo +nightly test                         # test_case_registration enabled
//! cargo +nightly test --no-default-features   # no test_case_registration, uses ctor

// self-testing config only:
#![cfg(all(feature = "rustc_is_nightly"))]
#![feature(custom_test_frameworks)]
#![test_runner(datatest::runner)]

// We want to share tests between "nightly" and "stable" suites. These have to be two different
// suites as we set `harness = false` for the "stable" one.
include!("tests/mod.rs");

// Regular tests still work

#[test]
fn regular_test() {
    println!("regular tests also work!");
}

#[test]
fn regular_test_result() -> Result<(), Box<dyn std::error::Error>> {
    println!("regular tests also work!");
    Ok(())
}
