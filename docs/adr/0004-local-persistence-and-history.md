# ADR 0004: Local persistence and history

Status: Accepted 2026-11-11

Date: 2026-11-11

Decision owner: COOPERATOR

## Ownership and lifecycle

- **AP relationship:** historical; durable historical architecture rationale for a project-owned decision.
- **Current semantic owner:** [domain contract](../domain-contract.md). This ADR records why the local proof stores history this way; it is not a second product specification.
- **Implementation owners:** the workspace `Cargo.toml`, `Cargo.lock`, `crates/bookradar-store`, and this record.
- **Consumers and discovery:** implementers and reviewers, through the [README](../../README.md).
- **Retention:** preserve while the decision remains relevant. Replacement uses explicit supersession and updated inbound links; Git history preserves earlier rationale.
- **Cleanup owner:** COOPERATOR through separately authorized maintenance; no automatic deletion.

Lifecycle reference: [AP: RF-14 — Artifact Ownership and Lifecycle](../../.ap/AP.md#rf-14-artifact-ownership-and-lifecycle).

## Context

`bookradar-core` evaluates one supplied observation and one explicit assessment
context. It does not choose a current observation, detect a conflict, or store
history. The next proof needs a durable local store, a deterministic history
reader, and a small synthetic command that reopens the same database.

The product rules stay in the domain contract. This record only explains the
storage boundary selected for that contract.

## Decision

Keep `bookradar-core` dependency-free. Add a workspace member,
`bookradar-store`, which depends on the core path and on bundled SQLite
through `rusqlite` `0.37.0` with default features disabled. The `bookradar-poc`
binary lives in that crate.

Use one local database file per caller-selected path. Creation is exclusive.
The schema is version 1, identified by application id `0x42524452`. Opening
any other version, application id, journal mode, corrupt file, or foreign-key
violation fails closed. There is no automatic migration or reset.

Observations and reviews are append-only. Identity registries are inserted in
the same transaction as the observation that asserts them. Time is stored as a
signed Unix second plus nanoseconds, with seconds rounded downward before the
epoch. Identifiers above `i64::MAX` are rejected rather than truncated.

The store orders one offer by normalized observation time. It selects the
newest readable candidate, or the newest attempt when no readable observation
exists, and calls `evaluate_verification` for freshness and eligibility. It
does not copy the 24-hour rule. Equal-time incompatible claims, intrinsic
conflicting assertions, and active disputed reviews stay unresolved until an
explicit attributed review rejects or revises them. A change at a different
time is ordinary history. Missing observation time blocks a positive reading.
A newer unsuitable observation does not revive an older eligible one.

The proof CLI has four commands and four fixed synthetic fixtures. It is not
an import surface.

No public API stability is committed. The store package is `publish = false`.

## Alternatives

| Alternative | Benefit | Reason not selected |
|---|---|---|
| In-memory only | Easy isolated tests and no native build. | It cannot show that a later process reopens the same history. |
| Custom append-only file | No database dependency. | Framing, torn writes, locking, and versioning would be new format ownership. |
| Pure-Rust embedded database | No C build. | The file format and recovery model would still be engine-specific, and this slice did not compare one in tests. |
| System SQLite | Familiar inspection tools. | Linking depends on a host development library and version. |
| **Bundled SQLite** | Transactions, constraints, and a reproducible engine source. | **Selected.** Native compilation and schema ownership remain. |

## Consequences

The core library still does not persist data or reduce history. Callers of the
store supply observation time, ingestion time, and review time. Review text
does not make an observation younger.

Equal-time conflicts remain visible until an attributed rejection or review
revision clears the incompatible pair. A later successful observation alone
does not clear one. Different-time availability, reservation, and listing
changes are normal history.

The database uses rollback journal mode `DELETE`, `synchronous=FULL`, foreign
keys, and immediate busy errors. It does not use WAL, a connection pool, an
ORM, or an async runtime.

Existing files are never deleted or downgraded by opening them. A later format
needs its own migration design and must preserve existing data.

## Limitations and deferrals

The proof does not ingest network data, inspect evidence bytes, authenticate
reviewers, repair missing observation times, or answer what was known at a
past instant. It does not claim power-loss behavior beyond SQLite transaction
rollback, multi-process stress, or Ubuntu execution. Natural-language
contradiction analysis, catalog matching, prices, and a user interface remain
deferred. The domain contract and ADRs 0001–0003 are outside this decision's
implementation.
