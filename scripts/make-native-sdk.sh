#!/bin/sh
# Makes the SDK kit a native plugin outside this checkout is built
# with: the compiled crates of one build of Hamlet, so that a plugin
# links the very engine and SDK that build has. A plugin built with
# the kit passes the gate of the Hamlet of that build, and of no other.
#
#   scripts/make-native-sdk.sh <kit> [cargo build options]
#
# The options are those the Hamlet that loads the plugins is built
# with: none for `cargo build`, `--release` for `cargo build
# --release`. The build is run with them (quick when it is done
# already) to learn which files it made. Make the kit again whenever
# that Hamlet is built again.
set -e

[ $# -ge 1 ] || {
    echo "usage: $0 <kit> [cargo build options]" >&2
    exit 2
}
kit=$1
shift
case $kit in
    /*) ;;
    *) kit="$PWD/$kit" ;;
esac
if [ -e "$kit" ]; then
    echo "$kit exists" >&2
    exit 1
fi
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

target=
take=
for arg in "$@"; do
    [ "$take" = target ] && target=$arg
    take=
    case $arg in
        --target) take=target ;;
        --target=*) target=${arg#--target=} ;;
    esac
done
[ -n "$target" ] || target=$(rustc -vV | sed -n 's/^host: //p')

messages=$(mktemp)
trap 'rm -f "$messages"' EXIT
cargo build "$@" --message-format=json-render-diagnostics > "$messages"

mkdir -p "$kit/deps"
# Every library the build compiled, and the procedural macros the
# engine's derives expand with. The compiler picks a crate's
# dependencies out of them by their hash. A member of the workspace is
# named by its copy outside deps/ and its .rmeta inside, so the .rlib
# beside the .rmeta is taken.
grep '"reason":"compiler-artifact"' "$messages" |
    grep -o '"filenames":\[[^]]*\]' |
    grep -o '"[^"]*/deps/[^"]*"' |
    tr -d '"' |
    sed 's/\.rmeta$/.rlib/' |
    grep -E '\.(rlib|dylib|so|dll)$' |
    sort -u |
    while read -r file; do
        cp "$file" "$kit/deps/"
    done

# The crates a plugin names: the engine, the SDK and the contract.
# The engine's crates are named one by one because its derives write
# paths such as `bevy_ecs::`.
for file in "$kit"/deps/lib*.rlib; do
    base=${file##*/}
    crate=${base#lib}
    crate=${crate%-*}
    case $crate in
        bevy | bevy_* | hamlet_sdk | hamlet_native) ;;
        *) continue ;;
    esac
    if grep -q "^$crate=" "$kit/externs" 2>/dev/null; then
        echo "$crate was built twice; give the options of one build" >&2
        exit 1
    fi
    echo "$crate=deps/$base" >> "$kit/externs"
done
for crate in bevy hamlet_sdk hamlet_native; do
    grep -q "^$crate=" "$kit/externs" || {
        echo "the build made no $crate" >&2
        exit 1
    }
done

sed -n 's/^channel = "\(.*\)"/\1/p' rust-toolchain.toml > "$kit/toolchain"
echo "$target" > "$kit/target"
echo "made $kit ($(du -sh "$kit" | cut -f1)), for the Hamlet of:"
echo "  cargo build $*"
echo "build a plugin with it: HAMLET_SDK=$kit ./build.sh"
