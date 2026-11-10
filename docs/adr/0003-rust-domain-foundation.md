# ADR 0003: Rust domain foundation

Status: Accepted 2026-11-10

Date: 2026-11-10

Decision owner: COOPERATOR

## Ownership and lifecycle

- **AP relationship:** historical; durable historical architecture rationale for a project-owned decision.
- **Current semantic owner:** [domain contract](../domain-contract.md). This ADR records why the first Rust slice has its shape; it is not a second product specification.
- **Implementation owners:** `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, and the `bookradar-core` Rust sources.
- **Consumers and discovery:** implementers and reviewers, through the [README](../../README.md).
- **Retention:** preserve while the decision remains relevant. Replacement uses explicit supersession and updated inbound links; Git history preserves earlier rationale.
- **Cleanup owner:** COOPERATOR through separately authorized maintenance; no automatic deletion.

Lifecycle reference: [AP: RF-14 — Artifact Ownership and Lifecycle](../../.ap/AP.md#rf-14-artifact-ownership-and-lifecycle).

## Context

BookRadar had an accepted domain contract and no executable implementation.
The first code slice needs a precise place for identity, language, access,
availability, listing, knowledge, and freshness without also choosing storage,
ingestion, or a user interface.

The product rules stay in the domain contract. This record only explains the
Rust boundary selected for that contract's verification kernel.

## Decision

Implement one library, `bookradar-core`, with a pure typed boundary and the
Rust standard library only. Pin the toolchain at Rust `1.99.1` with `rustfmt`
and `clippy`. The library evaluates one caller-supplied observation and one
explicit assessment context. It does not persist observations, reduce history,
or admit research.

No public API stability is committed. The package is `publish = false`.

## Alternatives

| Alternative | Benefit | Reason not selected |
|---|---|---|
| Workspace or binary | Room for later applications and commands in the same Cargo tree. | The selected slice has one library and no executable behavior to host. |
| In-memory repository | A place to retain more than one observation during tests. | History selection and conflict detection are deferred, so a store would invent ownership and lifecycle rules early. |
| SQLite | Durable storage inside the first implementation. | It adds a native dependency, schema ownership, and migration decisions before the policy kernel is executable. |

## Consequences

Callers own the current assessment context, including conflict and recheck
state. The library does not detect those conditions or choose which historical
observation is current. Clearing a context does not change an observation's age.

The slice gives no ingestion, parsing, or storage guarantees. A successful
evaluation does not prove that evidence was inspected or that a purchase will
succeed. Later persistence can consume the typed seam without inheriting a
storage contract from this decision.

No public API stability commitment follows from publishing this crate in the
repository. The contract remains the product semantic owner.

## Limitations and deferrals

Catalog matching, ISBN processing, work and copy models, prices, owner scope,
research envelopes, replay, history reduction, storage, timestamp parsing,
presentation, UI, ingestion, and integrations remain deferred. Protocol files,
the domain contract, and the earlier ADRs are outside this decision's
implementation allowlist.
