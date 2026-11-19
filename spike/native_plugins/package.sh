#!/bin/sh
# Lays out a release in dist/: the app, the SDK and the toolchain's std,
# side by side.
set -eu
spike=$(cd "$(dirname "$0")" && pwd)
dist="$spike/dist"
rm -rf "$dist" && mkdir -p "$dist"
cp "$spike/sdk/target/release/app" "$spike/sdk/target/release/libhamlet_sdk.dylib" "$dist/"
cp "$(cd "$spike/sdk" && rustc --print target-libdir)"/libstd-*.dylib "$dist/"
