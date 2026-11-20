# Development workflow

Hooks and CI for Penguin Pass. Cargo on this machine runs through `./x`,
which uses the `penguin-pass-toolbox` image. Build that image once with
`./x toolbox` before a hook or a local cargo command.

## Enable the hooks

The directory has to be a git repository first. The owner runs `git init`.
Then, from the repo root:

```bash
lefthook install
```

Lefthook reads `lefthook.yml` and writes the hook scripts under `.git/hooks`.
Run `lefthook install` again after `lefthook.yml` changes. The config is
checked with `lefthook validate`.

## What pre-commit runs

`pre-commit` sets `parallel: true`. Jobs with a `glob` are skipped when
no staged file matches, so a docs-only commit does not start the toolbox.

| Job | Runs when | Command |
| --- | --- | --- |
| fmt | A Rust or Cargo file is staged (`*.rs`, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, rustfmt/clippy config, `.cargo/config`) | `./x fmt` |
| clippy | The same Rust or Cargo files | `./x clippy` |
| workflows | A file under `.github/**/*.yml` or `.github/**/*.yaml` is staged | actionlint 1.7.12 via podman |
| whitespace | Every commit | `git diff --cached --check` |

`./x fmt` is `cargo fmt --all --check`. `./x clippy` is `cargo clippy --workspace --all-targets -- -D warnings`. rustfmt does not take cargo's build lock, so the two jobs overlap. `./x lint` still runs both, one after the other, for a single local command. The workflow and whitespace jobs do not use `target/`.

`git diff --cached --check` fails the commit when the staged diff adds trailing whitespace or a conflict marker.

## What pre-push runs

`pre-push` runs typecheck, then test (`parallel: false`, `piped: true`).

| Job | Command | Cargo inside `./x` |
| --- | --- | --- |
| typecheck | `./x typecheck` | `cargo check --workspace --all-targets` |
| test | `./x test` | `cargo test --workspace` |

`typecheck` and `test` both compile into `target/` and take cargo's build lock. Running them together makes the second container wait on that lock. The hook runs typecheck first. With `piped: true`, a typecheck failure skips test.

## Skip a hook

Skip every Lefthook job for one command:

```bash
LEFTHOOK=0 git commit
LEFTHOOK=0 git push
```

`LEFTHOOK=false` is the same switch. Git also skips the hook scripts themselves:

```bash
git commit --no-verify
git push --no-verify
```

Skip a single job by its tag (`fmt`, `clippy`, `workflows`, `whitespace`, `typecheck`, `test`):

```bash
LEFTHOOK_EXCLUDE=clippy git commit
LEFTHOOK_EXCLUDE=test git push
```

## What CI runs

`.github/workflows/ci.yml` runs on a push to `main` and on every pull request. The workflow token can read the repository contents. Cargo cache save and restore use the runner's cache token, which is separate from that permission. A new run on the same ref cancels the older one.

Four jobs run in parallel on `ubuntu-latest`: `lint`, `typecheck`, `test`, and `smoke`. Each one calls `./x`. The workflow sets `PP_NATIVE=1`, so `./x` runs cargo on the runner instead of starting the toolbox image. The runner has a C toolchain. `actions/checkout@v7` checks out the repo. `actions-rust-lang/setup-rust-toolchain@v2` installs the channel and components from `rust-toolchain.toml` and caches cargo through Swatinem/rust-cache. The setup also installs `pkg-config` and `libdbus-1-dev` for the Bluetooth crate that will link libdbus.

The workflow sets `build-warnings` to an empty string. The action's default is `deny`, and `./x typecheck` and `./x test` do not pass that flag. Clippy still runs with `-D warnings`. The action also sets `CARGO_INCREMENTAL=0` and `CARGO_PROFILE_DEV_DEBUG=0` so the cache stays small.

`smoke` runs `./x build`, then downloads the official Godot 4.7.2 Linux x86_64 binary. It checks the zip against `SHA512-SUMS.txt` from that same release and keeps the download in the Actions cache (`actions/cache@v6`). It then runs `GODOT=<that binary> ./x smoke`, which looks for `PENGUIN_PASS_SMOKE_OK`.

## How local and CI commands map

CI calls the same `./x` subcommands as a local checkout. `PP_NATIVE=1` is the only difference, and it changes where cargo runs, not which flags it gets.

| Local | CI job |
| --- | --- |
| `./x lint` | lint (`./x lint`, which is `./x fmt` then `./x clippy`) |
| `./x typecheck` | typecheck |
| `./x test` | test |
| `GODOT=/path/to/godot ./x smoke` | smoke, after `./x build` |

On this machine, leave `PP_NATIVE` unset. `./x` then uses the `penguin-pass-toolbox` image. A missing image fails with `error: toolbox image 'penguin-pass-toolbox' is not built. Run ./x toolbox`.
