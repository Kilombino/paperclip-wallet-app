#!/bin/bash
# The Ark engine for Kilowallet iOS: KilombinoArk.xcframework (device arm64, simulator arm64 +
# x86_64), a static library with kilombino_ark.h and a module map, so Swift can `import KilombinoArk`.
#
# Needs macOS with Xcode, rustup (rustc 1.98.0, as Android) and protoc. The crate stays a cdylib
# for Android: the static library is asked of rustc here only (`cargo rustc --crate-type`), and
# its C functions are compiled for iOS only, so the Android build is unchanged.
set -euo pipefail
cd "$(dirname "$0")/.."
export IPHONEOS_DEPLOYMENT_TARGET=${IPHONEOS_DEPLOYMENT_TARGET:-16.0}
export RUSTFLAGS="--remap-path-prefix=$PWD=/src --remap-path-prefix=$HOME/.cargo=/cargo --remap-path-prefix=$HOME/.rustup=/rustup"
targets="aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios"
rustup target add $targets
for t in $targets; do
  cargo rustc --locked --profile android -p kilombino-ark --lib --target "$t" --crate-type staticlib
done
out=target/ios; rm -rf "$out"; mkdir -p "$out/headers" "$out/sim"
cp kilombino-ark/include/kilombino_ark.h kilombino-ark/include/module.modulemap "$out/headers/"
lipo -create target/aarch64-apple-ios-sim/android/libkilombino_ark.a target/x86_64-apple-ios/android/libkilombino_ark.a \
  -output "$out/sim/libkilombino_ark.a"
xcodebuild -create-xcframework \
  -library target/aarch64-apple-ios/android/libkilombino_ark.a -headers "$out/headers" \
  -library "$out/sim/libkilombino_ark.a" -headers "$out/headers" \
  -output "$out/KilombinoArk.xcframework"
cd "$out"
find KilombinoArk.xcframework | sort
du -sh KilombinoArk.xcframework KilombinoArk.xcframework/*
nm -gU KilombinoArk.xcframework/ios-arm64/libkilombino_ark.a 2>/dev/null | grep -E ' _kilombino_ark_(start|stop|free)$'
# A stable zip (sorted entries, fixed times) so the hash depends on the contents only.
find KilombinoArk.xcframework -exec touch -h -t 202601010000 {} +
find KilombinoArk.xcframework \( -type f -o -type l \) | LC_ALL=C sort | zip -q -X -y -9 KilombinoArk.xcframework.zip -@
shasum -a 256 KilombinoArk.xcframework.zip | tee KilombinoArk.xcframework.zip.sha256
