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
#
# The kit holds:
#   deps/     the libraries of the engine, the SDK and the contract,
#             and of everything they depend on
#   externs   the crates a plugin names, and their files
#   sdk.toml  the toolchain, the target and what the gate compares,
#             as the hamlet of the build reports them
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
# With a target, the libraries of the build scripts and procedural
# macros are built apart from the target's, which are under it.
ours=/
if [ -n "$target" ]; then
    ours=/$target/
else
    target=$(rustc -vV | sed -n 's/^host: //p')
fi

options=$*
raw=$(mktemp)
messages=$(mktemp)
tree=$(mktemp)
trap 'rm -f "$raw" "$messages" "$tree"' EXIT
cargo build "$@" --message-format=json-render-diagnostics > "$raw"
# A path on Windows is written with `\\`.
sed 's/\\\\/\//g' "$raw" > "$messages"

# The packages of the same build, as cargo tree takes them: the
# options that pick packages, features and the target.
count=$#
take=
for arg in "$@"; do
    if [ "$take" = keep ]; then
        set -- "$@" "$arg"
    fi
    if [ -n "$take" ]; then
        take=
        continue
    fi
    case $arg in
        -p | --package | --exclude | -F | --features | --target | \
            --manifest-path | --config | -Z)
            set -- "$@" "$arg"
            take=keep
            ;;
        --profile | -j | --jobs | --target-dir | --color | \
            --message-format | --bin | --example | --test | --bench | \
            --artifact-dir)
            take=drop
            ;;
        --package=* | --exclude=* | --features=* | --target=* | \
            --manifest-path=* | --config=* | --workspace | --all | \
            --all-features | --no-default-features | --locked | \
            --frozen | --offline)
            set -- "$@" "$arg"
            ;;
    esac
done
shift "$count"
cargo tree "$@" -e normal --prefix depth -f '{p}' > "$tree"

mkdir -p "$kit/deps"
# The libraries a plugin links: the engine, the SDK, the contract and
# what they depend on, and the procedural macros among them, which the
# engine's derives expand with. What a procedural macro depends on is
# inside its own library. A member of the workspace is named by its
# copy outside deps/ and its .rmeta inside, so the .rlib beside the
# .rmeta is taken.
awk '
    FNR == NR {
        depth = $1
        sub(/[^0-9].*/, "", depth)
        depth += 0
        name = $1
        sub(/^[0-9]+/, "", name)
        key = name " " substr($2, 2)
        above[depth] = key
        if (depth > 0) edges[above[depth - 1]] = edges[above[depth - 1]] " " key
        if (index($0, "(proc-macro)")) macro[key] = 1
        if (name == "bevy" || name == "hamlet_sdk" || name == "hamlet_native") wanted[key] = 1
        next
    }
    /"reason":"compiler-artifact"/ {
        id = $0
        sub(/.*"package_id":"/, "", id)
        sub(/".*/, "", id)
        split(id, parts, "#")
        if (index(parts[2], "@")) {
            split(parts[2], named, "@")
            key = named[1] " " named[2]
        } else {
            n = split(parts[1], path, "/")
            key = path[n] " " parts[2]
        }
        list = $0
        sub(/.*"filenames":\[/, "", list)
        sub(/\].*/, "", list)
        files[key] = files[key] list ","
    }
    END {
        for (key in wanted) queue[++last] = key
        for (at = 1; at <= last; at++) {
            key = queue[at]
            if (macro[key]) continue
            n = split(edges[key], children, " ")
            for (i = 1; i + 1 <= n; i += 2) {
                child = children[i] " " children[i + 1]
                if (!(child in wanted)) {
                    wanted[child] = 1
                    queue[++last] = child
                }
            }
        }
        for (key in wanted) print files[key]
    }
' "$tree" "$messages" |
    tr ',' '\n' |
    tr -d '"' |
    grep '/deps/' |
    sed 's/\.rmeta$/.rlib/' |
    grep -E '\.(rlib|dylib|so|dll)$' |
    awk -v ours="$ours" '!/\.rlib$/ || index($0, ours)' |
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

hamlet=$(grep -o '"executable":"[^"]*"' "$messages" |
    cut -d'"' -f4 |
    grep -E '/hamlet(\.exe)?$' |
    head -n 1)
[ -n "$hamlet" ] || {
    echo "the build made no hamlet: build the package hamlet" >&2
    exit 1
}
{
    echo "toolchain = \"$(sed -n 's/^channel = "\(.*\)"/\1/p' rust-toolchain.toml)\""
    echo "target = \"$target\""
    "$hamlet" --native-abi
} > "$kit/sdk.toml" || {
    echo "$hamlet --native-abi failed: is it of this system?" >&2
    exit 1
}
echo "made $kit ($(du -sh "$kit" | cut -f1)), for the Hamlet of:"
echo "  cargo build $options"
echo "build a plugin with it: HAMLET_SDK=$kit ./build.sh"
