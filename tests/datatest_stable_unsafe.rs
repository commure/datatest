//! Run with: `cargo +stable test --features unsafe_test_runner --test datatest_stable_unsafe`

#[cfg(feature = "unsafe_test_runner")]

// We want to share tests between "nightly" and "stable" suites. These have to be two different
// suites as we set `harness = false` for the "stable" one.
include!("tests/mod.rs");
