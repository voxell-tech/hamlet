# Releases

`.github/workflows/release.yml` builds Hamlet for macOS (Apple Silicon),
Windows (x64) and Linux (x64). It runs only when a version tag is pushed
or when it is started by hand from the Actions tab:

```sh
git tag v0.1.0 && git push origin v0.1.0   # builds, and publishes a release
```

A run by hand keeps the three archives as the run's artifacts and
publishes nothing. `.github/scripts/package.sh` does the packing, and
can be run on a build of your own.
