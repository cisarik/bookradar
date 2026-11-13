# ADR 0006: Verification gate and CI

Status: Accepted 2026-11-13

Date: 2026-11-13

Decision owner: COOPERATOR

## Ownership and lifecycle

- **AP relationship:** historical; durable historical architecture rationale for a project-owned decision.
- **Current semantic owner:** this record. It explains the verification gate. It is not a product specification, and it does not change the domain contract.
- **Implementation owners:** `.gitignore`, `scripts/verify.sh`, `.github/workflows/verify.yml`, the [README](../../README.md), and this record.
- **Consumers and discovery:** implementers and reviewers, through the [README](../../README.md).
- **Retention:** preserve while the decision remains relevant. Replacement uses explicit supersession and updated inbound links; Git history preserves earlier rationale.
- **Cleanup owner:** COOPERATOR through separately authorized maintenance; no automatic deletion.

Lifecycle reference: [AP: RF-14 — Artifact Ownership and Lifecycle](../../.ap/AP.md#rf-14-artifact-ownership-and-lifecycle).

## Context

The workspace already had a dependency-free core, a local store, a research ledger, and 74 tests. The gates that checked them were a manual command list. A maintenance commit left `.gitignore` matching only `/tests/target/`, so a normal build created an untracked `target/`. Nothing ran those gates on a clean machine after publication.

The product rules stay in the domain contract. This record only explains how that repository is verified.

## Decision

One bash script, `scripts/verify.sh`, is the local and CI gate. GitHub Actions workflow `verify` checks out the repository and runs that script. It does not restate the cargo commands.

The job runs on `ubuntu-24.04` with a 20-minute timeout. It triggers on push to `main`, on pull requests to `main`, and on manual dispatch. Workflow permissions are `contents: read`. Checkout uses `actions/checkout` pinned to `3d3c42e5aac5ba805825da76410c181273ba90b1`, with submodules, without persisted credentials, and with fetch depth 1. The runner's rustup selects Rust `1.99.1` from `rust-toolchain.toml`. There is no cache and no third-party Rust action.

The script fetches the locked dependency graph, then builds, tests, and runs clippy offline. It checks formatting, requires `cargo tree -p bookradar-core` to be one `bookradar-core` line, runs `git diff --check`, and runs strict `./.ap/ap doctor`. It refuses a dirty worktree before those commands and requires `git status --porcelain` to be empty afterward, which fails if `/target/` is no longer ignored.

## Alternatives

| Alternative | Benefit | Reason not selected |
|---|---|---|
| `ubuntu-latest` | Tracks GitHub's current image label. | The label moves, so the runner is not the one named in this decision. |
| A macOS job | Would repeat the maintainer's Darwin host in CI. | It adds a second image without a known Linux-only failure. |
| `dtolnay/rust-toolchain` | Installs the toolchain explicitly. | The Ubuntu 24.04 image already provides rustup, and the script checks Rust `1.99.1`. |
| `actions/cache` | Shorter repeat runs. | Another action pin, and a warm cache can hide a cold-runner failure. |
| `Swatinem/rust-cache` | Rust-specific cache paths. | A third-party cache action is a larger supply chain than this slice needs. |
| Duplicating commands in YAML | The workflow would show every cargo flag. | The list would drift from `scripts/verify.sh`. |
| **One script on `ubuntu-24.04`** | The same fail-closed gate locally and in CI, with one pinned checkout action. | **Selected.** |

## Consequences

`bash scripts/verify.sh` is the command a reviewer runs. The workflow file is not evidence that a public run passed; that run is observed after publication. A missing `/target/` ignore fails the clean-worktree check after the build. The core package stays dependency-free because the tree check rejects a second line.

The workflow does not deploy, release, schedule, or read a secret. Repository settings are unchanged.

## Limitations and deferrals

This decision does not add a status badge, a macOS job, a dependency cache, or a second workflow. License, contribution, privacy, security-response, support, and release policy remain undecided. The domain contract and ADRs 0001–0005 are outside this decision's implementation.
