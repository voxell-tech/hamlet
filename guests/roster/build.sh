#!/bin/sh
# Builds the plugin and installs it into $HAMLET_HOME/plugins/roster/.
# Needs: rustup target add wasm32-wasip2
set -e
: "${HAMLET_HOME:?set HAMLET_HOME to the home to install into}"
cd "$(dirname "$0")"
cargo build --release --target wasm32-wasip2
dest="$HAMLET_HOME/plugins/roster"
mkdir -p "$dest"
cp target/wasm32-wasip2/release/roster.wasm "$dest/plugin.wasm"
cp manifest.toml "$dest/manifest.toml"
echo "installed $dest (approve it, and grant read.agents, in $HAMLET_HOME/plugins/approved.toml)"
