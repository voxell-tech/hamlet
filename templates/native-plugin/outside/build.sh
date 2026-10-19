#!/bin/sh
# Builds the plugin against an SDK kit and puts its manifest and
# library in a plugin folder: the one given, or
# $HAMLET_HOME/plugins/{{name}}.
#
#   HAMLET_SDK=<kit> ./build.sh [folder] [release]
#
# The kit is made from the Hamlet that loads the plugin, with
# scripts/make-native-sdk.sh of its checkout. It holds that build's
# compiled engine and SDK, and the plugin links those very crates, so
# Hamlet's gate lets it through. A kit of another build is turned
# down by the gate, and one of another compiler by the compiler.
set -e
: "${HAMLET_SDK:?set HAMLET_SDK to a kit made by scripts/make-native-sdk.sh}"
dest=
release=
for arg in "$@"; do
    case $arg in
        release) release=--release ;;
        /*) dest=$arg ;;
        *) dest="$PWD/$arg" ;;
    esac
done
if [ -z "$dest" ]; then
    : "${HAMLET_HOME:?give a folder, or set HAMLET_HOME to the home to install into}"
    dest="$HAMLET_HOME/plugins/{{name}}"
fi
here=$(cd "$(dirname "$0")" && pwd)
kit=$(cd "$HAMLET_SDK" && pwd)
toolchain=$(cat "$kit/toolchain")
target=$(cat "$kit/target")

set -- -L "dependency=$kit/deps"
while IFS= read -r line; do
    set -- "$@" --extern "${line%%=*}=$kit/${line#*=}"
done < "$kit/externs"

cd "$here"
cargo "+$toolchain" rustc --lib --target "$target" $release -- "$@"
profile=debug
[ -n "$release" ] && profile=release
mkdir -p "$dest"
cp "${CARGO_TARGET_DIR:-$here/target}/$target/$profile/{{library}}" \
    "$dest/{{library}}"
cp "$here/manifest.toml" "$dest/manifest.toml"
echo "built into $dest"
echo "Hamlet asks before it loads this, and loads it when it starts."
