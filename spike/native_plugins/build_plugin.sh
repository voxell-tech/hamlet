#!/bin/sh
# Builds a plugin the way Hamlet would: in a build workspace of its own
# making, with the SDK at the path it has in the app's workspace, the app's
# Cargo.lock, flags and toolchain.
#
#   build_plugin.sh <plugin-folder> <build-folder>
set -eu
spike=$(cd "$(dirname "$0")" && pwd)
plugin=$(cd "$1" && pwd)
build=$2
name=$(sed -n 's/^name = "\(.*\)"/\1/p' "$plugin/Cargo.toml" | head -1)

mkdir -p "$build/plugins/$name"
rsync -a --delete --exclude target "$spike/sdk/hamlet_sdk/" "$build/hamlet_sdk/"
rsync -a --delete "$spike/sdk/.cargo/" "$build/.cargo/"
cp "$spike/sdk/rust-toolchain.toml" "$build/"
[ -f "$build/Cargo.lock" ] || cp "$spike/sdk/Cargo.lock" "$build/"
sed 's/^members = .*/members = ["hamlet_sdk", "plugins\/*"]/' "$spike/sdk/Cargo.toml" > "$build/Cargo.toml"

rsync -a --delete --exclude target --exclude Cargo.lock --exclude .cargo \
    --exclude rust-toolchain.toml "$plugin/" "$build/plugins/$name/"
# The plugin is a member here: no workspace of its own, and the SDK is
# the build workspace's copy.
sed -e '/^\[workspace\]$/d' \
    -e 's#^hamlet_sdk = .*#hamlet_sdk = { path = "../../hamlet_sdk" }#' \
    "$plugin/Cargo.toml" > "$build/plugins/$name/Cargo.toml"

cd "$build"
env -u RUSTFLAGS -u CARGO_ENCODED_RUSTFLAGS cargo build --release -p "$name"
