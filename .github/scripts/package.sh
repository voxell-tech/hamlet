#!/usr/bin/env bash
# Packs the CLI, server, window, the app's launcher, pi and Node into
# dist/: the archive for every platform, and what a chief installs
# without a terminal: a disk image on macOS, an installer on Windows,
# a .deb and an .rpm on Linux.
# Run on the target platform so npm installs its native dependencies.
#
# Usage: package.sh <target> <version> [profile]
# `version` is a tag (v0.1.0) or a commit. `profile` is the folder of
# the build under target/<target>/, release unless given.
#
# On macOS, APPLE_SIGNING_IDENTITY signs with a Developer ID in the
# keychain, and APPLE_ID, APPLE_TEAM_ID and APPLE_APP_PASSWORD then
# have Apple notarize the disk image. Without them the app is signed
# for this machine only.
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

# The version packages compare: the tag's, or for a commit the
# workspace's with the commit after it, which sorts before the tag's.
cargo_version=$(sed -n '/^version = "/{s/^version = "\([^"]*\)".*/\1/p;q;}' Cargo.toml)
case $version in
  v*) package_version=${version#v} ;;
  *) package_version=$cargo_version~g$version ;;
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

required=$(tr -d '\r\n' < harnesses/pi/.nvmrc)
[[ $(node --version) == "v$required" ]] || {
  echo "Packaging requires Node $required (harnesses/pi/.nvmrc)" >&2
  exit 1
}

sha256() {
  if command -v sha256sum >/dev/null; then
    sha256sum "$1" | cut -d' ' -f1
  else
    shasum -a 256 "$1" | cut -d' ' -f1
  fi
}

# Node itself, as nodejs.org builds it, checked against its sums.
case $target in
  aarch64-apple-darwin) node_dist=node-v$required-darwin-arm64.tar.gz ;;
  x86_64-unknown-linux-gnu) node_dist=node-v$required-linux-x64.tar.xz ;;
  x86_64-pc-windows-msvc) node_dist=node-v$required-win-x64.zip ;;
esac
node_url=https://nodejs.org/dist/v$required
curl -fsSL --retry 3 -o "$scratch/$node_dist" "$node_url/$node_dist"
curl -fsSL --retry 3 -o "$scratch/SHASUMS256.txt" "$node_url/SHASUMS256.txt"
expected=$(awk -v file="$node_dist" '$2 == file { print $1 }' "$scratch/SHASUMS256.txt")
[[ -n $expected && $(sha256 "$scratch/$node_dist") == "$expected" ]] || {
  echo "$node_dist does not match nodejs.org's SHASUMS256.txt" >&2
  exit 1
}
node_dir=$scratch/${node_dist%.tar.*}
node_dir=${node_dir%.zip}
case $node_dist in
  *.zip) (cd "$scratch" && 7z x -bso0 "$node_dist") ;;
  *) tar -C "$scratch" -xf "$scratch/$node_dist" ;;
esac

if [[ $target == *-apple-darwin ]]; then
  app=$stage/Hamlet.app
  bin=$app/Contents/MacOS
  pi=$app/Contents/Resources/pi
else
  bin=$stage
  pi=$bin/pi
fi
mkdir -p "$bin" "$pi"
exe=
[[ $target == *-windows-* ]] && exe=.exe
for binary in hamlet hamlet_server hamlet_app hamlet_launch; do
  cp "$built/$binary$exe" "$bin/"
done
# The server runs pi with the Node beside it.
if [[ -n $exe ]]; then
  cp "$node_dir/node.exe" "$bin/"
else
  cp "$node_dir/bin/node" "$bin/"
fi
cp "$node_dir/LICENSE" "$stage/LICENSE-node"
cp -R harnesses/pi/src "$pi/"
cp harnesses/pi/{package.json,package-lock.json,.nvmrc} "$pi/"
# The Claude program is Anthropic's, under its own terms: pi offers Claude only
# where its optional packages are installed (ADR 0014).
npm ci --omit=dev --omit=optional --prefix "$pi"
cat > "$stage/README.txt" <<INFO
Hamlet $version

To install Hamlet without a terminal, use the installer for your system
instead of this archive: Hamlet.dmg on macOS, the Setup .exe on Windows,
the .deb or .rpm on Linux.

