# Branches and releases

## Branches

| Branch | Role | Merges into |
| --- | --- | --- |
| `main` | Released code. Every commit is a tagged release or a hotfix. | — |
| `develop` | Integration branch for the next release. Default target for pull requests. | `release/*` |
| `feature/<name>` | One feature or change, cut from `develop`. | `develop` |
| `fix/<name>` | Bug fix for the next release, cut from `develop`. | `develop` |
| `release/<x.y>` | Stabilisation of a release, cut from `develop`. Only fixes, version bump and changelog. | `main`, then back into `develop` |
| `hotfix/<x.y.z>` | Urgent fix of a published release, cut from `main`. | `main`, then back into `develop` |

## Versions

Semantic versioning. The version lives in two places, keep them equal:

- `Cargo.toml` (`[workspace.package] version`)
- `crates/desktop/tauri.conf.json` (`version`)

`CHANGELOG.md` collects entries under `## Unreleased` on `develop`.

## Release

1. `git switch -c release/x.y develop`
2. Bump the version in both files, rename `## Unreleased` to `## x.y.0` in `CHANGELOG.md`.
3. Fix only what blocks the release; CI must be green. Pushes to `release/*` also run the CI `installer` job, which builds the
   NSIS installer and uploads it as the `diskmap-setup` artifact; install and uninstall it once before tagging.
4. Merge into `main`, tag `vx.y.0` on `main` and push the tag. The Release workflow builds and publishes two assets:
   `diskmap-vx.y.0-windows-x64.zip` (portable, with the `portable` marker) and `diskmap-vx.y.0-windows-x64-setup.exe` (NSIS
   installer). Keep these names: winget and the updater depend on them.
5. Merge `main` back into `develop` and delete the release branch.

## Hotfix

1. `git switch -c hotfix/x.y.z main`
2. Fix, bump the patch version, add a `## x.y.z` changelog entry.
3. Merge into `main`, tag `vx.y.z`, push the tag.
4. Merge `main` back into `develop`.

## Repository settings

Done once on GitHub, not in the repository:

- Default branch: `develop`.
- Protect `main` and `develop`: pull request required, CI `check` required, no force push.
