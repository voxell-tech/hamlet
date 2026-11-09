#!/usr/bin/env bash
# Renders ui_gallery scenes without a window, one PNG per scene.
#
#   shoot.sh <out folder> <light|dark> <scene>...
#
# Writes <out folder>/<scene>.png. Scene names are the gallery's own (see
# SHOTS in crates/hamlet_app/examples/ui_gallery/main.rs). Set WORLD to a
# PNG to show it behind the panels. Nothing is opened on screen.
set -euo pipefail

if [ "$#" -lt 3 ]; then
    echo "usage: shoot.sh <out folder> <light|dark> <scene>..." >&2
    exit 2
fi
out="$1"
mode="$2"
shift 2
case "$mode" in light | dark) ;; *)
    echo "mode must be light or dark" >&2
    exit 2
    ;;
esac

here="$(cd "$(dirname "$0")" && pwd)"
root="$(git -C "$here" rev-parse --show-toplevel)"
cache="${CLIMB_CACHE:-${TMPDIR:-/tmp}/hamlet-climb-tools}"
mkdir -p "$out" "$cache"

# The sampler, compiled once into a cache folder outside the repository.
if [ ! -x "$cache/sample" ] || [ "$here/sample.swift" -nt "$cache/sample" ]; then
    swiftc -O "$here/sample.swift" -o "$cache/sample"
fi

(cd "$root" && cargo build --example ui_gallery -p hamlet_app)

for scene in "$@"; do
    args=(--scene "$scene" --out "$out")
    [ -n "${WORLD:-}" ] && args+=(--world "$WORLD")
    GALLERY_THEME="Hamlet:$mode" "$root/target/debug/examples/ui_gallery" "${args[@]}" >"$out/$scene.log" 2>&1
    echo "$out/$scene.png"
done