This archive holds the same programs. Keep its layout: the four binaries,
Node and pi/ side by side, with pi/ under Hamlet.app/Contents/Resources
on macOS. Node $required is included; HAMLET_NODE selects another.

hamlet_launch is what the installed app opens: it starts the server, or
reuses the one running, and opens the window, showing any failure in a
dialog. hamlet is the same from a terminal, and more: hamlet --help.

Linux: working bubblewrap (bwrap) is required; there is no insecure mode.
Windows: Windows has no sandbox yet, so the server starts only with
hamlet.exe --insecure, in a terminal. Agents' commands then run with this
account's full reach. Agents get PowerShell, and bash too where Git for
Windows is installed.

The default home is ~/.hamlet. For a disposable trial, set HAMLET_HOME to a new
scratch directory and HAMLET_VIEW to a scratch window directory. hamlet stop
stops the home's server; closing its window does not.

Remote workspaces need a separately run workspace worker.
INFO

case $target in
  *-apple-darwin)
    mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
    cp "$stage"/LICENSE-* "$app/Contents/Resources/"
    # The icon, at every size the system asks for.
    icons=$scratch/Hamlet.iconset
    mkdir -p "$icons"
    for size in 16 32 128 256 512; do
      sips -z $size $size brand/icon/hamlet-icon.png \
        --out "$icons/icon_${size}x${size}.png" >/dev/null
      sips -z $((size * 2)) $((size * 2)) brand/icon/hamlet-icon.png \
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
  <key>CFBundleExecutable</key><string>hamlet_launch</string>
  <key>CFBundleIconFile</key><string>Hamlet</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$package_version</string>
  <key>CFBundleVersion</key><string>$package_version</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
    if [[ -n ${APPLE_SIGNING_IDENTITY:-} ]]; then
      # A Developer ID signature with the hardened runtime, which
      # notarization asks of every program in the app, pi's native
      # modules among them. Node compiles JavaScript as it runs.
      cat > "$scratch/node.entitlements" <<ENTITLEMENTS
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>com.apple.security.cs.allow-jit</key><true/>
  <key>com.apple.security.cs.allow-unsigned-executable-memory</key><true/>
</dict>
</plist>
ENTITLEMENTS
      sign() {
        codesign --force --timestamp --options runtime \
          --sign "$APPLE_SIGNING_IDENTITY" "$@"
      }
      while IFS= read -r -d '' file; do
        if file -b "$file" | grep -q Mach-O; then
          sign "$file"
        fi
      done < <(find "$pi" -type f -print0)
      sign --entitlements "$scratch/node.entitlements" "$bin/node"
      for binary in hamlet hamlet_server hamlet_app; do
        sign "$bin/$binary"
      done
      sign "$app"
    else
      # Signed for this machine only. Without a developer's signature
      # the system asks before it opens the app the first time.
      for binary in hamlet hamlet_server hamlet_app; do
        codesign --force --sign - "$bin/$binary"
      done
      codesign --force --sign - "$app"
    fi
    codesign --verify --deep --strict "$app"
    ditto -c -k --keepParent "$stage" "$scratch/$name.zip"
    mv "$scratch/$name.zip" "dist/$name.zip"

    # The disk image: the app beside a link to Applications, to drag
    # it onto.
    image=$scratch/image
    mkdir -p "$image"
    ditto "$app" "$image/Hamlet.app"
    ln -s /Applications "$image/Applications"
    dmg=dist/Hamlet-$version.dmg
    hdiutil create -volname Hamlet -srcfolder "$image" -ov \
      -format UDZO "$dmg"
    if [[ -n ${APPLE_SIGNING_IDENTITY:-} ]]; then
      codesign --force --timestamp --sign "$APPLE_SIGNING_IDENTITY" "$dmg"
      if [[ -n ${APPLE_ID:-} ]]; then
        xcrun notarytool submit "$dmg" --wait \
          --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" \
          --password "$APPLE_APP_PASSWORD"
        xcrun stapler staple "$dmg"
      fi
    fi
    ;;
  *-windows-*)
    (cd "$scratch" && 7z a -bso0 "$name.zip" "$name")
    mv "$scratch/$name.zip" "dist/$name.zip"
    cp brand/hamlet-icon.ico "$stage/hamlet.ico"
    iscc=$(command -v iscc || echo "/c/Program Files (x86)/Inno Setup 6/ISCC.exe")
    # Git's bash would turn each /option into a path; ISCC then reads
    # it as a second script.
    MSYS2_ARG_CONV_EXCL='*' "$iscc" /Q \
      "/DVersion=$package_version" \
      "/DStage=$(cygpath -w "$stage")" \
      "/DIcon=$(cygpath -w "$PWD/brand/hamlet-icon.ico")" \
      "/O$(cygpath -w "$PWD/dist")" \
      "/FHamlet-$version-Setup" \
      .github/scripts/hamlet.iss
    ;;
  *)
    tar -C "$scratch" -czf "$scratch/$name.tar.gz" "$name"
    mv "$scratch/$name.tar.gz" "dist/$name.tar.gz"

    # The installed tree the .deb and the .rpm share: the programs in
    # /opt/hamlet, the command on the path, and the menu's entry.
    tree=$scratch/tree
    mkdir -p "$tree/opt" "$tree/usr/bin" "$tree/usr/share/applications" \
      "$tree/usr/share/pixmaps"
    cp -al "$stage" "$tree/opt/hamlet"
    # Installed as root's: writable by root alone, whatever the umask.
    chmod -R u=rwX,go=rX "$tree"
    ln -s /opt/hamlet/hamlet "$tree/usr/bin/hamlet"
    cp brand/hamlet-icon.png "$tree/usr/share/pixmaps/hamlet.png"
    cat > "$tree/usr/share/applications/hamlet.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Hamlet
