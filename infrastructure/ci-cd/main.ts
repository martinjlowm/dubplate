import { resolve } from 'node:path';

import { App, Job, Stack, Workflow } from '@factbird/cdkactions';
import type { Construct } from 'constructs';

// The workflows in .github/workflows are GENERATED from this file, not hand
// written. Edit here and re-synth with `just synth-workflows`; the committed
// YAML has to match, and the `workflows-drift` job below is what enforces it.
// See this directory's README.

// GitHub Actions expressions, built rather than written inline: a `${{ … }}`
// inside a template literal reads as a mistyped interpolation.
const expression = (body: string) => `\${{ ${body} }}`;

// Nix is the only entry to the toolchain. The Determinate installer plus devenv
// give CI the compiler pinned in rust-toolchain.toml, the same treefmt, the same
// crate2nix and the same mkfs.vfat that a laptop gets, so a green run here means
// the same thing as a green run there.
// Every action is pinned to a commit, tag in the comment beside it. A tag and a
// branch both move, and whoever moves one runs code in this repository's CI on
// the next push. Bumping one means resolving the new tag to its commit:
//
//   gh api repos/<owner>/<repo>/commits/<tag> --jq .sha
const nixSetup = [
  { uses: 'actions/checkout@11d5960a326750d5838078e36cf38b85af677262' }, // v4
  {
    name: 'Install Nix',
    uses: 'DeterminateSystems/nix-installer-action@ef8a148080ab6020fd15196c2084a2eea5ff2d25', // v22
  },
  {
    name: 'Nix cache',
    uses: 'DeterminateSystems/magic-nix-cache-action@908b263ff629f4cc17666315b7fd3ec127c6244d', // v14
  },
  { name: 'Install devenv', run: 'nix profile install nixpkgs#devenv' },
];

class DubplateWorkflows extends Stack {
  constructor(scope: Construct, id: string) {
    super(scope, id);

    const ci = new Workflow(this, 'ci', {
      name: 'CI',
      on: { pullRequest: { branches: ['main'] }, push: { branches: ['main'] } },
      concurrency: {
        group: `${expression('github.workflow')}-${expression('github.event.pull_request.number || github.ref')}`,
        'cancel-in-progress': true,
      },
      permissions: { contents: 'read' },
    });

    // One command, the same one a contributor runs: formatting, clippy, the
    // tests, and the check that the committed crate graph still matches the
    // manifests.
    new Job(ci, 'check', {
      name: 'Format, lint, test',
      runsOn: 'ubuntu-latest',
      timeoutMinutes: 30,
      steps: [...nixSetup, { name: 'just check', run: 'devenv shell -- just check' }],
    });

    // Builds every dependency from the committed crate graph rather than from a
    // cargo cache, which is what makes the result reproducible off one machine.
    new Job(ci, 'build', {
      name: 'Nix build',
      runsOn: 'ubuntu-latest',
      timeoutMinutes: 45,
      steps: [...nixSetup, { name: 'devenv build', run: 'devenv build outputs.dubplate' }],
    });

    // The committed workflows must equal a fresh synth of this file.
    new Job(ci, 'workflows-drift', {
      name: 'Workflows drift',
      runsOn: 'ubuntu-latest',
      timeoutMinutes: 15,
      steps: [
        ...nixSetup,
        { name: 'Re-synth workflows', run: 'devenv shell -- just synth-workflows' },
        { name: 'No drift', run: 'git diff --exit-code -- .github/workflows/' },
      ],
    });
  }
}

// Anchored to the repo-root .github/workflows regardless of the working
// directory, so running this from the root and from this package write to the
// same place.
const app = new App({
  createValidateWorkflow: false,
  outdir: resolve(import.meta.dir, '../../.github/workflows'),
});
new DubplateWorkflows(app, 'dubplate');
app.synth();
