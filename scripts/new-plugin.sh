#!/bin/sh
# Starts a plugin from a template: copies it into a folder and fills in
# the placeholders.
#
#   scripts/new-plugin.sh <wasm|data> <name> [folder]
#
# <name> is the plugin's id: lowercase letters, digits and underscores,
# starting with a letter. The folder is <name> in the current folder
# unless one is given. The placeholders are:
#   {{name}}       the id, also the crate's name
#   {{title}}      the name in words, as Settings shows it
#   {{TITLE}}      the same in capitals, as a panel's title
#   {{interface}}  the version of the interface this Hamlet speaks
#   {{hamlet}}     the folder of this checkout, or $HAMLET_SRC where that
#                  is set to another copy of `crates/hamlet_guest` and
#                  `crates/hamlet_wasm/wit` (a plugin of code only)
set -e

usage() {
    echo "usage: $0 <wasm|data> <name> [folder]" >&2
    exit 2
}

[ $# -ge 2 ] && [ $# -le 3 ] || usage
kind=$1
name=$2
dest=${3:-$name}

case $kind in
    wasm) template=plugin ;;
    data) template=data-plugin ;;
    *) usage ;;
esac

lower=abcdefghijklmnopqrstuvwxyz
case $name in
    [$lower]*) ;;
    *) echo "the name must start with a lowercase letter" >&2; exit 2 ;;
esac
case $name in
    *[!${lower}0123456789_]*)
        echo "the name may hold lowercase letters, digits and _" >&2
        exit 2
        ;;
esac

if [ -e "$dest" ]; then
    echo "$dest exists" >&2
    exit 1
fi

root=$(cd "$(dirname "$0")/.." && pwd)
interface=$(sed -n 's/^pub const VERSION: &str = "\(.*\)";$/\1/p' \
    "$root/crates/hamlet_wasm/src/interface.rs")
if [ -z "$interface" ]; then
    echo "no interface version in $root" >&2
    exit 1
fi
title=$(echo "$name" | tr '_' ' ' | awk '{
    for (i = 1; i <= NF; i++)
        $i = toupper(substr($i, 1, 1)) substr($i, 2)
    print
}')
upper=$(echo "$title" | tr '[:lower:]' '[:upper:]')

# A value as sed reads it in a replacement.
esc() {
    printf '%s' "$1" | sed 's/[\\&|]/\\&/g'
}

cp -R "$root/templates/$template" "$dest"
find "$dest" -type f | while read -r file; do
    sed \
        -e "s|{{name}}|$(esc "$name")|g" \
        -e "s|{{title}}|$(esc "$title")|g" \
        -e "s|{{TITLE}}|$(esc "$upper")|g" \
        -e "s|{{interface}}|$(esc "$interface")|g" \
        -e "s|{{hamlet}}|$(esc "${HAMLET_SRC:-$root}")|g" \
        "$file" > "$file.new"
    cat "$file.new" > "$file"
    rm "$file.new"
done

echo "$dest is ready: $kind plugin $name, interface $interface"
here=$(cd "$dest" && pwd)
case $kind in
    wasm)
        echo "build:  $here/build.sh $here/build/$name"
        folder="$here/build/$name"
        ;;
    data)
        folder="$here"
        ;;
esac
echo "check:  cd $root && cargo run --example plugin_check \\"
echo "          --features wasm -- $folder --script $here/steps.toml"
