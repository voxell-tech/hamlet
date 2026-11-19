#!/usr/bin/env bash
# Keeps the release of a channel other than stable, where its copies
# find the manifest of its newest build (docs/releases.md#updates):
# made the first time as a pre-release, which GitHub never takes for
# the latest release, and its tag moved to `sha` each time.
#
# Usage: channel.sh <release> <sha> <title> <repository>
set -euo pipefail

release=$1
sha=$2
title=$3
repository=$4

if gh release view "$release" --repo "$repository" >/dev/null 2>&1; then
  gh api --method PATCH "repos/$repository/git/refs/tags/$release" \
    -f sha="$sha" -F force=true >/dev/null
else
  gh release create "$release" \
    --repo "$repository" \
    --target "$sha" \
    --title "$title" \
    --notes "" \
    --prerelease
fi
