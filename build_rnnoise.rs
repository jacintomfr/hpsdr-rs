//! Downloads RNNoise's trained-model tarball (not vendored in the git
//! submodule -- upstream publishes it separately, see vendor/rnnoise/
//! download_model.sh) if not already present, then builds the library
//! from vendor/rnnoise's own portable (non-x86-SIMD) source list via a
//! plain `cc::Build` -- no autotools/cmake needed, unlike build_rade.rs's
//! Opus dependency: RNNoise's own build (Makefile.am's `RNNOISE_SOURCES`,
//! read directly rather than guessed at) is just a handful of portable C
//! files plus the downloaded weight table.
//!
//! vendor/rnnoise is a git submodule (https://github.com/xiph/rnnoise,
//! BSD-3-Clause) -- see README for the `git submodule update --init`
//! step a fresh clone needs.

use std::path::{Path, PathBuf};
use std::process::Command;

pub fn build() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let root = manifest.join("vendor/rnnoise");
    let src = root.join("src");
    if !src.join("denoise.c").exists() {
        panic!(
            "vendored RNNoise sources are missing at {}\n\
             run: git submodule update --init --recursive",
            root.display()
        );
    }

    fetch_model(&root, &src);

    // Portable RNNOISE_SOURCES from vendor/rnnoise/Makefile.am, minus the
    // optional x86 SSE4.1/AVX2 runtime-dispatch sources (RNN_ENABLE_X86_RTCD)
    // -- nnet.c/nnet_default.c is the plain-C fallback those accelerate,
    // and 10ms frames of mono mic audio are cheap enough without them.
    let sources = [
        "denoise.c",
        "rnn.c",
        "pitch.c",
        "kiss_fft.c",
        "celt_lpc.c",
        "nnet.c",
        "nnet_default.c",
        "parse_lpcnet_weights.c",
        "rnnoise_data.c",
        "rnnoise_tables.c",
    ];

    let mut build = cc::Build::new();
    build.include(root.join("include")).include(&src).opt_level(2).warnings(false);
    for f in sources {
        build.file(src.join(f));
    }
    build.compile("hpsdr_rs_rnnoise");

    println!("cargo:rerun-if-changed={}", src.display());
    println!("cargo:rerun-if-changed={}", root.join("include").display());
}

/// Downloads and extracts `rnnoise_data.{c,h}` per vendor/rnnoise/
/// download_model.sh's own logic (a single tarball named by the hash in
/// `model_version`), skipped entirely once already present -- so this
/// only costs anything on the very first build, same as build_rade.rs's
/// own Opus/weights download.
fn fetch_model(root: &Path, src: &Path) {
    if src.join("rnnoise_data.c").exists() {
        return;
    }
    let hash = std::fs::read_to_string(root.join("model_version"))
        .expect("read vendor/rnnoise/model_version")
        .trim()
        .to_string();
    let filename = format!("rnnoise_data-{hash}.tar.gz");
    let url = format!("https://media.xiph.org/rnnoise/models/{filename}");
    let out = std::env::var("OUT_DIR").map(PathBuf::from).unwrap_or_else(|_| root.to_path_buf());
    let archive = out.join(&filename);

    if !archive.exists() {
        let status = Command::new("curl")
            .args(["-L", "-f", "-o"])
            .arg(&archive)
            .arg(&url)
            .status()
            .expect("run curl to fetch the RNNoise model");
        assert!(status.success(), "curl failed fetching {url}");
    }

    // The tarball's own entries are already prefixed `src/rnnoise_data.c`
    // etc. (plus a `models/` dir of training checkpoints this project has
    // no use for) -- extract at vendor/rnnoise's own root, matching
    // download_model.sh's own working directory, not into `src` directly
    // (which would double up the path).
    let status = Command::new("tar")
        .arg("xzf")
        .arg(&archive)
        .arg("-C")
        .arg(root)
        .status()
        .expect("run tar to extract the RNNoise model");
    assert!(status.success(), "tar failed extracting {}", archive.display());
    assert!(src.join("rnnoise_data.c").exists(), "rnnoise_data.c missing after extracting {url}");
}
