#!/bin/sh
# Builds the plugin and puts its manifest and component in a plugin
# folder: the one given, or $HAMLET_HOME/plugins/diary.
# Needs: rustup target add wasm32-wasip2
set -e
if [ -n "$1" ]; then
    case $1 in
        /*) dest=$1 ;;
        *) dest="$PWD/$1" ;;
    esac
else
    : "${HAMLET_HOME:?give a folder, or set HAMLET_HOME to the home to install into}"
    dest="$HAMLET_HOME/plugins/diary"
fi
cd "$(dirname "$0")"
cargo build --release --target wasm32-wasip2
mkdir -p "$dest"
cp target/wasm32-wasip2/release/diary.wasm "$dest/plugin.wasm"
cp manifest.toml "$dest/manifest.toml"
echo "built into $dest (the app asks the chief to install it)"
