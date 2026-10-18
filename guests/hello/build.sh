#!/bin/sh
# Builds the plugin and installs it into $HAMLET_HOME/plugins/hello/.
# Needs: rustup target add wasm32-wasip2
set -e
: "${HAMLET_HOME:?set HAMLET_HOME to the home to install into}"
cd "$(dirname "$0")"
cargo build --release --target wasm32-wasip2
dest="$HAMLET_HOME/plugins/hello"
mkdir -p "$dest"
cp target/wasm32-wasip2/release/hello.wasm "$dest/plugin.wasm"
cp manifest.toml "$dest/manifest.toml"
echo "copied to $dest (the app asks the chief to install it)"
