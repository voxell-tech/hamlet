#!/bin/sh
# Builds the plugin and puts its manifest and library in a plugin
# folder: the one given, or $HAMLET_HOME/plugins/{{name}}.
#
#   ./build.sh [folder] [release]
#
# Build the way the Hamlet that loads it was built: a debug Hamlet
# loads a debug plugin, a release Hamlet a release one. The compiler
# must be the same too: build with the toolchain of the checkout.
set -e
dest=
profile=debug
for arg in "$@"; do
    case $arg in
        release) profile=release ;;
        /*) dest=$arg ;;
        *) dest="$PWD/$arg" ;;
    esac
done
if [ -z "$dest" ]; then
    : "${HAMLET_HOME:?give a folder, or set HAMLET_HOME to the home to install into}"
    dest="$HAMLET_HOME/plugins/{{name}}"
fi
here=$(cd "$(dirname "$0")" && pwd)
cd "{{hamlet}}"
# The whole workspace, not the plugin alone: the crates are built with
# the features everything in the workspace asks for, and Hamlet's own
# build asks for the same. A build of the plugin alone differs, and
# Hamlet turns it down.
if [ "$profile" = release ]; then
    cargo build --release --workspace
else
    cargo build --workspace
fi
mkdir -p "$dest"
cp "${CARGO_TARGET_DIR:-{{hamlet}}/target}/$profile/{{library}}" \
    "$dest/{{library}}"
cp "$here/manifest.toml" "$dest/manifest.toml"
echo "built into $dest"
echo "Hamlet asks before it loads this, and loads it when it starts."
