#!/bin/sh
# Builds the sample native plugin in native/ and puts it in a plugin
# folder of a home, ready for Hamlet to ask about.
#
#   scripts/build-native.sh <home> [release]
#
# The home is a folder of your own, not the one you use: a native
# plugin runs as Hamlet. Start Hamlet with HAMLET_HOME set to it.
set -e
[ $# -ge 1 ] && [ $# -le 2 ] || {
    echo "usage: $0 <home> [release]" >&2
    exit 2
}
home=$1
profile=debug
[ "$2" = release ] && profile=release

root=$(cd "$(dirname "$0")/.." && pwd)
case $(uname -s) in
    Darwin) library=libnative_spark.dylib ;;
    MINGW* | MSYS* | CYGWIN*) library=native_spark.dll ;;
    *) library=libnative_spark.so ;;
esac

cd "$root"
# The whole workspace, as the plugin template's build.sh says why.
if [ "$profile" = release ]; then
    cargo build --release --workspace
else
    cargo build --workspace
fi
dest="$home/plugins/spark"
mkdir -p "$dest"
cp "target/$profile/$library" "$dest/$library"
printf 'id = "spark"\nname = "Spark"\nversion = "0.1.0"\nkind = "native"\nlibrary = "%s"\n' \
    "$library" > "$dest/manifest.toml"
echo "built into $dest"
