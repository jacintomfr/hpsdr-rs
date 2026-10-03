//! Builds the vendored RADE C library (vendor/rade_c, a git submodule -- see
//! README for the `git submodule update --init` step a fresh clone needs)
//! plus the patched Opus/FARGAN vocoder it depends on, then generates raw
//! bindings for both plus src/rade_shim.c's own small C surface over the
//! vocoder. Adapted from SDRoxide's own crates/sdroxide-rade/build.rs
//! (https://github.com/madmedicnl/sdroxide, upstream commit 63b0b29,
//! GPL-3.0-or-later -- see src/rade.rs's own doc comment for the full
//! licensing story), with two real changes, both confirmed necessary by
//! actually building this on the user's own Windows/MSYS2 machine rather
//! than guessed at:
//!
//! 1. `de_verbatim` below: bindgen's own libclang fails to resolve
//!    `#include "rade_tx.h"` (and friends) when handed a `\\?\`-prefixed
//!    verbatim path -- what `Path::canonicalize()` returns on Windows -- as
//!    a `-I` search dir or header path. cc/gcc (the shim.c compile, and
//!    rade_c's own CMake build) handle the same paths fine; only libclang's
//!    include resolution does not. Stripping the verbatim prefix for
//!    bindgen's own arguments (only) works around it.
//! 2. This project vendors the shim as `src/rade_shim.c`/`.h` (alongside
//!    the rest of this project's own adapted-from-SDRoxide modem ports --
//!    see rtty.rs/sstv.rs's identical convention) rather than inside the
//!    build-dependency crate SDRoxide keeps it in.
//!
//! Beyond the WDSP/fftw3 toolchain build.rs's own doc comment already asks
//! for, this needs: `autoconf`/`automake`/`libtool`/`gettext-devel` (MSYS
//! packages -- the *plain* `pacman -S autoconf automake libtool
//! gettext-devel` versions, not the `mingw-w64-x86_64-*` ones: rade_c's own
//! CMake runs Opus's `autogen.sh` through the MSYS Makefiles generator, so
//! it needs those tools on the MSYS side of PATH, not the MinGW side) and
//! `mingw-w64-x86_64-clang` (for bindgen's libclang). The first build also
//! needs network access -- rade_c's own CMake downloads Opus source and the
//! trained model weights from GitHub via `ExternalProject_Add`.

use std::path::PathBuf;

pub fn build() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());

    let rade_c = manifest.join("vendor/rade_c");
    if !rade_c.join("src/rade_api.h").exists() {
        panic!(
            "vendored RADE sources are missing at {}\n\
             run: git submodule update --init --recursive",
            rade_c.display()
        );
    }
    let rade_c = rade_c.canonicalize().expect("canonicalize vendor/rade_c");

    let wrapper = write_wrapper_project(&out, &rade_c);

    // Always Release: the neural encoder/decoder is unusably slow
    // unoptimized, including under a plain `cargo build` -- confirmed the
    // same way SDRoxide's own comment says (real testing, not a guess).
    let dst = cmake::Config::new(&wrapper)
        .define("RADE_C_DIR", rade_c.to_string_lossy().as_ref())
        .profile("Release")
        .build_target("rade_static")
        .build();

    let build = dst.join("build");
    // ExternalProject's default layout for rade_c's `build_opus` target,
    // which is configured BUILD_IN_SOURCE so its source and binary dirs
    // coincide.
    let opus_src = build.join("build_opus-prefix/src/build_opus");
    let opus_lib_dir = opus_src.join(".libs");
    assert!(
        opus_lib_dir.is_dir(),
        "expected the Opus build at {} -- did rade_c's ExternalProject change layout?",
        opus_lib_dir.display()
    );

    let includes: Vec<PathBuf> = vec![
        rade_c.join("src"),
        opus_src.clone(),
        opus_src.join("dnn"),
        opus_src.join("celt"),
        opus_src.join("include"),
    ];

    // The vocoder half: RADE's own API stops at feature vectors, so speech
    // <-> features goes through a small C shim over Opus's FARGAN/LPCNet.
    let mut shim = cc::Build::new();
    shim.file(manifest.join("src/rade_shim.c")).define("HAVE_CONFIG_H", None).opt_level(2);
    for inc in &includes {
        shim.include(inc);
    }
    shim.compile("hpsdr_rs_rade_shim");

    println!("cargo:rustc-link-search=native={}", build.display());
    println!("cargo:rustc-link-lib=static=rade_static");
    println!("cargo:rustc-link-search=native={}", opus_lib_dir.display());
    println!("cargo:rustc-link-lib=static=opus");
    println!("cargo:rustc-link-lib=dylib=m");

    // See this file's own doc comment (point 1) for why paths passed to
    // bindgen specifically need de-verbatim-ing, unlike the cc::Build above
    // or cmake::Config, both of which are fine with the raw canonicalized
    // (`\\?\`-prefixed) paths.
    fn de_verbatim(p: &std::path::Path) -> String {
        let s = p.to_string_lossy().to_string();
        s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()
    }
    let mut builder = bindgen::Builder::default()
        .header(de_verbatim(&rade_c.join("src/rade_api.h")))
        .header(de_verbatim(&manifest.join("src/rade_shim.h")))
        .allowlist_function("rade_.*")
        .allowlist_function("sdrx_voc_.*")
        .allowlist_type("RADE_COMP")
        .allowlist_var("RADE_.*")
        // `struct rade` is fully defined in the header -- it embeds the V1
        // and V2 transmit and receive state inline -- but we only ever
        // hold a pointer to one. Blocklisting it keeps Rust off that
        // layout entirely; see the opaque declaration in rade_sys.rs.
        .blocklist_type("rade")
        .layout_tests(false)
        .derive_debug(false)
        .clang_arg("-DHAVE_CONFIG_H");
    for inc in &includes {
        builder = builder.clang_arg(format!("-I{}", de_verbatim(inc)));
    }
    builder
        .generate()
        .expect("bindgen rade_api.h + rade_shim.h")
        .write_to_file(out.join("rade_bindings.rs"))
        .expect("write rade_bindings.rs");

    println!("cargo:rerun-if-changed=src/rade_shim.c");
    println!("cargo:rerun-if-changed=src/rade_shim.h");
    println!("cargo:rerun-if-changed={}", rade_c.join("src").display());
    println!("cargo:rerun-if-changed={}", rade_c.join("cmake").display());
}

