# CI workflows

`.github/workflows/*.yaml` is generated from `main.ts` by
[cdkactions](https://github.com/FactbirdHQ/cdkactions). Edit the definition, never
the YAML: the generated files carry a "Do not modify" header, and a job in the CI
workflow re-synthesises them and fails on any difference.

```sh
just synth-workflows     # regenerate
just check-workflows     # regenerate and fail if the committed YAML differs
```

`just check` runs the second one, so a change to this file that is not synthesised
fails before it reaches a pull request.

## What is defined here

One workflow, `cdkactions_ci.yaml`, on pull requests to `main` and on pushes to it:

| Job | Does |
|---|---|
| `check` | `just check` in the dev shell: formatting, clippy, tests, crate-graph staleness, and this drift check. |
| `build` | `devenv build outputs.dubplate`, which builds every dependency from the committed crate graph. |
| `workflows-drift` | Re-synthesises this definition and fails if the committed YAML moved. |

Every job installs Nix and devenv and then runs through the dev shell, so CI uses
the compiler pinned in `rust-toolchain.toml` and the same `treefmt`, `crate2nix`
and `mkfs.vfat` a laptop gets. A green run here means what a green run there does.

## Why a TypeScript app in a Rust repository

cdkactions is a TypeScript library, and this is the one place the repo needs a
JavaScript runtime. `devenv.nix` provides Bun for it: two dependencies, both
public on npm, run directly from source with no build step and no package manager
beyond the lockfile committed here.

The alternative is hand-written YAML, which is what this replaced. The workflows
were already duplicating the four Nix setup steps in every job, and there was
nothing to stop them drifting from what `just check` actually runs.
