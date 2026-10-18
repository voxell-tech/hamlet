#!/bin/sh
# Copies the crates a plugin of code builds from into <plugin dir>/vendor
# and has cargo read them from there, so that the plugin builds with no
# network. Run it where there is a network (or a filled cargo cache), for
# example by the chief, before an agent builds inside its sandbox.
#
#   scripts/vendor-plugin-deps.sh <plugin dir>
set -e

[ $# -eq 1 ] || { echo "usage: $0 <plugin dir>" >&2; exit 2; }
dir=$1
[ -f "$dir/Cargo.toml" ] || { echo "no Cargo.toml in $dir" >&2; exit 1; }

cd "$dir"
mkdir -p .cargo
cargo vendor --versioned-dirs vendor > .cargo/config.toml
echo "vendored into $dir/vendor"
echo "build with: CARGO_HOME=\$PWD/.cargo-home cargo build --offline \\"
echo "  --release --target wasm32-wasip2"
