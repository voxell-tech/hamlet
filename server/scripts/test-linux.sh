#!/bin/sh
# Runs the server's tests on Linux, where tools run under bubblewrap, in an
# OrbStack machine with bubblewrap, git, curl, python3 and the Node version
# of .nvmrc. Arguments go to vitest.
set -eu
machine=${HAMLET_LINUX_MACHINE:-hamlet-sandbox}
repo=$(cd "$(dirname "$0")/../.." && pwd)
orbctl run -m "$machine" sh -c "
    set -eu
    mkdir -p ~/hamlet/assets
    rsync -a --delete --exclude node_modules '$repo/server/' ~/hamlet/server/
    cp '$repo/assets/charter.md' ~/hamlet/assets/
    cd ~/hamlet/server
    npm ci --silent --no-audit --no-fund
    npx vitest --run $*
"
