#!/bin/sh
# Starts a native plugin from its template: copies it into a folder and
# fills in the placeholders.
#
#   scripts/new-native-plugin.sh <name> [folder]
#
# <name> is the plugin's id: lowercase letters, digits and underscores,
# starting with a letter. A library has to be built with the very
# crates, features and settings of the Hamlet that loads it. Without a
# folder the plugin is made in native/<name> of this checkout, a member
# of its workspace, and built with the workspace. With one it is made
# there, a workspace of its own, and built against an SDK kit made by
# scripts/make-native-sdk.sh (the files of templates/native-plugin/
# outside replace their namesakes). The placeholders are:
#   {{name}}     the id, also the crate's name
#   {{title}}    the name in words, as Settings shows it
#   {{TITLE}}    the same in capitals, as a panel's title
#   {{library}}  the file name of the library on this system
#   {{hamlet}}   the folder of this checkout
set -e

usage() {
    echo "usage: $0 <name> [folder]" >&2
    exit 2
}

[ $# -ge 1 ] && [ $# -le 2 ] || usage
name=$1
root=$(cd "$(dirname "$0")/.." && pwd)
dest=$root/native/$name
case $2 in
    "") ;;
    /*) dest=$2 ;;
    *) dest="$PWD/$2" ;;
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

case $(uname -s) in
    Darwin) library="lib$name.dylib" ;;
    MINGW* | MSYS* | CYGWIN*) library="$name.dll" ;;
    *) library="lib$name.so" ;;
esac

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

cp -R "$root/templates/native-plugin" "$dest"
if [ -n "$2" ]; then
    cp -R "$dest/outside/." "$dest"
fi
rm -r "$dest/outside"
find "$dest" -type f | while read -r file; do
    sed \
        -e "s|{{name}}|$(esc "$name")|g" \
        -e "s|{{title}}|$(esc "$title")|g" \
        -e "s|{{TITLE}}|$(esc "$upper")|g" \
        -e "s|{{library}}|$(esc "$library")|g" \
        -e "s|{{hamlet}}|$(esc "$root")|g" \
        "$file" > "$file.new"
    cat "$file.new" > "$file"
    rm "$file.new"
done
chmod +x "$dest/build.sh"

echo "$dest is ready: native plugin $name"
here=$(cd "$dest" && pwd)
if [ -n "$2" ]; then
    echo "build:  HAMLET_SDK=<kit> $here/build.sh \"\$HAMLET_HOME/plugins/$name\""
else
    echo "build:  $here/build.sh \"\$HAMLET_HOME/plugins/$name\""
fi
echo "Hamlet asks the chief for full access before it loads it."
