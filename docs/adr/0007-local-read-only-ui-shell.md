# ADR 0007: Local read-only UI shell

Status: Accepted 2026-11-13

Date: 2026-11-13

Decision owner: COOPERATOR

## Ownership and lifecycle

- **AP relationship:** historical; durable historical architecture rationale for a project-owned decision.
- **Current semantic owner:** this record. It explains the local UI shell. It is not a product specification, and it does not change the domain contract.
- **Implementation owners:** `crates/bookradar-ui`, the additive `render_current_labels` helper in `bookradar-store`, the workspace membership in the root manifest, the [README](../../README.md), and this record.
- **Consumers and discovery:** implementers and reviewers, through the [README](../../README.md).
- **Retention:** preserve while the decision remains relevant. Replacement uses explicit supersession and updated inbound links; Git history preserves earlier rationale.
- **Cleanup owner:** COOPERATOR through separately authorized maintenance; no automatic deletion.

Lifecycle reference: [AP: RF-14 — Artifact Ownership and Lifecycle](../../.ap/AP.md#rf-14-artifact-ownership-and-lifecycle).

## Context

The workspace already verified observations and research admissions through text transcripts. There was no rendered page for the current assessment, observation history, research ledger, or research candidates, and no repeatable local UI demonstration.

The product rules stay in the domain contract and in ADRs 0001–0006. This record only explains the local read-only shell.

## Decision

Add one unpublished workspace crate, `bookradar-ui`, version `0.1.0`, edition `2024`, Rust `1.99.1`, `publish = false`, with `#![forbid(unsafe_code)]`. Its only dependencies are path dependencies on `bookradar-core` and `bookradar-store`. It adds no external crate.

The crate serves one English, server-rendered HTML page and its embedded stylesheet at `/style.css`. There is no JavaScript and no JSON endpoint. The process opens the observation store and the research ledger with literal `OpenMode::ReadOnly`, builds one snapshot at the required `--now` instant, closes both connections, and serves that snapshot until exit. A failed database slot is an error panel. The other slot still renders. The server binds only the literal IPv4 address `127.0.0.1`.

`bookradar-store` exposes `render_current_labels` and routes the existing `render_current` transcript through it. The UI changes only presentation typography. It does not select candidates, parse research JSON, or apply a freshness rule of its own.

The restricted HTTP surface accepts origin-form HTTP/1.1 `GET` and `HEAD` for `/` and `/style.css`, limits headers, closes each connection after one request, and checks `Host`, `Origin`, and cross-site `Sec-Fetch-Site`. Responses carry a strict content security policy and escape displayed text.

Tests cover the rendered truth labels, uncertainty, escaping, read-only files, and the loopback boundary. `scripts/verify.sh` and the GitHub Actions workflow stay unchanged. Browser acceptance is a separate fresh session. This shell is not a running monitor.

## Alternatives

| Alternative | Benefit | Reason not selected |
|---|---|---|
| JSON endpoint plus a client page | A browser could refresh without a new process. | It adds a second wire interface, JavaScript, and more security decisions. |
| Terminal UI | Avoids an HTTP parser. | Robust terminal interaction needs its own accessibility and acceptance path. |
| UI modules inside `bookradar-store` | Avoids a new workspace member. | HTTP and HTML would sit in the persistence package. |
| An external HTTP framework | Reuses a maintained parser. | This local shell does not justify a new registry dependency. |
| **Stdlib loopback page in `bookradar-ui`** | One fixed snapshot, no new external crate, and a small test surface. | **Selected.** |

## Consequences

`bookradar-ui` is a workspace member and a default member. `Cargo.lock` records that local package and its two path dependencies. The core dependency tree stays one line. Existing CLI transcripts stay byte-compatible because they share `render_current_labels`.

The page can show checked-available wording only when the store's existing formatter returns it. Research candidates stay unreviewed or disputed, agent-only, and not eligible. An unexpected research assessment suppresses the verification display. The server does not create, migrate, import, review, or repair a database.

## Limitations and deferrals

This decision does not add live ingestion, polling, accounts, a public bind address, or a production deployment. The custom HTTP subset is for this synthetic shell. Displayed assessments use the supplied instant and do not move while the process runs. Rendered browser acceptance is outside this record and remains a separate fresh review. The domain contract and ADRs 0001–0006 are unchanged.