/// Generate the wrapper CMake project into `OUT_DIR` and return its path.
///
/// It mirrors rade_c's own top-level `CMakeLists.txt` (include `BuildOpus`,
/// then add `src`) and adds one static target built from upstream's own
/// source list. `BuildOpus.cmake` resolves its Opus patches relative to
/// `CMAKE_SOURCE_DIR`, which is now *this* project, so the two `.diff`
/// files are copied in alongside. On Windows the C sources are copied into
/// the build tree as well -- see the comment in the generated file.
fn write_wrapper_project(out: &std::path::Path, rade_c: &std::path::Path) -> PathBuf {
    let wrapper = out.join("wrapper");
    std::fs::create_dir_all(wrapper.join("src")).expect("create wrapper dir");
    for diff in ["opus-nnet.h.diff", "opus-nnet.c.diff"] {
        std::fs::copy(rade_c.join("src").join(diff), wrapper.join("src").join(diff))
            .unwrap_or_else(|e| panic!("copy {diff}: {e}"));
    }
    std::fs::write(
        wrapper.join("CMakeLists.txt"),
        r#"# Generated by build_rade.rs -- do not edit.
cmake_minimum_required(VERSION 3.16)
project(hpsdr_rs_rade C)
set(CMAKE_C_STANDARD 11)
set(CMAKE_POSITION_INDEPENDENT_CODE ON)

# Cross-compiling to Linux/arm64 with GCC (e.g. `cargo build --target
# aarch64-unknown-linux-gnu` for the Raspberry Pi package): BuildOpus.cmake
# only passes `--host` to Opus's ./configure from CMAKE_C_COMPILER_TARGET,
# which is a Clang-only variable and empty with GCC -- so without this Opus
# is silently built for the BUILD machine and the final link fails with
# "skipping incompatible libopus.a".
if(CMAKE_CROSSCOMPILING AND NOT CMAKE_C_COMPILER_TARGET AND CMAKE_SYSTEM_PROCESSOR MATCHES "aarch64")
  set(CMAKE_C_COMPILER_TARGET aarch64-linux-gnu)
endif()

# Fetches and patches the FARGAN/LPCNet-enabled Opus, and defines the imported
# `opus` target plus its include directories for everything below.
include(${RADE_C_DIR}/cmake/BuildOpus.cmake)

# Upstream's own targets, unmodified. We never build its shared `rade`; we only
# borrow the target's source list.
add_subdirectory(${RADE_C_DIR}/src rade_c_src)

get_target_property(RADE_SOURCES rade SOURCES)
get_target_property(RADE_SRC_DIR rade SOURCE_DIR)

if(WIN32)
    # The windows-gnu build runs through MSYS make, whose makefile parser has
    # no notion of drive letters. For a source outside the build tree CMake
    # emits the rule line
    #     CMakeFiles/rade_static.dir/....obj: D:/.../rade_api.c
    # whose second colon makes it a malformed static pattern rule -- make stops
    # with "target pattern contains no '%'". Compiling from copies inside the
    # binary directory keeps every path in the generated makefiles relative.
    set(RADE_COPY_DIR ${CMAKE_CURRENT_BINARY_DIR}/rade_src)
    set(RADE_ABS_SOURCES ${RADE_SOURCES})
    list(TRANSFORM RADE_ABS_SOURCES PREPEND "${RADE_SRC_DIR}/")
    file(GLOB RADE_HEADERS "${RADE_SRC_DIR}/*.h")
    file(COPY ${RADE_ABS_SOURCES} ${RADE_HEADERS} DESTINATION ${RADE_COPY_DIR})
    list(TRANSFORM RADE_SOURCES PREPEND "${RADE_COPY_DIR}/")
else()
    list(TRANSFORM RADE_SOURCES PREPEND "${RADE_SRC_DIR}/")
endif()

add_library(rade_static STATIC ${RADE_SOURCES})
target_include_directories(rade_static PRIVATE ${RADE_SRC_DIR})
target_compile_definitions(rade_static PRIVATE IS_BUILDING_RADE_API=1)
target_link_libraries(rade_static opus m)
"#,
    )
    .expect("write wrapper CMakeLists.txt");
    wrapper
}
