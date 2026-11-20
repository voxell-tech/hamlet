#!/bin/sh
# Lays out the plugin kit in dist/kit/: the metadata of every crate in the
# SDK and the proc macros among them, taken from the SDK's own build. With
# it, rustc builds a plugin against the release without building Bevy.
#
#   kit.sh   (after package.sh)
set -eu
spike=$(cd "$(dirname "$0")" && pwd)
kit="$spike/dist/kit"
rm -rf "$kit" && mkdir -p "$kit"
cd "$spike/sdk"
cargo build --release -p hamlet_core --message-format=json 2>/dev/null |
    jq -r 'select(.reason == "compiler-artifact") | .filenames[]
        | select(endswith(".rmeta") or (contains("/out/lib") and endswith(".dylib")))' |
    grep -v '/libhamlet_core\.' |
    while read -r file; do cp -p "$file" "$kit/"; done
# The crates the SDK's macros name by their own names: `bevy` for Bevy's
# derives, `fynix` for `#[element]`. One `name=file` line each.
cargo build --release -p hamlet_core --message-format=json 2>/dev/null |
    jq -r 'select(.reason == "compiler-artifact"
            and (.target.name == "bevy" or .target.name == "fynix"))
        | .target.name + "=" + (.filenames[] | select(endswith(".rmeta"))
            | split("/") | last)' > "$kit/externs"
