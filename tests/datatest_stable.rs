#[cfg(feature = "subvert_stable_guarantees")]

// We want to share tests between "nightly" and "stable" suites. These have to be two different
// suites as we set `harness = false` for the "stable" one.
include!("tests/mod.rs");

datatest::harness!();
