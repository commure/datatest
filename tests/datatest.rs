#![cfg(feature = "nightly")]
#![feature(custom_test_frameworks)]
#![test_runner(datatest::runner)]

// We want to share tests between "nightly" and "stable" suites. These have to be two different
// suites as we set `harness = false` for the "stable" one.
include!("tests/mod.rs");
