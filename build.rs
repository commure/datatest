fn main() {
    #[cfg(feature = "subvert_stable_guarantees")]
    println!("cargo:rustc-env=RUSTC_BOOTSTRAP=1");
}