Comment=Lead a hamlet of AI agents
Exec=/opt/hamlet/hamlet_launch
Icon=hamlet
Terminal=false
Categories=Development;
StartupWMClass=hamlet
DESKTOP
    summary="Lead a hamlet of AI agents"
    about="Hamlet is an agent orchestrator. You lead a hamlet of AI agents and
watch them work in a small 3D world where each agent has a house and a
robot."

    deb=$scratch/deb
    cp -al "$tree" "$deb"
    mkdir -p "$deb/DEBIAN"
    cat > "$deb/DEBIAN/control" <<CONTROL
Package: hamlet
Version: $package_version
Section: devel
Priority: optional
Architecture: amd64
Maintainer: Voxell <https://github.com/voxell-tech/hamlet>
Installed-Size: $(du -sk "$tree" | cut -f1)
Depends: bubblewrap, libc6 (>= 2.35), libasound2t64 | libasound2, libudev1, libwayland-client0, libxkbcommon0, libx11-6, libxcursor1, libxi6, libxrandr2, libvulkan1
Recommends: zenity, mesa-vulkan-drivers
Homepage: https://github.com/voxell-tech/hamlet
Description: $summary
$(sed 's/^/ /' <<< "$about")
CONTROL
    dpkg-deb --root-owner-group -Zxz --build "$deb" \
      "dist/hamlet_${package_version}_amd64.deb"

    cat > "$scratch/hamlet.spec" <<SPEC
Name: hamlet
Version: ${package_version//-/_}
Release: 1
Summary: $summary
License: MIT OR Apache-2.0
URL: https://github.com/voxell-tech/hamlet
# pi's native modules would add what they link against; these are
# what the window and the sandbox need.
AutoReqProv: no
Requires: bubblewrap, alsa-lib, systemd-libs, libwayland-client, libxkbcommon, libX11, libXcursor, libXi, libXrandr, vulkan-loader
Recommends: zenity, mesa-vulkan-drivers
%global debug_package %{nil}
%global __os_install_post %{nil}
%global __arch_install_post %{nil}

%description
$about

%install
mkdir -p %{buildroot}
cp -a $tree/opt $tree/usr %{buildroot}/

%files
/opt/hamlet
/usr/bin/hamlet
/usr/share/applications/hamlet.desktop
/usr/share/pixmaps/hamlet.png
SPEC
    rpmbuild -bb --quiet \
      --define "_topdir $scratch/rpm" \
      --define "_rpmdir $PWD/dist" \
      --define "_build_name_fmt %%{NAME}-%%{VERSION}-%%{RELEASE}.%%{ARCH}.rpm" \
      "$scratch/hamlet.spec"
    ;;
esac

ls -l dist
