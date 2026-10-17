#!/bin/sh
# Runs the server's tests on Linux, where tools run under bubblewrap, in an
# OrbStack machine with Rust, bubblewrap, git, curl, python3 and the Node version
# of .nvmrc. Arguments go to vitest.
set -eu
machine=${HAMLET_LINUX_MACHINE:-hamlet-sandbox}
repo=$(cd "$(dirname "$0")/../.." && pwd)
# One copy per checkout, so worktrees can test at the same time.
copy="hamlet-$(printf %s "$repo" | shasum | cut -c1-12)"
orbctl run -m "$machine" sh -c "
    set -eu
    mkdir -p ~/$copy
    rsync -a --delete --exclude .git --exclude target --exclude node_modules --exclude .amp '$repo/' ~/$copy/
    cd ~/$copy/server
    export HAMLET_HOME=\$(mktemp -d)
    export HAMLET_VIEW=\"\$HAMLET_HOME/window\"
    trap 'rm -rf \"\$HAMLET_HOME\"' EXIT
    npm ci --silent --no-audit --no-fund
    cargo build -p hamlet_cli
    npx vitest --run $*
"
