#!/bin/sh
# Builds every plugin that comes with Hamlet, the crates of the
# workspace in plugins/, for wasm32-wasip2, and lays each one out as
# plugins/dist/<id>/ with its plugin.toml and plugin.wasm, and the
# plugin.wasm compiled for the engine in compiled/, where the engine
# finds them. A running engine reloads what changed.
#
#   scripts/build_plugins.sh

set -eu

target=wasm32-wasip2
root=$(cd "$(dirname "$0")/.." && pwd)
plugins="$root/plugins"
dist="$plugins/dist"
built="$plugins/target/$target/release"

if command -v rustup >/dev/null 2>&1 &&
    ! rustup target list --installed | grep -qx "$target"; then
    echo "The Rust target $target is not installed. Add it with:" >&2
    echo "    rustup target add $target" >&2
    exit 1
fi

(cd "$plugins" && cargo build --release --target "$target")

mkdir -p "$dist"
laid=" "
for manifest in "$plugins"/*/plugin.toml; do
    [ -f "$manifest" ] || continue
    folder=$(dirname "$manifest")
    # The crate's folder is its name, and its library's.
    name=$(basename "$folder" | tr '-' '_')
    id=$(sed -n 's/^id[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' \
        "$manifest" | head -n 1)
    if [ -z "$id" ]; then
        echo "$manifest names no id" >&2
        exit 1
    fi
    wasm="$built/$name.wasm"
    if [ ! -f "$wasm" ]; then
        echo "$wasm was not built" >&2
        exit 1
    fi
    out="$dist/$id"
    mkdir -p "$out"
    # Each file is written beside its place and moved over it, so the
    # engine never reads half of one.
    # Compiled before the plugin.wasm is moved into place, so a
    # running engine that reloads it finds the compiled one waiting.
    (cd "$root" && cargo run --release --quiet \
        -p hamlet_engine_harness --bin precompile_plugins -- \
        "$wasm" "$out")
    cp "$manifest" "$out/.plugin.toml.new"
    mv -f "$out/.plugin.toml.new" "$out/plugin.toml"
    cp "$wasm" "$out/.plugin.wasm.new"
    mv -f "$out/.plugin.wasm.new" "$out/plugin.wasm"
    laid="$laid$id "
    echo "$id: $out"
done

# A plugin no longer in the workspace is taken out of dist/, so the
# engine stops loading it.
for out in "$dist"/*/; do
    [ -d "$out" ] || continue
    id=$(basename "$out")
    case "$laid" in
        *" $id "*) ;;
        *)
            rm -rf "$out"
            echo "$id: removed"
            ;;
    esac
done
