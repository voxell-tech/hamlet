#!/bin/sh
# Runs the server's tests on Linux, where tools run under bubblewrap, in an
# OrbStack machine with bubblewrap, git, curl, python3 and the Node version
# of .nvmrc. Arguments go to vitest.
set -eu
machine=${HAMLET_LINUX_MACHINE:-hamlet-sandbox}
repo=$(cd "$(dirname "$0")/../.." && pwd)
# One copy per checkout, so worktrees can test at the same time.
copy="hamlet-$(printf %s "$repo" | shasum | cut -c1-12)"
orbctl run -m "$machine" sh -c "
    set -eu
    mkdir -p ~/$copy/assets
    rsync -a --delete --exclude node_modules '$repo/server/' ~/$copy/server/
    rsync -a '$repo/assets/' ~/$copy/assets/
    cd ~/$copy/server
    npm ci --silent --no-audit --no-fund
    npx vitest --run $*
"
