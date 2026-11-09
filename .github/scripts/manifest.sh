#!/usr/bin/env bash
# Writes dist/latest.json, the manifest an installed Hamlet reads to
# find an update, and signs it into dist/latest.json.sig
# (docs/releases.md#updates). Run after package.sh has made every
# platform's files in dist/.
#
# Usage: manifest.sh <tag> <repository>
# HAMLET_UPDATE_SIGNING_KEY: the Ed25519 private key, in PEM, whose
# public half release builds are given as HAMLET_UPDATE_KEY.
set -euo pipefail

tag=$1
repository=$2
version=${tag#v}
base=https://github.com/$repository/releases/download/$tag

# A file of the release: where it is, its SHA-256 and its length.
asset() {
  [[ -f $1 ]] || { echo "The release has no $1" >&2; exit 1; }
  jq -n \
    --arg url "$base/$(basename "$1")" \
    --arg sha256 "$(sha256sum "$1" | cut -d' ' -f1)" \
    --argjson size "$(stat -c %s "$1")" \
    '{url: $url, sha256: $sha256, size: $size}'
}

notes=$(gh release view "$tag" --repo "$repository" --json body --jq .body)
jq -n \
  --arg version "$version" \
  --arg notes "$notes" \
  --arg page "https://github.com/$repository/releases/tag/$tag" \
  --argjson macos "$(asset "dist/hamlet-$tag-aarch64-apple-darwin.zip")" \
  --argjson windows "$(asset "dist/Hamlet-$tag-Setup.exe")" \
  --argjson deb "$(asset "dist/hamlet_${version}_amd64.deb")" \
  --argjson rpm "$(asset "dist/hamlet-${version//-/_}-1.x86_64.rpm")" \
  '{version: $version, notes: $notes, page: $page,
    files: {macos: $macos, windows: $windows, deb: $deb, rpm: $rpm}}' \
  > dist/latest.json

key=$(mktemp)
trap 'rm -f "$key"' EXIT
printf '%s\n' "$HAMLET_UPDATE_SIGNING_KEY" > "$key"
openssl pkeyutl -sign -rawin -inkey "$key" -in dist/latest.json \
  | base64 -w0 > dist/latest.json.sig
