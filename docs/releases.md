# Releases

`.github/workflows/release.yml` builds Hamlet for macOS (Apple Silicon),
Linux (x64) and Windows (x64), with `.github/scripts/` doing the
packaging. Each platform gets an installer and an archive of the same
programs:

| Platform | Installs with | Archive |
|---|---|---|
| macOS | `Hamlet-<version>.dmg` | `.zip` |
| Linux | `.deb` and `.rpm` | `.tar.gz` |
| Windows | `Hamlet-<version>-Setup.exe` | `.zip` |

Each holds `hamlet`, `hamlet_launch`, `hamlet_server`, `hamlet_app`, the
`node` of `harnesses/pi/.nvmrc`, `pi/` with its production
`node_modules`, and licenses.

The server finds sibling `pi/src/main.ts`, then the macOS bundle's
`Contents/Resources/pi/src/main.ts`, then the checkout it was built
from, and refuses to start, naming `HAMLET_PI_HARNESS`, when none
exists. It runs pi with `HAMLET_NODE`, else the `node` beside it, else
Node on `PATH`.

pi's optional packages are left out: they carry Anthropic's Claude
program, whose terms are Anthropic's, so a released pi offers no Claude
([ADR 0014](adr/0014-claude-subscriptions-run-in-anthropics-own-program-under-pi.md)).

## The launcher

`hamlet_launch` does what bare `hamlet` does, for a chief with no
terminal: it starts the realm's server or reuses the running one, and
opens the window. When the server does not start, a dialog gives the last
`Error:` in `server.log`. Given `--update <file>`, it first puts that
update in place (see [Updates](#updates)).

## What each installs

macOS: without a Developer ID the app is ad-hoc signed and Gatekeeper
refuses it until the chief chooses Open Anyway in Privacy & Security.

Linux: the packages depend on bubblewrap, which the server requires and
has no insecure fallback for.

Windows: `.github/scripts/hamlet.iss` installs per account with no
administrator. Windows has no server sandbox, so a server starts only
with `hamlet.exe --insecure` in a terminal.

Remote workspaces need a separately run workspace worker, which is not
included.

## Building them

Build natively on the target platform: npm's optional and native
dependencies must match the target.

```sh
cargo xtask build --target x86_64-unknown-linux-gnu
.github/scripts/package.sh x86_64-unknown-linux-gnu v0.1.0
```

For macOS use `aarch64-apple-darwin`. For Windows see the workflow for
the build command, and run the script in Git Bash. The script writes
under `dist/`.

`cargo xtask install` is for source development: it leaves pi in the
build checkout, so keep that checkout or set `HAMLET_PI_HARNESS`.

## Signing

The release signs when the repository has the secrets the workflow's
header names, and skips signing without them.

## Updates

An installed release looks for a newer one while its window is open, and
when the chief picks **Check for Updates** in the Hamlet menu.
`hamlet_update` holds the looking, the download and the putting in place.

Each release publishes `latest.json` and `latest.json.sig`, an Ed25519
signature in base64. The window takes the manifest only when the
signature holds under the public key built into it and the version is
newer, and keeps the downloaded file only if its SHA-256 and length
match.

Restart hands the file to `hamlet_launch --update <file>`, which stops
the realm's server and puts the update in place:

- macOS: the new app takes the old one's place once `codesign` finds it
  whole and, when the old app was signed by a developer, signed by the
  same team.
- Windows: the installer runs silently.
- Linux: `pkexec` runs `apt-get install` or `dnf install`.

When the update fails, a dialog says why and the copy that is there
opens. An archive or a build from source cannot replace itself: its
toast links to the release's page.

Only a release looks: the workflow gives a tag's build the public key,
from the repository variable `HAMLET_UPDATE_KEY`, and a build without it
never looks. `HAMLET_NO_UPDATE` set to a non-empty value stops a release
from looking. `HAMLET_UPDATE_FEED` names another manifest, for trying an
update before it is published. The key pair is made once with OpenSSL;
the private key goes in the secret `HAMLET_UPDATE_SIGNING_KEY` and the
public one in the variable:

```sh
openssl genpkey -algorithm ed25519 -out hamlet-update.pem
openssl pkey -in hamlet-update.pem -pubout -outform DER | tail -c 32 | base64
```

Keep the private key out of the repository: whoever holds it can offer
every installed Hamlet a program to run.

## Publishing

The workflow runs only for pushed `v*` tags or a manual run, which
uploads workflow artifacts and publishes no release. A tag must match
the version in `Cargo.toml`'s `[workspace.package]`, or be a pre-release
of it such as `v0.1.0-dev.1`; a pre-release tag makes a GitHub
pre-release, which installed copies never update to. `latest.json` is
uploaded last, so installed copies find the update only once every file
it names is there. Pushing a tag is a publishing action, not a local
packaging check.

Validate an extracted archive from outside the checkout on a fresh
`HAMLET_HOME`. Never point release smoke tests at a personal realm.
