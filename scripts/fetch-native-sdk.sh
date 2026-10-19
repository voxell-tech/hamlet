#!/bin/sh
# Fetches the SDK kit of a release of Hamlet and unpacks it into a
# folder. A native plugin built with it loads in that release's
# Hamlet for that target, and in no other.
#
#   scripts/fetch-native-sdk.sh <version> <target> <kit>
#
# <version> is the release's tag (v0.1.0, or 0.1.0) and <target> the
# triple of the Hamlet that loads the plugin: aarch64-apple-darwin,
# x86_64-pc-windows-msvc or x86_64-unknown-linux-gnu. The kit is
# downloaded with gh when it is signed in, else with curl.
# HAMLET_REPO names another repository (owner/name).
set -e

[ $# -eq 3 ] || {
    echo "usage: $0 <version> <target> <kit>" >&2
    exit 2
}
case $1 in
    v*) tag=$1 ;;
    *) tag=v$1 ;;
esac
target=$2
kit=$3
case $kit in
    /*) ;;
    *) kit="$PWD/$kit" ;;
esac
if [ -e "$kit" ]; then
    echo "$kit exists" >&2
    exit 1
fi
repo=${HAMLET_REPO:-voxell-tech/hamlet}
name=hamlet-sdk-$tag-$target
archive=$name.tar.gz

parent=$(dirname "$kit")
mkdir -p "$parent"
work=$(mktemp -d "$parent/.fetch.XXXXXX")
trap 'rm -rf "$work"' EXIT

if command -v gh >/dev/null 2>&1 && gh auth status >/dev/null 2>&1; then
    gh release download "$tag" --repo "$repo" --pattern "$archive" \
        --dir "$work"
else
    curl --fail --location --proto '=https' --output "$work/$archive" \
        "https://github.com/$repo/releases/download/$tag/$archive"
fi
tar -xzf "$work/$archive" -C "$work"
if ! grep -q "^target = \"$target\"$" "$work/$name/sdk.toml" 2>/dev/null
then
    echo "$archive holds no kit for $target" >&2
    exit 1
fi
mv "$work/$name" "$kit"
echo "fetched $kit:"
cat "$kit/sdk.toml"
echo "build a plugin with it: HAMLET_SDK=$kit ./build.sh"
