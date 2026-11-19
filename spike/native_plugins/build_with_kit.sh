#!/bin/sh
# Builds a plugin with rustc against the release in dist/ and its kit, the
# way Hamlet would without cargo: no Bevy to build and no cargo home, so the
# kit and the plugin can be anywhere.
#
#   build_with_kit.sh <plugin-folder> <out-folder>   (after kit.sh)
#
# A plugin may name only hamlet_sdk. Bevy's derives and fynix's
# `#[element]` read the manifest in CARGO_MANIFEST_DIR for a `bevy` or
# `fynix` dependency, so they get one written here, and the crates are
# passed under those names.
set -eu
spike=$(cd "$(dirname "$0")" && pwd)
plugin=$(cd "$1" && pwd)
mkdir -p "$2"
out=$(cd "$2" && pwd)
dist="$spike/dist"
name=$(sed -n 's/^name = "\(.*\)"/\1/p' "$plugin/Cargo.toml" | head -1)
edition=$(sed -n 's/^edition = "\(.*\)"/\1/p' "$plugin/Cargo.toml" | head -1)
rustc=${RUSTC:-$(cd "$spike/sdk" && rustup which rustc)}

mkdir -p "$out/manifest"
cat > "$out/manifest/Cargo.toml" <<EOF
[package]
name = "$name"
version = "0.1.0"
edition = "${edition:-2024}"

[dependencies]
hamlet_sdk = "0.1.0"
EOF
externs=""
while IFS== read -r crate file; do
    echo "$crate = \"*\"" >> "$out/manifest/Cargo.toml"
    externs="$externs --extern $crate=$dist/kit/$file"
done < "$dist/kit/externs"

CARGO_MANIFEST_DIR="$out/manifest" CARGO_PKG_NAME="$name" CARGO_CRATE_NAME="$name" \
    "$rustc" --crate-name "$name" --edition "${edition:-2024}" --crate-type dylib \
    "$plugin/src/lib.rs" --out-dir "$out" \
    -C opt-level=3 -C embed-bitcode=no -C prefer-dynamic -C metadata="$name" \
    -L dependency="$dist/kit" \
    --extern hamlet_sdk="$dist/libhamlet_sdk.dylib" $externs
