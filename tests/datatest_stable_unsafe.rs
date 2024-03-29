//! cargo +stable test --features subvert_stable_guarantees,unsafe_test_runner

#![cfg(feature = "rustc_is_stable")]
#![cfg(feature = "unsafe_test_runner")]

// We want to share tests between "nightly" and "stable" suites. These have to be two different
// suites as we set `harness = false` for the "stable" one.
include!("tests/mod.rs");
