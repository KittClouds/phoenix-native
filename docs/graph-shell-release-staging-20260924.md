# Graph shell staging correction

The Gate 3A and initial Gate 3B executables were copied from the development
target. They were unsuitable for interactive acceptance: the user reported
that dragging the shell froze the desktop across monitors. Disabling debug
information reduced linker memory but did not enable release optimization.

The Reader freeze and KAMMI candidate used release builds. The KAMMI candidate
at `C:\phoenix-bin\phoenix-kammi-golden-path-20260924\phoenix-shell.exe`
remains the known-good comparison. The user recognized it during the diagnostic
launch. No footer changes were made during this investigation.

## Measurements before replacement

Each run used a bounded five-second process CPU sample after seven seconds of
startup. Percentages are normalized across 16 logical processors. These are
diagnostic samples, not a statistically qualified performance benchmark.

| Candidate | Window | Footer | Graph | CPU |
| --- | --- | --- | --- | --- |
| Gate 3B development | Hidden | Static | Disabled | 0.00% |
| Gate 3B development | Hidden | Static | Enabled | 0.04% |
| Gate 3B development | Hidden | Default pulse | Disabled | 0.08% |
| Gate 3B development | Hidden | Default pulse | Enabled | 0.02% |
| Gate 3B development | Visible | Default pulse | Enabled | 4.06% |
| Gate 3B development | Visible | Static | Enabled | 8.93% |
| Gate 3B development | Visible | Static | Disabled | 3.95% |
| Existing KAMMI release | Visible | Default pulse | Enabled | 0.53% |

The visible static-footer test reproduced substantial CPU use. Therefore the
footer was not established as the cause. The older release also differs in
source, so it establishes a useful baseline, not an isolated build-profile
experiment. Gate 3B must be tested again from its own release executable.

## Staging rule

Use `scripts/Stage-PhoenixRelease.ps1 -Name phoenix-graph-gate3b-20260924`.
It builds with `--locked --release` on D:, copies only the release executable
to C:, verifies SHA-256 equality, and writes `release-stage.json` with source
revision, dirty status, build profile, and executable identity. Do not replace
the product executable with a development build for live acceptance.

The two mistakenly staged development executables were renamed to
`phoenix-shell.exe.disabled-dev` in their existing Gate 3A/3B directories.
Their bytes are preserved; their `.exe` launch paths are retired.

Acceptance remains open until the Gate 3B release passes visible idle CPU,
window dragging, graph controls, and return-to-idle checks. Preserve the user’s
document workspace and frozen Reader while testing the isolated QA copy.
