# Research input contract version 1

Status: implementation format for the synthetic research ledger.

This document is subordinate to the [domain contract](domain-contract.md). It
records the executable wire shape, not product truth. Structural admission
does not establish factual truth, reviewer approval, eligibility, or verified
stock.

## Envelope

The contract name is `bookradar-research`. `contract_version` is the integer
`1`. A different version is retained as an opaque quarantined envelope and is
not interpreted as version 1. Semantic input version, canonical-content rule,
ledger schema version, and the domain-contract revision are separate numbers.

Envelope fields are exactly:

| Field | Shape |
|---|---|
| `contract` | string `bookradar-research` |
| `contract_version` | integer `1` |
| `producer_id` | identifier |
| `run_id` | identifier |
| `run_context` | `ProducerContext` |
| `reported_run_outcome` | `succeeded`, `partial`, `failed`, or `unknown` |
| `findings` | array of finding objects, at most 256 |

`producer_id` is an opaque declared identifier. It is not authenticated, and
it is not derived from a hash or from `run_context`. Identifiers match
`[A-Za-z0-9][A-Za-z0-9._:-]{0,127}` with no trimming and no case folding.

`reported_run_outcome` is stored on the receipt. It is not part of the
immutable `(producer_id, run_id)` run context and not part of finding
equality. A later submission may report a different outcome for the same run.
That changes neither the stored run context nor replay equality of an
unchanged finding.

`ProducerContext` contains exactly `agent`, `model`, `skills`, `tools`, and
`context`. `agent` and `model` are null or `{name, version}` objects. `version`
may be null. `skills` and `tools` are null or arrays of the same object. Null
means the producer did not supply the value. An empty array means it was
supplied as empty. `context` is null or opaque text.

## Finding

Every finding object contains exactly these fields:

| Field | Shape |
|---|---|
| `finding_id` | identifier, scoped to one producer and run |
| `source_ref` | nullable opaque reference |
| `listing_ref` | nullable opaque reference |
| `listing_generation` | nullable opaque reference |
| `subject` | `{state, candidate_ref, identifiers}` |
| `times` | four timestamp assertions |
| `facts` | language, access, listing, availability, source statement, price, claim text |
| `evidence` | array of evidence assertions |
| `uncertainty` | `{rationale, reported_confidence}`, both nullable text |
| `source_provenance` | `{requested_url, resolved_url, redirect_reference}`, all nullable |

`subject.state` is `candidate`, `unknown`, or `conflicting`. `candidate_ref`
is nullable text. `identifiers` is an array of `{kind, value, scope,
diagnostics}`. Identifier text is preserved. No ISBN checksum, hyphen
normalization, or edition match is performed. A projected observation uses
`Unresolved` for `candidate` and `unknown`, and `Conflicting` for
`conflicting`. No edition row is created. The `current` command's edition id
is an operator-supplied application target and is never inferred from the
finding.

`times` contains exactly `observed_at`, `accessed_at`, `captured_at`, and
`source_updated_at`. Each value is an object with exactly `original`
(nullable text), `offset_seconds` (nullable integer), and `instant` (`null`
or `{unix_seconds, nanoseconds}`). Nanoseconds are `0..999999999`. Offsets
are within ±64800 seconds. A known offset requires an instant. Null offset
means the offset is unknown even when an instant is present. Numeric instants
are the wire time. Original text is retained and is not parsed as RFC 3339.
Unrepresentable instants quarantine the finding.

`facts.language` is null or `slovak`, `czech`, `mixed`, `unknown`, or
`conflicting`. Null language is unknown. `facts.access` is null or one of
`readable`, `rate-limited`, `captcha`, `authentication-blocked`, `timeout`,
`transport-error`, or `page-gone`. Null access means unknown and is not
projected. `facts.listing` is null or `present`, `removed`, or `uncertain`.
`facts.availability` is null or `available`, `reserved`, `unavailable`, or
`unknown`. `source_statement` and `claim_text` are nullable text.

## Price

`facts.price` is null or `{amount, currency, basis}`. This object is the
asserted asking price only. Shipping, fees, totals, ranges, currency
conversion, unit-price derivation, and price history are deferred. Unknown
shipping is not stored as zero and a total is not invented.

`amount` is null or a nonnegative decimal string. The string is not
rewritten. `currency` is null or three uppercase ASCII letters. `basis` is
`per-copy`, `per-lot`, or `uncertain`. A known amount requires a currency.
Missing currency, a sign, an exponent, or any other basis quarantines the
finding.

## Evidence and provenance

Each evidence item contains exactly `reference`, `reported_method`, `state`,
`supports`, and `excerpt`. Methods are `manual`, `direct-browser`,
`agent-only`, or `unknown`. States are `present`, `missing`, `partial`,
`obscured`, `inaccessible`, or `unknown`. `supports` names `subject`,
`language`, `availability`, `access`, `listing`, or `price`. Any other name
quarantines the finding. These values remain producer assertions.

