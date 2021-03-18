extern crate version_check as rustc;

fn main() {
    let subverted = cfg!(feature = "subvert_stable_guarantees");

    if subverted {
        // This will cease to work sometime between cargo 1.50 and 1.52. Nightly clippy
        // --all-features at the time of writing warns that:
        //
        // warning: Cannot set `RUSTC_BOOTSTRAP=1` from build script of `datatest v0.6.4 (...)`
        // note: Crates cannot set `RUSTC_BOOTSTRAP` themselves, as doing so would subvert the
        // stability guarantees of Rust for your project.
        println!("cargo:rustc-env=RUSTC_BOOTSTRAP=1");
    }

    let nightly = rustc::is_feature_flaggable().unwrap_or(false);
    if nightly {
        println!("cargo:rustc-cfg=feature=\"rustc_is_nightly\"");
    } else {
        println!("cargo:rustc-cfg=feature=\"rustc_is_stable\"");
        if !subverted {
            println!("cargo:warning=attempting to compile datatest on stable without opting in to subvert_stable_guarantees feature; will fail");
        }
    }
    // See src/runner.rs
    if rustc::is_min_version("1.52.0").unwrap_or(false) {
        println!("cargo:rustc-cfg=feature=\"rustc_test_TestOpts_filters_vec\"");
    }
}
