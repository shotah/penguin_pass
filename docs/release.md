# Cutting a release

`./x release` is how a version gets a tag. The tag push is what starts
the Flatpak workflow. The workflow does not run on a normal push.

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
`<release>` from the tag, or from `Cargo.toml` on a manual run, when
it builds. `godot/project.godot` and `godot/export_presets.cfg` do not
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

After a push, the run is listed at
<https://github.com/shotah/penguin_pass/actions> and the release at
<https://github.com/shotah/penguin_pass/releases>. The release job
creates the GitHub Release if it does not exist, or uploads the
bundle onto it if it does.

## Run the same tag again

Open the failed run and choose **Re-run all jobs**. That uses the same
tag and the same commit. Do not delete the tag and push it again; a
second `./x release` of that version is refused because the tag
exists.

The **Run workflow** button starts the workflow on a branch. The
release job only runs when the ref is a `v*` tag, so a button press on
`main` builds a bundle and does not attach it to a release. To
dispatch the workflow on the tag itself:

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

The workflow file is the setup. The release job asks for
`contents: write` on the automatic `GITHUB_TOKEN` and uses no secret.
Actions has to be enabled, and the repository has to allow a workflow
to take that write. If the release job fails with
`Resource not accessible by integration`, the limit is
**Settings → Actions → General → Workflow permissions**. No other
token, environment, or secret is named in the workflow.
