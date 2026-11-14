# ADR 0008: Governed review and promotion

Status: Accepted 2026-11-14

Date: 2026-11-14

Decision owner: COOPERATOR

## Ownership and lifecycle

- **AP relationship:** historical; durable historical architecture rationale for a project-owned decision.
- **Current semantic owner:** this record. It explains the reviewer-governed path from an admitted research finding into the observation store. It is not a product specification, and it does not change the domain contract.
- **Implementation owners:** `bookradar-review` in `bookradar-store`, the additive `ResearchStore::candidate` read, the promotion and application-review helpers, the optional `bookradar-ui` `--source-id` and `--offer-id` flags, the [README](../../README.md), and this record.
- **Consumers and discovery:** implementers and reviewers, through the [README](../../README.md).
- **Retention:** preserve while the decision remains relevant. Replacement uses explicit supersession and updated inbound links; Git history preserves earlier rationale.
- **Cleanup owner:** COOPERATOR through separately authorized maintenance; no automatic deletion.

Lifecycle reference: [AP: RF-14 — Artifact Ownership and Lifecycle](../../.ap/AP.md#rf-14-artifact-ownership-and-lifecycle).

## Context

[ADR 0005](0005-versioned-research-admission.md) admits research JSON into a separate ledger and projects only unreviewed or disputed, agent-only candidates. It has no dispute-resolution command. [ADR 0004](0004-local-persistence-and-history.md) keeps `bookradar-poc` to four commands over source `1`, offer `1`. [ADR 0007](0007-local-read-only-ui-shell.md) renders that default offer. Promotion of a human-attested finding was deferred.

The product rules stay in the domain contract and in ADRs 0001–0007. This record only explains the positive promotion path approved on 2026-11-14.

## Decision

Add the `bookradar-review` binary in `bookradar-store` with exactly three commands: `show`, `promote`, and `review`. Do not add a command to `bookradar-poc` or `bookradar-research`. Do not add a crate, a schema version, or a core type.

`show` opens the research ledger read-only and prints one finding addressed by producer, run, finding id, receipt id, and position. Position `0` is valid because finding positions are zero-based. Receipt id `0` is rejected as usage. A disputed ledger review is displayed and does not by itself block a later promotion of an admitted row.

`promote` accepts only an `admitted` finding. The caller supplies the application-store source id, offer id, and observation id. A duplicate key rolls back the append and writes nothing further. The same finding may be cited again under a new observation id. There is no provenance uniqueness constraint. Provenance is `research:<producer>/<run>/<finding>@<receipt>/<position>`. Fact fields come only from the reviewer flags. The command does not copy claim text, URLs, or the agent observation time from the JSON.

A positive observation is written only when the attestation is complete: resolved edition, Slovak language, readable access, present listing, available stock, all three claims covered, evidence method `direct-browser` or `manual`, nonempty statement, reference, assessor, rationale, and reviewer approval. `agent-only` evidence is refused even when approval is present. Rejected, quarantined, and replayed rows are refused. A structurally rejected row that retained no finding id is reported as `unknown-finding`. A missing row is `unknown-finding`. An admitted row that fails the attestation is `incomplete-attestation`. A stale positive observation may be stored and stays not eligible. There is no promotion path for a negative or unavailable claim.

The research ledger is not given an `append_review`. Human reviews are appended only in the application store, application id `0x42524452`, through the existing review transaction. `review` requires the caller’s expected revision. A mismatch is `review-conflict`. A regressed assessment time stays a storage failure.

Policy refusals exit `1` with stderr prefixed by `unknown-finding`, `not-admitted`, `agent-only-evidence`, or `incomplete-attestation`. Exit `2` is usage and happens before either database is opened. Exit `1` also covers storage and filesystem failures, distinguished by the prefix. There is no exit `4`.

`bookradar-ui` accepts optional `--source-id` and `--offer-id`. Both default to `1`. Invalid ids exit `2` before I/O. `bookradar-poc` remains source `1`, offer `1`.

The nine approved cells are:

| # | Decision |
|---|---|
| 1 | New `bookradar-review` binary with exactly `show`, `promote`, and `review`. |
| 2 | Full positive attestation only. Stale positive rows may be stored and stay not eligible. No negative promotion path. |
| 3 | Ledger stays read-only. Additive `ResearchStore::candidate` only. Human reviews only in the application store. |
| 4 | Caller-supplied `(source_id, offer_id, observation_id)`. Collisions are `duplicate-key`, with no partial write and no automatic id. |
| 5 | Optional UI `--source-id` and `--offer-id`, default `1` and `1`. |
| 6 | Evidence tier E2 for this implementation. A fresh independent audit and rendered browser acceptance stay separate. This record does not certify acceptance or publication. |
| 7 | An admitted finding may be promoted while ledger knowledge is `disputed`. `show` displays that dispute. |
| 8 | The same finding may be cited by a new observation id. |
| 9 | Policy refusals exit `1` with the four categories above. No exit `4`. |

## Alternatives

| Alternative | Benefit | Reason not selected |
|---|---|---|
| Extend `bookradar-poc` | One observation binary. | ADR 0004 keeps that binary at four commands. |
| Extend `bookradar-research` | One research binary. | Promotion writes the application store, and the ledger has no review command. |
| Library helpers only | No new CLI surface. | The operator workflow needs a governed command. |
| Copy the agent-only projection | Reuses the ledger observation. | Agent-only evidence cannot become eligible. |
| `append_review` on the ledger | Could mark a finding supported in place. | Human reviews stay in the application store. |
| Automatic observation ids | Avoids caller collisions. | The caller must name the key that rolls back on collision. |
| Offer flags on `bookradar-poc` | One way to choose an offer. | The proof binary stays on source 1, offer 1. The UI flags are enough. |
| Schema version 2 | Could store promotion metadata in new columns. | Provenance and reviews already fit schema version 1. |
| Policy exit `4` | Separates refusal from storage failure. | The category prefix already separates them. Exit `3` stays the research-import code. |
| **`bookradar-review` plus a read-only candidate lookup** | Positive attestation, no schema change, and the ledger stays read-only. | **Selected.** |

## Consequences

`bookradar-core` and both SQLite schemas stay at their current contracts. `bookradar-poc` stays four commands. An agent-only observation cannot become eligible through a later supported review, because the evidence method is fixed on the observation. The UI can render a non-default offer that already exists in the store. The research candidate for the same listing stays unreviewed or disputed and not eligible.

## Limitations and deferrals

This decision does not add a live producer, a ledger dispute-resolution command, accounts, or screenshots. Rendered browser acceptance is outside this record. The domain contract, the research input contract, and ADRs 0001–0007 are unchanged. This record does not close the logical whole.
