#!/usr/bin/env bash
# Packs the CLI, server, window and pi, or the Windows window, into dist/.
# Run on the target platform so npm installs its native dependencies.
#
# Usage: package.sh <target> <version> [profile]
# `version` is a tag (v0.1.0) or a commit. `profile` is the folder of
# the build under target/<target>/, release unless given.
set -euo pipefail

target=$1
version=$2
profile=${3:-release}

case $target in
  aarch64-apple-darwin|x86_64-unknown-linux-gnu|x86_64-pc-windows-msvc) ;;
  *) echo "Unsupported release target: $target" >&2; exit 1 ;;
esac
[[ $version =~ ^[a-zA-Z0-9._-]+$ ]] || { echo "Invalid version" >&2; exit 1; }

# A commit is told by its first seven letters.
case $version in
  v*) ;;
  *) version=${version:0:7} ;;
esac

built=${CARGO_TARGET_DIR:-target}/$target/$profile
name=hamlet-$version-$target
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
scratch=$(cd "$scratch" && pwd -P)
stage=$scratch/$name
mkdir -p "$stage"
mkdir -p dist
cp LICENSE-MIT LICENSE-APACHE "$stage/"

if [[ $target == x86_64-pc-windows-msvc ]]; then
  cp "$built/hamlet-app.exe" "$stage/"
  cat > "$stage/README.txt" <<INFO
Hamlet $version - Windows window only

This archive contains a protocol client, not a local agent server.
No Node installation is needed on the window's machine. Run a Hamlet server
on Linux or macOS and connect through an SSH tunnel. Set HAMLET_SERVER to its
WebSocket URL and HAMLET_TOKEN to its connection token before launching
hamlet-app.exe. Keep the token private. Set HAMLET_VIEW to a scratch folder
when trying the window without changing your saved layout.

Windows has no supported Hamlet server sandbox. Closing this window does not
stop agents on the remote server. Paths and pictures in chat resolve on this
Windows machine, not the server; remote files are not downloaded.
INFO
  archive=$scratch/$name.zip
  (cd "$scratch" && 7z a -bso0 "$archive" "$name")
  mv "$archive" "dist/$name.zip"
  ls -l "dist/$name.zip"
  exit 0
fi

required=$(tr -d '\r\n' < harnesses/pi/.nvmrc)
[[ $(node --version) == "v$required" ]] || {
  echo "Packaging requires Node $required (harnesses/pi/.nvmrc)" >&2
  exit 1
}

if [[ $target == *-apple-darwin ]]; then
  bin=$stage/Hamlet.app/Contents/MacOS
  pi=$stage/Hamlet.app/Contents/Resources/pi
else
  bin=$stage
  pi=$bin/pi
fi
mkdir -p "$bin" "$pi"
for binary in hamlet hamlet-server hamlet-app; do
  cp "$built/$binary" "$bin/"
done
cp -R harnesses/pi/src "$pi/"
cp harnesses/pi/{package.json,package-lock.json,link.schema.json,.nvmrc} "$pi/"
npm ci --omit=dev --prefix "$pi"
cat > "$stage/README.txt" <<INFO
Hamlet $version

Install Node $required separately; its executable must be on PATH, or set
HAMLET_NODE to its absolute path. Node itself is not bundled. Keep all three
binaries and pi/ in the archive's layout. On macOS pi/ lives under
Hamlet.app/Contents/Resources. No source checkout or npm install is needed.

Linux: run ./hamlet. Working bubblewrap (bwrap) is required; no insecure mode.
macOS: open Hamlet.app, or run Hamlet.app/Contents/MacOS/hamlet in a terminal.
Finder may not inherit your shell's PATH; launch from a terminal with HAMLET_NODE
if Node is not found. The app is ad-hoc signed, not notarized.

The default home is ~/.hamlet. For a disposable trial, set HAMLET_HOME to a new
scratch directory and HAMLET_VIEW to a scratch window directory. hamlet stop
stops the home's server; closing its window does not.

Windows has no server. Remote workspaces need a separately configured manager.
INFO

case $target in
  *-apple-darwin)
    app=$stage/Hamlet.app
    mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
    # The icon, at every size the system asks for.
    icons=$scratch/Hamlet.iconset
    mkdir -p "$icons"
    for size in 16 32 128 256 512; do
      sips -z $size $size brand/hamlet-icon.png \
        --out "$icons/icon_${size}x${size}.png" >/dev/null
      sips -z $((size * 2)) $((size * 2)) brand/hamlet-icon.png \
        --out "$icons/icon_${size}x${size}@2x.png" >/dev/null
    done
    iconutil -c icns "$icons" -o "$app/Contents/Resources/Hamlet.icns"

    cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Hamlet</string>
  <key>CFBundleDisplayName</key><string>Hamlet</string>
  <key>CFBundleIdentifier</key><string>gg.hamlet.app</string>
  <key>CFBundleExecutable</key><string>hamlet</string>
  <key>CFBundleIconFile</key><string>Hamlet</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${version#v}</string>
  <key>CFBundleVersion</key><string>${version#v}</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
    # Signed for this machine only. Without a developer's signature
    # the system asks before it opens the app the first time.
    codesign --force --sign - "$bin/hamlet-server"
    codesign --force --sign - "$bin/hamlet-app"
    codesign --force --sign - "$app"
    codesign --verify --deep --strict "$app"
    ditto -c -k --keepParent "$stage" "$scratch/$name.zip"
    mv "$scratch/$name.zip" "dist/$name.zip"
    ;;
  *)
    tar -C "$scratch" -czf "$scratch/$name.tar.gz" "$name"
    mv "$scratch/$name.tar.gz" "dist/$name.tar.gz"
    ;;
esac

ls -l dist
