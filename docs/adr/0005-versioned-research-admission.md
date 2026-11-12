# ADR 0005: Versioned research admission

Status: Accepted 2026-11-12

Date: 2026-11-12

Decision owner: COOPERATOR

## Ownership and lifecycle

- **AP relationship:** historical; durable historical rationale for a project-owned decision.
- **Current semantic owner:** [domain contract](../domain-contract.md). This ADR records why research admission is stored this way; it is not a second product specification.
- **Implementation owners:** [research input v1](../research-input-v1.md), `Cargo.lock`, `crates/bookradar-store`, and this record.
- **Consumers and discovery:** implementers, research producers, and reviewers, through the [README](../../README.md).
- **Retention:** preserve while version 1 is supported. Replacement uses explicit supersession and updated inbound links. Git history preserves earlier rationale.
- **Cleanup owner:** COOPERATOR through separately authorized maintenance; no automatic deletion.

Lifecycle reference: [AP: RF-14 — Artifact Ownership and Lifecycle](../../.ap/AP.md#rf-14-artifact-ownership-and-lifecycle).

## Context

The domain contract requires a versioned untrusted research boundary.
Structural validation must not become verified stock. The existing observation
store already has exact schema checks, so new research tables cannot be added
to those files without a migration.

## Decision

Use strict JSON contract version 1 and a separate SQLite research ledger.
Compare canonical content bytes under producer/run/finding identity. Reuse the
accepted observation/history implementation for explicitly unreviewed or
disputed candidates, without reviewer approval or promotion to existing
application databases.

The ledger application id is `0x42525249` and `user_version` is 1. The
research decoder uses exact `serde` `1.0.228` and `serde_json` `1.0.145`,
without the `derive` feature. Canonical bytes are `canonical-json-v1`, an
exact byte comparison, not a hash and not an RFC 8785 profile.

## Alternatives

| Alternative | Benefit | Reason not selected |
|---|---|---|
| Custom line-oriented text | No new Rust dependency. | New framing, escaping, nesting, duplicate, and canonicalization rules. |
| Validator only, no ledger | Smaller persistence surface. | Cannot prove replay across processes. |
| Custom append-only file | No second SQLite schema. | Framing, locking, and torn-write recovery would be new format ownership. |
| Shared schema v2 | One file for observations and research. | Requires a migration and older-reader compatibility that this boundary does not need. |
| **Strict JSON and a separate ledger** | Readable provenance, SQLite transactions, and untouched v1 files. | **Selected.** A second file, strict compatibility, and dependency provisioning remain. |

## Consequences

Existing v1 databases stay untouched. Research candidates live only in the
research file. Admission can quarantine, replay, and dispute, and it can
project an unreviewed or disputed observation. It cannot approve a claim,
match a catalog, or write an application store.

A second file must be opened explicitly. Strict decoding rejects duplicate
keys, unknown fields, and unsupported versions instead of guessing. The serde
pins and their checksums are part of the lockfile. Reconciliation of
quarantine and disputes is deferred; this slice has no resolution command.

The wire reference remains the implementation format. The domain contract
remains the product semantic owner.

## Deferrals

Live producers, application-store promotion, reviewer approval, catalog
matching, screenshots, a user interface, network APIs, and deployment remain
deferred. No license, contribution, privacy, security-response, support, or
release policy is selected.
