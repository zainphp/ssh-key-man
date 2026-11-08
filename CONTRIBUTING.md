# Contributing

## Releases

Releases are published by [release.yml](.github/workflows/release.yml). Set the package version in `Cargo.toml`, update `Cargo.lock`, then push a matching tag such as `v0.1.1`. The workflow runs CI, builds Linux musl and macOS binaries for x86_64 and ARM64, and publishes archives with SHA256 checksum files. Tags containing a hyphen publish a prerelease; the default installer uses the latest stable release. Test the installer locally with `python3 tests/install.py target/debug/skm` after building the binary.

Publishing creates or reuses a draft release, checks that all four platform archives and their checksums are present, and uploads them before publishing. If an upload fails, the release stays a draft so the workflow can be rerun. Existing draft titles, notes, and prerelease settings are preserved.

Published releases are left unchanged on reruns, whether or not they are immutable. Use a new version and tag to publish fixes. This workflow supports [GitHub release immutability](https://docs.github.com/en/code-security/concepts/supply-chain-security/immutable-releases); enable it in the repository's release settings to lock published assets and tags. Publishing jobs for the same tag run one at a time.

To retry a failed publication, run the Release workflow on the existing tag:

```sh
gh workflow run release.yml --repo zainphp/ssh-key-man --ref v0.1.1
```

Manual runs use the workflow and source at the selected tag. CI and release builds check out the triggering revision, keeping code execution in that revision's cache scope. Release runs on branches are rejected. This retry command applies to tags containing this workflow; older tags retain their original workflow and inputs. To include a workflow fix, commit it and create a new version tag; rerunning an existing tag does not pick up changes from `main`.
