#!/bin/zsh
# Builds the plugin again with one thing changed from the app's build, and
# reports whether its SDK still matches the shipped one and whether it loads.
#
#   mismatch.sh [variant...]   (after package.sh)
set -u
spike=$(cd "$(dirname "$0")" && pwd)
cd "$spike"
runs=${TMPDIR:-/tmp}/hamlet-native-spike
mkdir -p $runs
shipped=$(nm -gU dist/libhamlet_sdk.dylib | awk '/rust_metadata_hamlet_sdk/ {print $3}')
nm -gU dist/libhamlet_sdk.dylib dist/libstd-*.dylib | awk 'NF == 3 {print $3}' | sort -u > $runs.exports

variant() {
    local name=$1 b=build_$1 p=plugin
    rm -rf $b
    case $name in
    feature)
        p=$runs/plugin_feature; rm -rf $p; cp -R plugin $p
        sed -i '' 's/^bevy = .*/bevy = { version = "=0.20.1", default-features = false, features = ["jpeg"] }/' $p/Cargo.toml ;;
    depfeature)
        p=$runs/plugin_depfeature; rm -rf $p; cp -R plugin $p
        sed -i '' 's/^\[dependencies\]$/[dependencies]\nsmallvec = { version = "1", features = ["write"] }/' $p/Cargo.toml ;;
    esac
    PREPARE_ONLY=1 ./build_plugin.sh $p $b
    case $name in
    lock) (cd $b && cargo update -q) ;;
    cpu) sed -i '' 's/^rustflags = .*/rustflags = ["-C", "prefer-dynamic", "-C", "target-cpu=native"]/' $b/.cargo/config.toml ;;
    static) sed -i '' 's/^rustflags = .*/rustflags = []/' $b/.cargo/config.toml ;;
    esac
    case $name in
    rustc) export RUSTUP_TOOLCHAIN=1.99.1 ;;
    opt) export CARGO_PROFILE_RELEASE_OPT_LEVEL=2 ;;
    debug) export CARGO_PROFILE_RELEASE_DEBUG=line-tables-only ;;
    esac
    local start=$SECONDS out=target/release target=()
    if [ $name = target ]; then
        target=(--target aarch64-apple-darwin)
        out=target/aarch64-apple-darwin/release
    fi
    (cd $b && cargo build --release -p pulse $target) > $b.log 2>&1
    local built=$? took=$((SECONDS - start))
    [ $name = target ] && cp $b/$out/lib*.dylib $b/target/release/
    unset RUSTUP_TOOLCHAIN CARGO_PROFILE_RELEASE_OPT_LEVEL CARGO_PROFILE_RELEASE_DEBUG
    echo "== $name: build exit $built in ${took}s"
    [ $built -eq 0 ] || { grep -E "^error" $b.log | sort | uniq -c | head -5; return; }
    local own=$(nm -gU $b/target/release/libhamlet_sdk.dylib | awk '/rust_metadata_hamlet_sdk/ {print $3}')
    echo "   sdk hash: $([ "$own" = "$shipped" ] && echo same || echo "differs ($own)")"
    nm -u $b/target/release/libpulse.dylib | sort -u > $runs.needs
    echo "   symbols the plugin needs that the release lacks: $(comm -23 $runs.needs $runs.exports | grep -c '^__R')"
    otool -L $b/target/release/libpulse.dylib | awk 'NR > 1 && /hamlet_sdk|libstd/ {print "   links " $1}'
    local run=$runs/run_$name; rm -rf $run; mkdir -p $run/plugins
    cp $b/target/release/libpulse.dylib $run/plugins/
    (cd $run && DYLD_LIBRARY_PATH=/nonexistent HAMLET_SPIKE_PLUGINS=$run/plugins HAMLET_SPIKE_FRAMES=30 \
        "$spike/dist/app" > $run/out.log 2>&1)
    echo "   app exit $?"
    sed 's/\x1b\[[0-9;]*m//g' $run/out.log | grep -E "^(loaded|failed)|pulse:|panicked|^thread" \
        | sed -e 's#/[^ ]*/##g' -e 's/dlopen([^)]*): //' -e 's/^/   /'
}

(( $# )) || set -- rustc feature depfeature opt debug lock cpu static target
for name in "$@"; do
    variant $name
done
