extern crate bindgen;

fn main() {
    // Tell cargo to invalidate the built crate whenever the wrapper changes
    println!("cargo:rerun-if-changed=depend/hash");

    let mut base_config = cc::Build::new();
    base_config.include("depend/hash/include");
    base_config.file("depend/hash/hash.c");
    base_config.file("depend/hash/crypto-ops.c");
    base_config.file("depend/hash/keccak.c");
    base_config.compile("hash");

    println!("cargo:rustc-link-lib=static=hash");
}
