//! Builds the vendored mruby with rake and compiles the small C shim Knut
//! uses to talk to it.
//!
//! `MRUBY_SRC` overrides the mruby source directory (default: the
//! `vendor/mruby` submodule), which keeps sandboxed builds such as Nix
//! working without network access.

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let mruby_src = env::var_os("MRUBY_SRC")
        .map(PathBuf::from)
        .unwrap_or_else(|| manifest_dir.join("vendor/mruby"));
    let build_config = manifest_dir.join("mruby/build_config.rb");
    let build_dir = out_dir.join("mruby-build");

    if !mruby_src.join("Rakefile").exists() {
        panic!(
            "mruby sources not found at {}; run `git submodule update --init` or set MRUBY_SRC",
            mruby_src.display()
        );
    }

    let status = Command::new("rake")
        .arg("-f")
        .arg(mruby_src.join("Rakefile"))
        .arg("-j4")
        .arg("all")
        .current_dir(&mruby_src)
        .env("MRUBY_CONFIG", &build_config)
        .env("MRUBY_BUILD_DIR", &build_dir)
        .status()
        .expect("failed to run rake; mruby needs ruby and rake to build");
    assert!(status.success(), "building mruby failed");

    let host_dir = build_dir.join("host");
    cc::Build::new()
        .file(manifest_dir.join("mruby/shim.c"))
        .include(mruby_src.join("include"))
        .include(host_dir.join("include"))
        .define("MRB_UTF8_STRING", None)
        .flag_if_supported("-Wno-unused-parameter")
        .compile("knut_mruby_shim");

    println!("cargo:rustc-link-search=native={}", host_dir.join("lib").display());
    println!("cargo:rustc-link-lib=static=mruby");
    println!("cargo:rerun-if-changed=mruby/shim.c");
    println!("cargo:rerun-if-changed=mruby/build_config.rb");
    println!("cargo:rerun-if-env-changed=MRUBY_SRC");
}
