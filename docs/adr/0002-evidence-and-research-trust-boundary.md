# ADR 0002: Evidence-backed observations and untrusted research admission

Status: Accepted 2026-11-10

Date: 2026-11-10

Decision owner: COOPERATOR

## Ownership and lifecycle

- **AP relationship:** historical; durable historical rationale for a project-owned decision.
- **Current semantic owner:** [domain contract](../domain-contract.md); this ADR records rationale rather than a second set of current rules.
- **Consumers and discovery:** future implementers and reviewers, through the [contract](../domain-contract.md) and [README](../../README.md).
- **Retention:** preserve while relevant; a replacement explicitly supersedes this record and updates inbound links. Git history retains earlier rationale.
- **Cleanup owner:** COOPERATOR through separately authorized maintenance; no automatic deletion.

Lifecycle reference: [AP: RF-14 — Artifact Ownership and Lifecycle](../../.ap/AP.md#rf-14-artifact-ownership-and-lifecycle).

## Context

Merchant offers change over time. A page can remain accessible after stock
changes, while CAPTCHA or rate limits can prevent a recheck without saying
anything about stock. Agent-generated research can contain mistakes,
contradictions, fabricated certainty, or hostile embedded instructions.
Screenshots preserve only what is visible and do not establish rights to share it.

## Decision

Separate stable offers from attributed, time-bound observations and preserve
their provenance. Distinguish structural admission of research from evidence
assessment and from eligibility for a positive verification label. Agent or
tool output remains untrusted data even when its structure is valid.

Accept the verification rule recorded on 2026-11-10: reviewer-approved direct
browser/manual evidence identifying the relevant subject and an explicit source
availability statement, a maximum age of 24 hours, timestamped wording, and
immediate suppression after conflict or failed recheck. Eligibility expires at
observation time plus 24 hours. Structural validation and LLM assertions alone
do not qualify. Access failure never implies stock exhaustion.

Screenshot semantics cover metadata and evidence references only. This
decision establishes neither screenshot retention nor redistribution rights.

These statements record the decision's rationale and scope. The current rules
belong to the contract's [Offers and observations](../domain-contract.md#offers-and-observations),
[Verification and freshness](../domain-contract.md#verification-and-freshness),
[Evidence and provenance](../domain-contract.md#evidence-and-provenance),
[Trust and research ingestion](../domain-contract.md#trust-and-research-ingestion),
and [User-facing truth](../domain-contract.md#user-facing-truth).

## Alternatives

| Alternative | Benefit | Reason not selected |
|---|---|---|
| Trust agent reports as application knowledge | Fastest route from research to visible results. | Report formatting, confidence, and model assertions do not establish truth, stock, or authority. |
| Mutable current-state record | Simple display and update model. | Overwrites history and conflates stock, access failure, provenance, and observation age. |
| Manual-only records | Direct human authorship of every observation. | Excludes useful research-assisted candidates and partial results; review can be separated from raw collection. |

## Consequences

The decision supports honest history, explicit failed rechecks, and partial
research results. Replay conflicts, contradictory findings, and unavailable
evidence remain reviewable rather than silently disappearing. It introduces
review and conflict concepts and limits when positive wording can be used.

It does not verify any real listing or implement admission controls. Integrity
hashes identify bytes rather than truth or authorship; owner scope does not
implement access control. The contract's [Personal state](../domain-contract.md#personal-state)
and [Worked cases](../domain-contract.md#worked-cases) preserve those distinctions.

## Deferrals and scope boundaries

Executable ingestion formats, validators, integrations, and account/privacy
implementation remain **Deferred**. Actual personal-data collection, sharing,
and retention remain **Deferred**. Screenshot storage duration, capture rights,
and public redistribution remain **Deferred**.

Multiple bookseller websites for physical Slovak-language books are accepted
scope; broader cross-category commerce monitoring is outside this contract.
No universal commerce watcher, security-response commitment, privacy policy,
or public API stability promise is selected.

See [Approved decisions](../domain-contract.md#approved-decisions),
[Explicit deferrals](../domain-contract.md#explicit-deferrals), and
[Evolution and review](../domain-contract.md#evolution-and-review) for the current
decision boundaries and change-review rules.
