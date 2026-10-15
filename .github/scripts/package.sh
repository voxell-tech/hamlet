#!/usr/bin/env bash
# Packs a release build of Hamlet into dist/, as the release workflow
# publishes it: an app in a zip for macOS, a zip for Windows and a
# tarball for Linux.
#
# Usage: package.sh <target> <version> [profile]
# `version` is a tag (v0.1.0) or a commit. `profile` is the folder of
# the build under target/<target>/, release unless given.
set -euo pipefail

target=$1
version=$2
profile=${3:-release}

# A commit is told by its first seven letters.
case $version in
  v*) ;;
  *) version=${version:0:7} ;;
esac

built=target/$target/$profile
name=hamlet-$version-$target
stage=dist/$name
rm -rf dist
mkdir -p "$stage"

case $target in
  *-apple-darwin)
    app=$stage/Hamlet.app
    mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
    cp "$built/hamlet-app" "$app/Contents/MacOS/hamlet-app"

    # The icon, at every size the system asks for.
    icons=$(mktemp -d)/Hamlet.iconset
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
  <key>CFBundleExecutable</key><string>hamlet-app</string>
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
    codesign --force --deep --sign - "$app"
    ditto -c -k --keepParent "$app" "dist/$name.zip"
    ;;
  *-pc-windows-*)
    cp "$built/hamlet-app.exe" "$stage/"
    (cd dist && 7z a -bso0 "$name.zip" "$name")
    ;;
  *)
    cp "$built/hamlet-app" "$stage/"
    tar -C dist -czf "dist/$name.tar.gz" "$name"
    ;;
esac

rm -rf "$stage"
ls -l dist
