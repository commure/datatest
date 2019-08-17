#![cfg(feature = "nightly")]
#![feature(custom_test_frameworks)]
#![test_runner(datatest::runner)]

include!("tests/mod.rs");
