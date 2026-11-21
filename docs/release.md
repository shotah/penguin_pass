# Cutting a release

Two Flatpak builds share one workflow.

A push to `main` that changes the game builds a bundle and updates one
prerelease, tag `latest`, title `Latest build`. The file name is the
Cargo version plus the short commit, for example
`penguin-pass-0.1.0+a1b2c3d-x86_64.flatpak`. A later push replaces that
file. It does not create a `v*` release.

`./x release` is how a version gets a numbered tag. That tag is a
separate release, and it behaves as it did before the prerelease.

```text
./x release [--dry-run|--no-push] {patch|minor|major|X.Y.Z}
```

From `0.1.0`, `patch` is `0.1.1`, `minor` is `0.2.0`, and `major` is
`1.0.0`. An explicit `X.Y.Z` is used as given. It has to be newer than
the version in `Cargo.toml`.

## What the command does

It refuses to start unless all of these are true. Each failure is one
line and a non-zero exit.

- The new version is `patch`, `minor`, `major`, or `X.Y.Z`, and it is
  greater than the current one.
- `git status --porcelain` is empty.
- The current branch is `main`.
- The tag `v<new>` does not already exist.

It then sets `version` under `[workspace.package]` in the root
`Cargo.toml` and runs `cargo update --workspace --offline`. That
rewrites the workspace crates in `Cargo.lock` and leaves registry
crates that are already locked. Those two files are the version. The
Flatpak metainfo is not edited here. The workflow stamps its
`<release>` from the tag on a numbered release, from the Cargo version
plus the short commit on a push to `main`, or from `Cargo.toml` alone
on a manual run. `godot/project.godot` and `godot/export_presets.cfg` do not
carry a game version. `./x package` reads the same `Cargo.toml` line.

It then runs `./x lint`, `./x typecheck`, and `./x test`. If one
fails, it copies the previous `Cargo.toml` and `Cargo.lock` back and
stops. It has not committed. If the process is killed and those files
stay bumped, put them back with:

```bash
git checkout -- Cargo.toml Cargo.lock
```

When the checks pass it commits `Release v<new>`, makes an annotated
tag `v<new>`, and pushes the branch and the tag to `origin`. It
prints each git command before it runs it.

`--dry-run` does the checks, applies the bump to a temporary copy
(the real `Cargo.toml` and `Cargo.lock` stay as they are), and prints
the git commands without running them.

`--no-push` commits and tags on this machine and prints the push
command. It does not push.

After a tag push, the run is listed at
<https://github.com/shotah/penguin_pass/actions> and the numbered
release at <https://github.com/shotah/penguin_pass/releases>. The
release job creates that GitHub Release if it does not exist, or
uploads the bundle onto it if it does. The same push of `main` also
updates the `latest` prerelease, because the version bump is not a
docs-only change.

## Every push to main

The workflow also runs on a push to `main`. It skips a push that only
touches `docs/**`, any `*.md` file, or a picture (`png`, `jpg`,
`jpeg`, `gif`, `webp`, `svg`). Several quick pushes share one
concurrency group; the newer push cancels the older build. A `v*` tag
is a different group and is not cancelled by a push to `main`.

The bundle version is the Cargo version, a `+`, and the first seven
characters of the commit. The metainfo `<release>` is stamped with
that same string. The job then moves the git tag `latest` to this
commit with the workflow token (no git command in this checkout) and
updates the prerelease: title `Latest build`, marked prerelease, notes
with the commit and the date. The previous `.flatpak` on that release
is removed so only the new file remains.

A manual **Run workflow** on `main` builds an artifact and does not
move `latest`. Only a push does that.

## Run the same tag again

Open the failed run and choose **Re-run all jobs**. That uses the same
tag and the same commit. Do not delete the tag and push it again; a
second `./x release` of that version is refused because the tag
exists.

The **Run workflow** button starts the workflow on a branch. The
numbered release job only runs when the ref is a `v*` tag, so a button
press on `main` builds a bundle and does not attach it to a release
or to `latest`. To dispatch the workflow on the tag itself:

```bash
gh workflow run flatpak.yml --ref v0.1.1
```

That is the same `workflow_dispatch` entry as the button. As the
workflow is written, the release job runs when the ref is a `v*` tag.

## By hand

1. Set `version` under `[workspace.package]` in `Cargo.toml`.
2. `cargo update --workspace --offline` (through `./x`, that is the
   toolbox unless `PP_NATIVE=1`).
3. `./x lint`, `./x typecheck`, `./x test`.
4. Commit `Release vX.Y.Z`, then `git tag -a vX.Y.Z -m "Release vX.Y.Z"`.
5. `git push origin HEAD "vX.Y.Z"`.

## Before the first tag

The workflow file is the setup. The numbered release job and the
`latest` prerelease job each ask for `contents: write` on the
automatic `GITHUB_TOKEN`. They use no secret. Actions has to be
enabled, and the repository has to allow a workflow to take that
write, including creating and moving the `latest` tag. If either job
fails with `Resource not accessible by integration`, the limit is
**Settings → Actions → General → Workflow permissions**. A ruleset
that blocks updating the tag `latest` stops the prerelease job. No
other token, environment, or secret is named in the workflow.
