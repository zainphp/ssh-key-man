# Contributing

## Releases

Releases are published by [release.yml](.github/workflows/release.yml). Set the package version in `Cargo.toml`, update `Cargo.lock`, then push a matching tag such as `v0.1.1`. The workflow runs CI, builds Linux musl and macOS binaries for x86_64 and ARM64, and publishes archives with SHA256 checksum files. Tags containing a hyphen publish a prerelease; the default installer uses the latest stable release. Test the installer locally with `python3 tests/install.py target/debug/skm` after building the binary.

Publishing creates or reuses a draft release, checks that all four platform archives and their checksums are present, and uploads them before publishing. If an upload fails, the release stays a draft so the workflow can be rerun. Existing draft titles, notes, and prerelease settings are preserved.

Published releases are left unchanged on reruns, whether or not they are immutable. Use a new version and tag to publish fixes. This workflow supports [GitHub release immutability](https://docs.github.com/en/code-security/concepts/supply-chain-security/immutable-releases); enable it in the repository's release settings to lock published assets and tags. Publishing jobs for the same tag run one at a time.

To recover a failed publication without moving its tag, push the workflow fix to `main`, then run the Release workflow manually with the existing tag:

```sh
gh workflow run release.yml --repo zainphp/ssh-key-man --ref main -f tag=v0.1.1
```

The manual run checks and builds the tagged source using the workflow on `main`. Re-running an old failed run uses its original workflow, so it will not pick up a later workflow fix.
