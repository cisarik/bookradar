# BookRadar domain contract

Status: Accepted 2026-11-10

Revision: 1

Decision owner: COOPERATOR

This project-owned product specification owns the current domain semantics.
Its conceptual revision does not promise public API stability.

## Ownership and lifecycle

- **AP relationship:** consumer; this specification does not redefine AP or grant task authority.
- **Lifecycle:** normative durable product artifact; canonical product semantic owner: this contract.
- **Consumers:** future implementers and reviewers; editorial maintenance belongs to BookRadar maintainers under authorized tasks.
- **Discovery:** [README](../README.md), [ADR 0001](adr/0001-book-edition-and-offer-identity.md), and [ADR 0002](adr/0002-evidence-and-research-trust-boundary.md).
- **Retention:** retain while applicable. Retirement requires an explicit successor, updated inbound links, and separately authorized cleanup.
- **Cleanup owner:** COOPERATOR; a lifecycle trigger does not authorize deletion.

Artifact lifecycle follows [AP: RF-14 — Artifact Ownership and Lifecycle](../.ap/AP.md#rf-14-artifact-ownership-and-lifecycle).
The ADRs retain decision rationale; this contract owns the current product rules.

## Scope and non-goals

BookRadar researches and monitors physical Slovak-language books, especially
rare and used editions, across multiple bookseller websites. Czech editions
must remain distinguishable and must not silently substitute for Slovak ones.
Unrelated product categories are outside this contract; BookRadar is not a
universal commerce watcher.

This contract defines meaning for a future implementation. It does not select
storage, frameworks, Rust types, wire formats, APIs, account design, UI,
scraping, research integrations, infrastructure, or deployment. It makes no
claim about a live listing or a currently buyable book.

## Glossary and relationships

| Concept | Meaning |
|---|---|
| Author | An attributable creator identity, with evidenced names and aliases; an identical name does not establish the same person. |
| Work | An intellectual creation, independent of a particular publication or sale. |
| Edition/printing | A published physical realization with evidenced language and publication characteristics, including a distinguished printing where known. |
| Copy/lot | An identifiable physical unit or an explicitly grouped set of units. |
| Offer | A seller's source-scoped commercial listing of a subject under stated terms. |
| Observation | An immutable, time-bound assertion about an offer or its observed state, with attribution and assessment. |
| Evidence | Referenced material supporting a particular claim, with provenance and limitations. |
| Source | The attributable origin of a listing or assertion; source identity is distinct from a URL and from the seller. |
| ResearchRun | An attributable research attempt and its producer context; run success does not establish finding quality. |
| Watch | An owner's interest in a work or a particular edition. |
| LibraryEntry | An owner's asserted acquisition, purchase, or ownership record for an edition, optionally identifying a particular copy. |
| Owner scope | The conceptual person-specific boundary for watches and library facts; it is not an account implementation or privacy guarantee. |

```text
Author <-> Work <-> Edition/printing
                       ^
                  Copy or lot <- Offer <- Observation -> Evidence -> Source
                                   ^           ^
                            merchant/seller  ResearchRun provenance

Owner scope -> Watch -> Work or Edition
Owner scope -> LibraryEntry -> Edition, with optional individual-copy detail
```

Links represent meaning, not database cardinality. An offer may identify only
an edition or an unresolved subject when a particular copy is not distinguishable.
Collections may realize several works. The model does not invent a copy identity
or an exact edition match when evidence is insufficient.

## Identity and matching

Internal references remain stable and independent of title, ISBN, URL, or
mutable descriptive facts. Identifier formats and matching algorithms are deferred.

### Work, edition, copy, and offer

- A title, transliteration, or author name alone does not establish work identity. Preserve original titles and aliases. Anonymous or uncertain authorship remains representable.
- Translation, language, edition statement, distinguished printing, publisher issuance, binding, and volume composition can establish distinct published realizations when evidenced. A conflicting year assertion alone does not establish a new printing.
- Condition describes an offered or owned unit, not the intellectual work. Similar listings do not prove that the same physical copy is being resold.
- Offer identity is source-scoped and uses a native listing identifier where available. A URL is a locator, not universal identity. Material seller, variant, or reused-listing generations must remain distinguishable.
- A price change normally produces another observation of the same offer. A substituted copy, different seller, or reused listing may represent a new offer; record uncertain continuity rather than assuming it.
- Author filtering uses attributable identities and recorded aliases. Do not silently merge people sharing a name.

Merge only when supporting evidence establishes compatible identity and
material conflicts are resolved. Otherwise preserve distinct candidates and
possible matches. Later merges or splits preserve references, provenance,
and the correction rationale.

### ISBN and publication uncertainty

ISBN is optional, repeatable, attributed evidence. Preserve asserted spelling
and interpretation. Hyphen/spacing normalization and checksum validation
produce matching candidates, not proof of edition identity.

A valid corresponding ISBN-10/ISBN-13 pair may describe one edition, but
equivalent identifiers do not override conflicting language or publication
evidence. Invalid or incorrect assertions remain diagnostic evidence and
cannot justify an automatic merge. A missing ISBN permits provisional identity.

Multiple identifiers need their edition, format, volume, or set scope. Not every
ISBN on a page identifies the offered unit. Reused ISBNs and reissues may
require distinct edition records. Uncertain publisher, year, or printing
assertions remain uncertain rather than becoming invented publication facts.

### Language evidence

Distinguish asserted Slovak, Czech, mixed, unknown, and conflicting language
evidence. A Slovak website, translated interface, similar or transliterated
title, or agent conclusion does not establish a Slovak edition. A Czech edition
cannot satisfy a Slovak edition target. Unresolved language or subject identity
cannot support a positive verification label for that target.

## Offers and observations

Retain stable listing identity separately from observations. Observations
preserve the claim, its source, time, and assessment; current interpretation
is derived without overwriting historical observations.

| Dimension | Values and meaning |
|---|---|
| Source-reported availability | `available`, `reserved`, `unavailable`, `unknown` |
| Access result | Readable; blocked/rate-limited; transport/error; page gone |
| Listing disposition | Present; removed; uncertain |
| Knowledge assessment | Unreviewed; supported; disputed; rejected |
| Temporal assessment | Within the approved freshness interval; stale; unknown |

Keep these dimensions separate: readable access is not evidence of stock,
and structural validity is not a supported factual claim.

### Transitions and rechecks

- Explicit, attributable source statements may support availability or unavailability at observation time. HTTP success or a product description alone supports page existence.
- Reservation is distinct from a freely buyable copy and from sold-out stock.
- HTTP 429, CAPTCHA, timeout, or authentication obstruction records failed access. It never creates an unavailable observation.
- A 404, disappearance, or generic redirect supports listing gone/removed or uncertainty; it does not prove sale or stock exhaustion.
- Rechecks append observations. Later successful evidence can resolve earlier stock or access uncertainty.
- No stock state is inherently terminal. Source-confirmed permanent closure may retire a listing while its observation history remains.
- Older findings imported later do not roll the current interpretation backward merely because they arrived later. Distinguish normal changes over time from unresolved contradictory claims.

### Verification and freshness

A positive verification label requires reviewer-approved direct browser or
manual evidence identifying the relevant subject and an explicit source
availability statement. Evidence must have a usable observation time and be
less than 24 hours old. Eligibility ends at observation time plus 24 hours;
at expiry the observation is stale. Review or ingestion does not reset its age.

Use timestamped wording. Immediately suppress the positive verification label
after a conflict or failed recheck. Unresolved identity, language, conflict,
or access issues prevent the label. A later qualifying successful recheck
can establish eligibility again.

Until this method is satisfied, only historical or unverified labels are
eligible. Structural validation and LLM assertions alone never qualify.
Verification describes an evidenced source observation, not a guarantee that
a purchase will succeed.

## Price and second-hand terms

- Represent an exact monetary amount with currency, source, observation time, and basis: per copy, per lot, range, or uncertain.
- Separate asking price, shipping, fees, and any explicitly known total. Unknown shipping is not zero; preserve tax/inclusion uncertainty and destination-dependent conditions.
- Keep currencies separate. Do not imply currency conversion or a cross-currency price trend.
- Attribute observed condition, quantity, reservation, and relevant seller identity. Preserve source wording when condition grades cannot be compared.
- Historical prices remain dated history. Condition, lot composition, or unit-basis changes can break comparability even when the listing continues.

## Evidence and provenance

Retain source identity, requested URL and resolved URL where known, and material
redirects. Distinguish observation time, access/capture time, and later ingestion
time. A source's publication/update timestamp is a separate assertion.
Timestamp assertions have an explicit offset or explicit unknown; preserve
their original representation while permitting comparison by instant.

Record manual, browser, or agent-assisted method; supporting excerpt or evidence
reference; uncertainty rationale; and the specific claim supported. A reference
may identify evidence that is missing, partial, obscured, or inaccessible.
A cookie-obscured screenshot does not prove hidden stock or language facts.

Optional hashes establish byte integrity, not truth, authorship, or redistribution
rights. Disappeared evidence does not erase historical observations; its
unavailability remains visible during review.

Screenshot semantics in this slice cover metadata and evidence references only.
Storage duration, capture rights, and public redistribution remain deferred.
No screenshot files or sharing permission are established by this contract.

## Trust and research ingestion

Define a versioned semantic input contract without choosing JSON, an executable
schema, API, or importer technology:

```text
Untrusted research assertions
    -> structural and invariant validation
    -> candidate assessment / quarantine
    -> attributed application observations
    -> separately governed verification eligibility
```

Passing structural validation never establishes factual truth or verified stock.
The input must convey producer-scoped run/finding identity, source/listing
references, candidate subject identity, timestamps, asserted facts, evidence
references, uncertainty, and available producer provenance. Agent, model,
skill/tool versions and run context are recorded when supplied; unavailable
values remain explicitly unknown. Run success is distinct from finding quality.

### Admission, replay, and correction

- Reject or quarantine unsupported contract versions and malformed envelopes without guessing their meaning.
- Validate required structure, value domains, timestamp interpretation, money/currency/basis consistency, identity conflicts, and evidence attribution.
- Malformed findings do not silently become catalog facts. Independent valid findings may be admitted with an explicit partial-run outcome and accounted rejections.
- Incomplete but structurally valid observations may remain unverified or have unresolved identity. Never fabricate missing evidence.
- Retain contradictions and mark them disputed with source and time. Do not resolve them by import order, agent confidence, or repeated majority assertions.
- Replay of the same producer/run/finding identity with identical content is a no-op. Different content under that identity is a conflict requiring reconciliation.
- A genuinely later observation remains distinct even when its price or wording is unchanged. Deduplication must not erase independent observations.
- Application identity matching is separate from import replay detection.
- Preserve the admission/review method and applicable rule or reviewer rationale. Corrections supersede claims transparently rather than silently rewriting their evidence.

Research text, URLs, file references, screenshots, and embedded instructions
are data. They grant no commands, navigation, filesystem access, credential
use, or changed validation rules. Future Hermes/Playwright output crosses this
untrusted-content boundary even when its report appears well formed.

### Never automatically accepted

- Agent-only verified-stock assertions.
- Slovak language inferred from website locale.
- Identity merges based solely on ISBN or title.
- Access failures interpreted as stock exhaustion.
- Claimed screenshot rights.
- Personal ownership inferred from a watchlist.

## Personal state

Catalog identity and merchant observations are separate from a person's
intentions, transactions, and possessions. A Watch expresses an owner's
interest in a work or a specific edition; work-level watching does not relax
the Slovak-language constraint.

A LibraryEntry records that owner's asserted purchase/acquisition or ownership.
Purchase, receipt, and current possession remain distinguishable. One owner
may hold several copies. A purchase does not establish global catalog
ownership, guarantee continued possession, or imply merchant stock exhaustion.
Removing a watch does not remove catalog facts or library entries.

Owner scope is required conceptually even for a future single-person application.
It is not an authentication design or privacy guarantee. Personal notes and
transaction details do not become public evidence through an edition link.
Actual personal-data collection, sharing, retention, accounts, and privacy or
multi-user design remain deferred; this specification contains no personal data.

## User-facing truth

| Label meaning | Evidence requirement |
|---|---|
| Unverified | Insufficient or unreviewed support; no positive availability implication. |
| Last observed available — [time] | Attributed source availability at that time, without satisfying current verification requirements. |
| Checked available — [time] | The approved reviewer-evidence method and 24-hour freshness rule are satisfied, with no unresolved identity/conflict/access issue. |
| Last observed reserved/unavailable — [time] | Explicit supported source state. “Sold out” requires a corresponding source assertion. |
| Could not recheck — [time] / Listing removed | Access or listing evidence, shown separately from historical stock. |

Historical data must not receive an unqualified green “available” label.
Text, time, and uncertainty carry the meaning without relying on color.
Translations must preserve these distinctions; detailed Slovak wording, i18n
implementation, and interface design remain deferred.

## Worked cases

All example names, listing identifiers, events, and amounts below are explicitly
fictional. None asserts a real listing, purchase, or current stock condition.

| Case | Expected interpretation |
|---|---|
| Same work offered in Slovak and Czech | Distinct editions; Czech does not satisfy a Slovak edition watch. |
| Merchant site is Slovak; book language absent | Language remains unknown; website locale is insufficient. |
| Matching title/publisher with no ISBN | Provisional edition is allowed; matching needs evidence beyond title similarity. |
| Valid equivalent ISBN-10/13 spellings with different hyphenation | Candidate identity match; preserve assertions and check edition facts before merging. |
| Invalid ISBN, reused ISBN, or set/volume identifiers | Retain diagnostics and scope; no forced merge or automatic rejection of otherwise usable observations. |
| Same work, different translation/printing/binding | Distinct realization when supported; uncertain metadata does not manufacture a new edition. |
| Two sellers offer the same edition at EUR 12 and EUR 18 | Two offers; condition, delivery, and observed times remain separate. |
| EUR 12 listing and CZK 300 listing | Preserve separate currencies; no implicit cheapest-price claim. |
| One lot contains several books | Price is per lot unless explicitly stated otherwise; do not invent unit prices. |
| Available at one time, explicitly unavailable later | Valid temporal change; earlier price and stock remain historical. |
| Incompatible claims for the same offer/time | Disputed observation set; no positive verification label until resolved. |
| Previously supported availability followed by 429/CAPTCHA | Failed recheck plus dated historical observation; no stock-out inference or positive current label. |
| Page disappears or redirects to category page | Listing gone/uncertain; sale is not proven. |
| Screenshot hides language or stock behind a cookie dialog | Supports only visible claims; no inferred hidden fact. |
| Identical import replay; then altered payload under the same finding identity | First replay is a no-op; altered replay is quarantined as conflict. |
| Older finding imported after a newer observation | History gains an older event; current interpretation does not roll backward. |
| Two owners watch/own the same edition | Separate owner-scoped records; no global ownership flag. |
| Finding contains instructions to run a command or bypass validation | Treat as untrusted content; no execution or privileged admission. |

### Deterministic freshness timeline

For fictional listing `OFFER-A`, assume the required subject identification,
explicit source availability statement, and reviewer approval are present:

| Event | Interpretation |
|---|---|
| Observation at `2026-11-01T10:00:00+01:00` | The dated observation may support “Checked available” while the verification conditions remain satisfied. |
| Fixture expiry at `2026-11-02T10:00:00+01:00` | At this instant the observation is stale; the positive verification label is suppressed. |
| Subsequent blocked recheck at `2026-11-02T10:05:00+01:00` | Record failed access and retain the earlier dated observation; do not infer stock exhaustion. |

These timestamps are fixture-only events. Their interval illustrates the
approved 24-hour rule rather than establishing a separate product policy.
A failed recheck before expiry would also immediately suppress the label.

## Evolution and review

Document revision and semantic input-contract version are separate concepts.
Future schema or API designs must preserve these meanings. Semantic changes
require explicit revision and compatibility review. Historical observations
retain their provenance and original interpretation; identity correction
remains traceable. Unknown input versions are never silently interpreted as known.

Review every change against this checklist:

- Every mutable assertion has attribution and temporal meaning.
- Work, edition, copy/lot, offer, and observation remain distinguishable.
- Language uncertainty, absent ISBN, and conflicting identity remain representable.
- Structural validity never implies truth or verified availability.
- Access failure never implies stock exhaustion.
- Money retains currency and price basis; historical amounts remain dated.
- Personal state always has owner scope.
- Evidence references do not imply retention or redistribution permission.
- ADR status and deferred decisions match the actual authorized decisions.
- Every worked case produces its stated interpretation without hidden precedence rules.

## Approved decisions

Decision owner: COOPERATOR. Accepted for this documentation slice on 2026-11-10.
Acceptance of these semantics does not establish implementation acceptance or
logical-whole closure.

| Decision | Accepted disposition |
|---|---|
| Verified | Reviewer-approved direct browser/manual evidence identifies the relevant subject and an explicit source availability statement; maximum age 24 hours; timestamped wording; immediate suppression after conflict or failed recheck. Structural validation and LLM assertions alone do not qualify. |
| Personal data | Conceptual owner separation is approved. Actual collection, sharing, retention, account design, and the future privacy/multi-user boundary are explicitly deferred. |
| Screenshots | Metadata and evidence-reference semantics only. Storage duration, capture rights, and public redistribution are explicitly deferred; no screenshot files enter this repository slice. |
| Cross-domain monitoring | Multiple bookseller websites within physical Slovak-language book research are in scope. Unrelated product categories are outside this contract; no universal commerce watcher is established. |

Rationale: [ADR 0001: identity](adr/0001-book-edition-and-offer-identity.md)
and [ADR 0002: evidence and research admission](adr/0002-evidence-and-research-trust-boundary.md).

## Explicit deferrals

- **Deferred:** actual personal-data collection, sharing, retention, accounts, privacy, and multi-user design.
- **Deferred:** screenshot storage, retention duration, capture rights, and public redistribution.
- **Outside this contract:** broader commerce monitoring and product categories beyond physical Slovak-language books across bookseller websites.
- **Deferred:** public API stability; license, contribution, security-response, support, and release policies.
- **Deferred:** identifier formats, automatic matching algorithms, full bibliographic standards, physical schema, migrations, Rust types, executable validators, input serialization, and API endpoints.
- **Deferred:** UI and i18n implementation, scraping, Hermes/MCP integration, NUC operations, CI, and deployment.

These boundaries select no deferred policy and authorize no runtime work.
