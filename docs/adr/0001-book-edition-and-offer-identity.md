# ADR 0001: Book, edition, copy, and offer identity

Status: Accepted 2026-11-10

Date: 2026-11-10

Decision owner: COOPERATOR

## Ownership and lifecycle

- **AP relationship:** historical; durable historical rationale for a project-owned decision.
- **Current semantic owner:** [domain contract](../domain-contract.md); this ADR records rationale rather than a second set of current rules.
- **Consumers and discovery:** future implementers and reviewers, through the [contract](../domain-contract.md) and [README](../../README.md).
- **Retention:** preserve while the decision remains relevant. Replacement uses explicit supersession and updated inbound links; Git history preserves earlier rationale.
- **Cleanup owner:** COOPERATOR through separately authorized maintenance; no automatic deletion.

Lifecycle reference: [AP: RF-14 — Artifact Ownership and Lifecycle](../../.ap/AP.md#rf-14-artifact-ownership-and-lifecycle).

## Context

BookRadar must distinguish physical Slovak-language editions from Czech
editions, including used copies offered by several sellers. Titles, ISBNs,
and URLs may be absent, ambiguous, incorrect, or reused. Flattening these
facts would merge distinct books or copies and attach stock or condition
claims to the wrong subject.

## Decision

Adopt the contract's separation of intellectual work, edition/printing,
physical copy/lot, and merchant-offer identity. Use stable internal references
and treat external identifiers as attributed evidence. Preserve unresolved
matches and traceable identity corrections instead of forcing a merge.

The current definitions and matching rules belong to
[Glossary and relationships](../domain-contract.md#glossary-and-relationships)
and [Identity and matching](../domain-contract.md#identity-and-matching).
Slovak/Czech differentiation is governed by
[Language evidence](../domain-contract.md#language-evidence).
Owner-specific acquisition and possession remain distinct from catalog facts
under [Personal state](../domain-contract.md#personal-state).

## Alternatives

| Alternative | Benefit | Reason not selected |
|---|---|---|
| ISBN-primary identity | Familiar lookup and an initially simple key. | Missing, invalid, reused, format, volume, and set identifiers cannot safely identify every edition or copy. |
| Title-level flattening | Minimal initial vocabulary and matching effort. | Similar titles and translations conflate works, languages, publications, and sale units. |
| Detailed bibliographic ontology | Rich publication and cataloging precision. | Requires standards and granularity decisions beyond the bounded product contract. |

## Consequences

The separation reduces false merges and prevents copy condition or merchant
availability from becoming work-level facts. It requires explicit uncertainty,
reviewer effort, and correction paths that preserve provenance across merges
or splits. More concepts are necessary than in a flat title list, but they
do not prescribe tables, cardinalities, or Rust types.

## Limitations and deferrals

Unresolved matches remain unresolved; there is no universally reliable ISBN,
title, author-name, or URL shortcut. Identifier normalization can produce a
candidate without proving bibliographic identity.

Identifier formats, automatic matching algorithms, full bibliographic standards,
storage, and Rust types remain **Deferred**. Account and privacy design remain
**Deferred**; conceptual owner separation selects neither.

Review examples in [Worked cases](../domain-contract.md#worked-cases) and use
the contract's [Evolution and review](../domain-contract.md#evolution-and-review)
rules when this decision changes.
