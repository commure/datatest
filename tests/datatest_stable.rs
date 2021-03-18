//! cargo +stable test --features subvert_stable_guarantees

// This test suite is configured with `harness = false` in Cargo.toml.
// So we need to make sure it has a main function when testing nightly
#[cfg(not(feature = "rustc_is_stable"))]
fn main() {}
// And uses the datatest harness when testing stable
#[cfg(feature = "rustc_is_stable")]
datatest::harness!();

#[cfg(feature = "rustc_is_stable")]
mod stable {
    // Regular test have to use `datatest` variant of `#[test]` to work.
    use datatest::test;

    // We want to share tests between "rustc_is_nightly" and "rustc_is_stable" suites. These have to be two different
    // suites as we set `harness = false` for the "stable" one.
    include!("tests/mod.rs");

    #[test]
    fn regular_test() {
        println!("regular tests also work!");
    }

    #[test]
    fn regular_test_result() -> Result<(), Box<dyn std::error::Error>> {
        println!("regular tests also work!");
        Ok(())
    }
}
