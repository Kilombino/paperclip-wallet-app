#!/bin/bash
# Reproducible arm64 build of the Ark engine for Kilombino wallet.
#
# Toolchain: rustc 1.98.0, cargo-ndk 4.1.2, Android NDK 27.1.12297006, API level 26.
# Paths are remapped so the result does not depend on where the repo or the
# toolchains live: two clean clones in different directories give the same SHA-256.
set -euo pipefail
cd "$(dirname "$0")/.."
: "${ANDROID_NDK_HOME:?set ANDROID_NDK_HOME to NDK 27.1.12297006}"
export RUSTFLAGS="--remap-path-prefix=$PWD=/src --remap-path-prefix=$HOME/.cargo=/cargo --remap-path-prefix=$HOME/.rustup=/rustup"
cargo ndk -t arm64-v8a -P 26 build --locked --profile android -p kilombino-ark
sha256sum target/aarch64-linux-android/android/libkilombino_ark.so