The projected observation always records application support as `AgentOnly`.
Coverage is `Covered` when an evidence item in state `present` or `partial`
names that claim, and `Missing` otherwise. There is no unassessed coverage
value. Producer-reported manual or browser methods stay in the research
provenance and cannot qualify the observation. The admission review is
`Unreviewed`, with assessor `rule:research-admission-v1` and no reviewer
approval. A detected conflict appends `Disputed` and still supplies no
approval.

`source_provenance` preserves the requested URL, resolved URL, and a material
redirect reference when the producer supplies them. URLs are data. The
importer does not open them.

## Limits and canonical bytes

The maximum envelope is 1 MiB. Readers stop after one extra byte and reject
the input before any ledger write. Nesting deeper than 32 containers, or more
than 256 findings, quarantines the envelope without per-finding
interpretation. Duplicate object keys, including escaped equivalents such as
`a` and `\u0061`, quarantine the whole envelope. Unknown envelope or context
fields do the same. Unknown finding fields quarantine only that finding.
Malformed JSON, invalid UTF-8, and malformed headers are quarantined with
`seen=unknown` and no guessed finding count.

`canonical-json-v1` re-serializes the decoded value with the pinned JSON
implementation: object keys are sorted, array order and decoded string
content are preserved, and there is no Unicode, URL, identifier, timestamp
text, or decimal normalization. This rule is not an RFC 8785 profile and it
is not a hash.

The ledger stores the bounded raw bytes and, when parsing succeeds, the
canonical bytes on `research_inputs`. Equality is exact byte equality of
those canonical bytes. The lookup is a comparison of the stored BLOB. It is
not a digest and not a page-sized index key, because a 1 MiB value does not
fit in a default SQLite index page. An identical canonical envelope adds no
receipt, observation, or assessment. Whitespace and object-key order do not
change equality.

Finding equality compares contract version 1, the immutable run context, and
the canonical finding bytes. Local ingestion time, receipt id, array
position, and `reported_run_outcome` are excluded.

## Admission

Replay identity is `(producer_id, run_id, finding_id)`. For a supported
envelope with `N` positions, `N = admitted + replayed + rejected +
quarantined`, `conflicts <= quarantined`, and `projected <= admitted`.
`conflicts` counts identity and run-context conflicts only.

`admitted` is a structurally valid candidate, initially unreviewed.
`replayed` is an identical known finding and creates nothing new. `rejected`
has no usable finding id. `quarantined` needs reconciliation. The same batch
pre-groups identities: identical copies admit once and replay the rest;
different copies quarantine every occurrence, with no first-item winner.
Changed content under an existing identity is retained, quarantined, and
disputes the original candidate. Replaying either variant does not clear the
dispute.

Run context is immutable for `(producer_id, run_id)`. A changed context is
retained on the new receipt, quarantines that submission's usable positions,
and disputes prior candidates of the run. Producer and run scopes do not
merge findings.

Only a complete `(source_ref, listing_ref, listing_generation)` tuple with
known access is projected. The projection uses a ledger-local source and
offer surrogate. It is not a catalog match and it is not written to an
existing application database. An access failure combined with
`availability: unavailable` is quarantined as `inconsistent-access-stock`
and does not create an unavailable observation. Equal-time incompatible
claims are both retained and disputed. A later or repeated agreement does
not clear them. An older observation can extend history but does not become
current merely because its ingestion time is newer.

Application run outcome is `complete` when no position is rejected or
quarantined, `partial` when usable and unusable positions coexist, and
`quarantined` when no usable position remains. An empty valid run is
complete.

Embedded instructions, file references, and URLs in the payload are opaque
text. They grant no command, navigation, or filesystem access.

## Ledger

The research file uses SQLite application id `0x42525249` and
`user_version` 1. `ResearchStore` accepts only that file.
`Store::open` accepts only application id `0x42524452`. Neither adopts the
other. There is no schema v2 migration.

Candidate observations and reviews reuse the v1 tables inside the research
file, with the same append-only triggers. `research_inputs` holds the
receipt, raw bytes, canonical bytes, injected ingestion time, disposition,
reported outcome, rule version, and issue code. `research_runs` holds the
first context for a producer and run. `research_findings` holds one row per
interpreted position. Local ingestion time must not move backward; such an
import is rejected and leaves the ledger unchanged.

## Commands

```text
bookradar-research init <ledger>
bookradar-research import <ledger> <input.json> <ingested-unix-seconds>
bookradar-research inspect <ledger>
bookradar-research current <ledger> <producer> <source> <listing> <generation> <target-edition-id> <now-unix-seconds>
```

`init` creates a new ledger. The other commands require an existing
compatible ledger. Input is read only from the explicit file. The worked
transcript and exit codes are in the README.
