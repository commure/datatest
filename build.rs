use version_check::Channel;

fn main() {
    let is_nightly = Channel::read().map_or(false, |ch| ch.is_nightly());
    if is_nightly {
        println!("cargo:rustc-cfg=feature=\"nightly\"");
    } else {
        println!("cargo:rustc-cfg=feature=\"stable\"");
    }
    println!("cargo:rustc-env=RUSTC_BOOTSTRAP=1");
}
