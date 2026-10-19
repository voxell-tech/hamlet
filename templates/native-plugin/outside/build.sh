#!/bin/sh
# Builds the plugin against an SDK kit and puts its manifest and
# library in a plugin folder: the one given, or
# $HAMLET_HOME/plugins/{{name}}.
#
#   HAMLET_SDK=<kit> ./build.sh [folder] [release]
#
# The kit is made from the Hamlet that loads the plugin, with
# scripts/make-native-sdk.sh of its checkout, or fetched for a release
# with scripts/fetch-native-sdk.sh. It holds that build's
# compiled engine and SDK, and the plugin links those very crates, so
# Hamlet's gate lets it through. A kit of another build is turned
# down by the gate, and one of another compiler by the compiler.
set -e
: "${HAMLET_SDK:?set HAMLET_SDK to a kit made by scripts/make-native-sdk.sh or fetched by scripts/fetch-native-sdk.sh}"
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
value() {
    sed -n "s/^$1 = \"\(.*\)\"/\1/p" "$kit/sdk.toml"
}
toolchain=$(value toolchain)
target=$(value target)
[ -n "$toolchain" ] && [ -n "$target" ] || {
    echo "$kit/sdk.toml names no toolchain or target: not a kit" >&2
    exit 1
}

set -- -L "dependency=$kit/deps"
while IFS= read -r line; do
    set -- "$@" --extern "${line%%=*}=$kit/${line#*=}"
done < "$kit/externs"

cd "$here"
built=${CARGO_TARGET_DIR:-$here/target}
profile=debug
[ -n "$release" ] && profile=release
# Cargo does not see the kit's files change, so a plugin built with
# another kit is built again.
stamp=$built/sdk-$profile.toml
if ! cmp -s "$kit/sdk.toml" "$stamp"; then
    cargo "+$toolchain" clean -p {{name}} --target "$target" $release
fi
cargo "+$toolchain" rustc --lib --target "$target" $release -- "$@"
cp "$kit/sdk.toml" "$stamp"
mkdir -p "$dest"
cp "$built/$target/$profile/{{library}}" "$dest/{{library}}"
cp "$here/manifest.toml" "$dest/manifest.toml"
echo "built into $dest"
echo "Hamlet asks before it loads this, and loads it when it starts."
