//! Strict JSON research contract version 1.
//!
//! Duplicate object keys are rejected before ordinary JSON objects are built.
//! Canonical bytes are the pinned serializer's compact encoding with object keys
//! sorted. This is `canonical-json-v1`; it is not an RFC 8785 profile.

use std::fmt;

use bookradar_core::{
    AccessResult, ClaimCoverage, ClaimCoverageState, EvidenceMethod, EvidenceReference,
    EvidenceSupport, LanguageEvidence, ListingDisposition, SourceAvailability, SourceStatement,
    SubjectIdentity, TimestampAssertion,
};
use serde::de::{self, DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Value};

use crate::store::{self, StoreError};

pub(crate) const MAX_ENVELOPE_BYTES: usize = 1024 * 1024;
const MAX_FINDINGS: usize = 256;
const MAX_DEPTH: u32 = 32;
const DUPLICATE_KEY: &str = "research-duplicate-key";
const NESTING_LIMIT: &str = "research-nesting-limit";
pub(crate) const RULE_VERSION: &str = "research-admission-v1";

const ENVELOPE_KEYS: &[&str] = &[
    "contract",
    "contract_version",
    "findings",
    "producer_id",
    "reported_run_outcome",
    "run_context",
    "run_id",
];
const CONTEXT_KEYS: &[&str] = &["agent", "context", "model", "skills", "tools"];
const NAMED_KEYS: &[&str] = &["name", "version"];
const FINDING_KEYS: &[&str] = &[
    "evidence",
    "facts",
    "finding_id",
    "listing_generation",
    "listing_ref",
    "source_provenance",
    "source_ref",
    "subject",
    "times",
    "uncertainty",
];
const SUBJECT_KEYS: &[&str] = &["candidate_ref", "identifiers", "state"];
const IDENTIFIER_KEYS: &[&str] = &["diagnostics", "kind", "scope", "value"];
const TIME_KEYS: &[&str] = &[
    "accessed_at",
    "captured_at",
    "observed_at",
    "source_updated_at",
];
const STAMP_KEYS: &[&str] = &["instant", "offset_seconds", "original"];
const INSTANT_KEYS: &[&str] = &["nanoseconds", "unix_seconds"];
const FACT_KEYS: &[&str] = &[
    "access",
    "availability",
    "claim_text",
    "language",
    "listing",
    "price",
    "source_statement",
];
const PRICE_KEYS: &[&str] = &["amount", "basis", "currency"];
const EVIDENCE_KEYS: &[&str] = &[
    "excerpt",
    "reference",
    "reported_method",
    "state",
    "supports",
];
const PROVENANCE_KEYS: &[&str] = &["redirect_reference", "requested_url", "resolved_url"];
const UNCERTAINTY_KEYS: &[&str] = &["rationale", "reported_confidence"];
const CLAIMS: &[&str] = &[
    "access",
    "availability",
    "language",
    "listing",
    "price",
    "subject",
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidationReport {
    pub oversized: bool,
    pub seen: Option<usize>,
    pub envelope_reason: Option<&'static str>,
    pub positions: Vec<ValidationPosition>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidationPosition {
    pub outcome: ValidationOutcome,
    pub reason: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValidationOutcome {
    Valid,
    Rejected,
    Quarantined,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct OfferTuple {
    pub source_ref: String,
    pub listing_ref: String,
    pub listing_generation: String,
}

#[derive(Clone, Debug)]
pub(crate) struct CandidateDraft {
    pub subject: SubjectIdentity,
    pub language: LanguageEvidence,
    pub observed_at: TimestampAssertion,
    pub captured_at: TimestampAssertion,
    pub access: AccessResult,
    pub listing: ListingDisposition,
    pub availability: SourceAvailability,
    pub source_statement: Option<SourceStatement>,
    pub evidence: EvidenceSupport,
    pub requested_url: Option<String>,
    pub resolved_url: Option<String>,
    pub redirect_reference: Option<String>,
    pub uncertainty_rationale: Option<String>,
}

pub(crate) enum StructuralPosition {
    Rejected {
        reason: &'static str,
    },
    Quarantined {
        reason: &'static str,
        finding_id: Option<String>,
        canonical: Option<Vec<u8>>,
    },
    Ready {
        finding_id: String,
        canonical: Vec<u8>,
        draft: Option<Box<CandidateDraft>>,
        tuple: Option<OfferTuple>,
    },
}

pub(crate) enum Envelope {
    Oversized,
    Opaque {
        reason: &'static str,
        canonical: Option<Vec<u8>>,
    },
    Ready {
        canonical: Vec<u8>,
        producer_id: String,
        run_id: String,
        context_canonical: Vec<u8>,
        reported_outcome: &'static str,
        positions: Vec<StructuralPosition>,
    },
}

pub fn validate_research(bytes: &[u8]) -> ValidationReport {
    match read_envelope(bytes) {
        Envelope::Oversized => ValidationReport {
            oversized: true,
            seen: None,
            envelope_reason: Some("oversized"),
            positions: Vec::new(),
        },
        Envelope::Opaque { reason, .. } => ValidationReport {
            oversized: false,
            seen: None,
            envelope_reason: Some(reason),
            positions: Vec::new(),
        },
        Envelope::Ready { positions, .. } => ValidationReport {
            oversized: false,
            seen: Some(positions.len()),
            envelope_reason: None,
            positions: positions
                .iter()
                .map(|position| match position {
                    StructuralPosition::Rejected { reason } => ValidationPosition {
                        outcome: ValidationOutcome::Rejected,
                        reason,
                    },
                    StructuralPosition::Quarantined { reason, .. } => ValidationPosition {
                        outcome: ValidationOutcome::Quarantined,
                        reason,
                    },
                    StructuralPosition::Ready { .. } => ValidationPosition {
                        outcome: ValidationOutcome::Valid,
                        reason: "none",
                    },
                })
                .collect(),
        },
    }
}

pub(crate) fn read_envelope(bytes: &[u8]) -> Envelope {
    if bytes.len() > MAX_ENVELOPE_BYTES {
        return Envelope::Oversized;
    }
    let value = match parse_strict(bytes) {
        Ok(value) => value,
        Err(ParseFailure::DuplicateKey) => {
            return Envelope::Opaque {
                reason: "duplicate-key",
                canonical: None,
            };
        }
        Err(ParseFailure::NestingLimit) => {
            return Envelope::Opaque {
                reason: "nesting-limit",
                canonical: None,
            };
        }
        Err(ParseFailure::Malformed) => {
            return Envelope::Opaque {
                reason: "malformed",
                canonical: None,
            };
        }
    };
    let canonical = match serde_json::to_vec(&value) {
        Ok(bytes) => bytes,
        Err(_) => {
            return Envelope::Opaque {
                reason: "malformed",
                canonical: None,
            };
        }
    };
    let Value::Object(object) = value else {
        return Envelope::Opaque {
            reason: "malformed-header",
            canonical: Some(canonical),
        };
    };
    interpret_v1(object, canonical)
}

fn interpret_v1(object: Map<String, Value>, canonical: Vec<u8>) -> Envelope {
    let Some(contract) = object.get("contract") else {
        return opaque("malformed-header", canonical);
    };
    let Some(version) = object.get("contract_version") else {
        return opaque("malformed-header", canonical);
    };
    match contract {
        Value::String(name) if name == "bookradar-research" => {}
        Value::String(_) => return opaque("unsupported-contract", canonical),
        _ => return opaque("malformed-header", canonical),
    }
    match version_class(version) {
        VersionClass::V1 => {}
        VersionClass::Unsupported => return opaque("unsupported-version", canonical),
        VersionClass::Malformed => return opaque("malformed-header", canonical),
    }
    if let Err(reason) = exact_keys(&object, ENVELOPE_KEYS) {
        return opaque(reason, canonical);
    }
    let Ok(producer_id) = required_identifier(object.get("producer_id")) else {
        return opaque("invalid-identifier", canonical);
    };
    let Ok(run_id) = required_identifier(object.get("run_id")) else {
        return opaque("invalid-identifier", canonical);
    };
    let Some(context) = object.get("run_context") else {
        return opaque("missing-required", canonical);
    };
    let Ok(context_canonical) = canonical_of(context) else {
        return opaque("malformed", canonical);
    };
    if let Err(reason) = validate_context(context) {
        return opaque(reason, canonical);
    }
    let Some(reported) = object.get("reported_run_outcome").and_then(Value::as_str) else {
        return opaque("malformed-header", canonical);
    };
    let Some(reported_outcome) = run_outcome(reported) else {
        return opaque("invalid-run-outcome", canonical);
    };
    let Some(findings) = object.get("findings").and_then(Value::as_array) else {
        return opaque("malformed-header", canonical);
    };
    if findings.len() > MAX_FINDINGS {
        return opaque("finding-limit", canonical);
    }
    let positions = findings.iter().map(interpret_finding).collect();
    Envelope::Ready {
        canonical,
        producer_id,
        run_id,
        context_canonical,
        reported_outcome,
        positions,
    }
}

fn interpret_finding(value: &Value) -> StructuralPosition {
    let Value::Object(object) = value else {
        return StructuralPosition::Rejected {
            reason: "non-object",
        };
    };
    let canonical = canonical_of(value).ok();
    let finding_id = match object.get("finding_id") {
        None | Some(Value::Null) => {
            return StructuralPosition::Rejected {
                reason: "missing-finding-id",
            };
        }
        Some(value) => match required_identifier(Some(value)) {
            Ok(id) => id,
            Err(()) => {
                return StructuralPosition::Rejected {
                    reason: "invalid-finding-id",
                };
            }
        },
    };
    let quarantine = |reason: &'static str| StructuralPosition::Quarantined {
        reason,
        finding_id: Some(finding_id.clone()),
        canonical: canonical.clone(),
    };
    if let Err(reason) = exact_keys(object, FINDING_KEYS) {
        return quarantine(reason);
    }
    let source_ref = match optional_opaque(object.get("source_ref")) {
        Ok(value) => value,
        Err(reason) => return quarantine(reason),
    };
    let listing_ref = match optional_opaque(object.get("listing_ref")) {
        Ok(value) => value,
        Err(reason) => return quarantine(reason),
    };
    let listing_generation = match optional_opaque(object.get("listing_generation")) {
        Ok(value) => value,
        Err(reason) => return quarantine(reason),
    };
    let subject = match parse_subject(object.get("subject")) {
        Ok(subject) => subject,
        Err(reason) => return quarantine(reason),
    };
    let times = match parse_times(object.get("times")) {
        Ok(times) => times,
        Err(reason) => return quarantine(reason),
    };
    let facts = match parse_facts(object.get("facts")) {
        Ok(facts) => facts,
        Err(reason) => return quarantine(reason),
    };
    let evidence = match parse_evidence(object.get("evidence")) {
        Ok(evidence) => evidence,
        Err(reason) => return quarantine(reason),
    };
    let uncertainty = match parse_uncertainty(object.get("uncertainty")) {
        Ok(uncertainty) => uncertainty,
        Err(reason) => return quarantine(reason),
    };
    let provenance = match parse_provenance(object.get("source_provenance")) {
        Ok(provenance) => provenance,
        Err(reason) => return quarantine(reason),
    };
    if matches!(facts.access, Some(access) if !matches!(access, AccessResult::Readable))
        && facts.availability == SourceAvailability::Unavailable
    {
        return quarantine("inconsistent-access-stock");
    }
    let tuple = match (&source_ref, &listing_ref, &listing_generation) {
        (Some(source_ref), Some(listing_ref), Some(listing_generation)) => Some(OfferTuple {
            source_ref: source_ref.clone(),
            listing_ref: listing_ref.clone(),
            listing_generation: listing_generation.clone(),
        }),
        _ => None,
    };
    let draft = match (facts.access, tuple.clone()) {
        (Some(access), Some(_)) => Some(Box::new(CandidateDraft {
            subject,
            language: facts.language,
            observed_at: times.observed_at,
            captured_at: times.captured_at,
            access,
            listing: facts.listing,
            availability: facts.availability,
            source_statement: facts.source_statement,
            evidence: project_evidence(&evidence),
            requested_url: provenance.requested_url,
            resolved_url: provenance.resolved_url,
            redirect_reference: provenance.redirect_reference,
            uncertainty_rationale: uncertainty,
        })),
        _ => None,
    };
    let Some(canonical) = canonical else {
        return quarantine("malformed");
    };
    StructuralPosition::Ready {
        finding_id,
        canonical,
        draft,
        tuple,
    }
}

fn project_evidence(items: &[EvidenceItem]) -> EvidenceSupport {
    let mut subject = ClaimCoverageState::Missing;
    let mut language = ClaimCoverageState::Missing;
    let mut availability = ClaimCoverageState::Missing;
    let mut reference = None;
    for item in items {
        if reference.is_none() {
            reference = item.reference.clone();
        }
        if !matches!(item.state.as_str(), "present" | "partial") {
            continue;
        }
        for claim in &item.supports {
            match claim.as_str() {
                "subject" => subject = ClaimCoverageState::Covered,
                "language" => language = ClaimCoverageState::Covered,
                "availability" => availability = ClaimCoverageState::Covered,
                _ => {}
            }
        }
    }
    let reference = reference.and_then(|text| EvidenceReference::new(&text).ok());
    EvidenceSupport::new(
        reference,
        EvidenceMethod::AgentOnly,
        ClaimCoverage::new(subject, language, availability),
    )
}

struct EvidenceItem {
    reference: Option<String>,
    state: String,
    supports: Vec<String>,
}

struct Times {
    observed_at: TimestampAssertion,
    captured_at: TimestampAssertion,
}

struct Facts {
    language: LanguageEvidence,
    access: Option<AccessResult>,
    listing: ListingDisposition,
    availability: SourceAvailability,
    source_statement: Option<SourceStatement>,
}

struct Provenance {
    requested_url: Option<String>,
    resolved_url: Option<String>,
    redirect_reference: Option<String>,
}

fn parse_subject(value: Option<&Value>) -> Result<SubjectIdentity, &'static str> {
    let object = required_object(value)?;
    exact_keys(object, SUBJECT_KEYS)?;
    optional_opaque(object.get("candidate_ref"))?;
    let identifiers = object
        .get("identifiers")
        .and_then(Value::as_array)
        .ok_or("invalid-subject")?;
    for identifier in identifiers {
        let identifier = identifier.as_object().ok_or("invalid-subject")?;
        exact_keys(identifier, IDENTIFIER_KEYS)?;
        required_opaque(identifier.get("kind"))?;
        required_opaque(identifier.get("value"))?;
        optional_opaque(identifier.get("scope"))?;
        optional_opaque(identifier.get("diagnostics"))?;
    }
    match object.get("state").and_then(Value::as_str) {
        Some("candidate" | "unknown") => Ok(SubjectIdentity::Unresolved),
        Some("conflicting") => Ok(SubjectIdentity::Conflicting),
        _ => Err("invalid-subject"),
    }
}

fn parse_times(value: Option<&Value>) -> Result<Times, &'static str> {
    let object = required_object(value)?;
    exact_keys(object, TIME_KEYS)?;
    let observed_at = parse_stamp(object.get("observed_at"))?;
    parse_stamp(object.get("accessed_at"))?;
    let captured_at = parse_stamp(object.get("captured_at"))?;
    parse_stamp(object.get("source_updated_at"))?;
    Ok(Times {
        observed_at,
        captured_at,
    })
}

fn parse_stamp(value: Option<&Value>) -> Result<TimestampAssertion, &'static str> {
    let object = required_object(value)?;
    exact_keys(object, STAMP_KEYS)?;
    let original = optional_opaque(object.get("original"))?;
    let offset = match object.get("offset_seconds") {
        Some(Value::Null) => None,
        Some(value) => Some(json_i64(value).ok_or("invalid-timestamp")?),
        None => return Err("missing-required"),
    };
    let instant = match object.get("instant") {
        Some(Value::Null) => None,
        Some(value) => {
            let instant = value.as_object().ok_or("invalid-timestamp")?;
            exact_keys(instant, INSTANT_KEYS)?;
            let seconds = json_i64(instant.get("unix_seconds").ok_or("missing-required")?)
                .ok_or("invalid-timestamp")?;
            let nanos = json_i64(instant.get("nanoseconds").ok_or("missing-required")?)
                .ok_or("invalid-timestamp")?;
            if !(0..=999_999_999).contains(&nanos) {
                return Err("invalid-timestamp");
            }
            let nanos = u32::try_from(nanos).map_err(|_| "invalid-timestamp")?;
            Some(store::decode_instant(seconds, i64::from(nanos)).map_err(timestamp_store_error)?)
        }
        None => return Err("missing-required"),
    };
    match offset {
        None => TimestampAssertion::with_unknown_offset(original.as_deref(), instant)
            .map_err(|_| "invalid-timestamp"),
        Some(seconds_east) => {
            let instant = instant.ok_or("invalid-timestamp")?;
            let seconds_east = i32::try_from(seconds_east).map_err(|_| "invalid-timestamp")?;
            TimestampAssertion::known(original.as_deref(), seconds_east, instant)
                .map_err(|_| "invalid-timestamp")
        }
    }
}

fn timestamp_store_error(error: StoreError) -> &'static str {
    let _ = error;
    "invalid-timestamp"
}

fn parse_facts(value: Option<&Value>) -> Result<Facts, &'static str> {
    let object = required_object(value)?;
    exact_keys(object, FACT_KEYS)?;
    let language = match object.get("language") {
        Some(Value::Null) => LanguageEvidence::Unknown,
        Some(value) => match value.as_str() {
            Some("slovak") => LanguageEvidence::Slovak,
            Some("czech") => LanguageEvidence::Czech,
            Some("mixed") => LanguageEvidence::Mixed,
            Some("unknown") => LanguageEvidence::Unknown,
            Some("conflicting") => LanguageEvidence::Conflicting,
            Some(_) => return Err("invalid-language"),
            None => return Err("invalid-language"),
        },
        None => return Err("missing-required"),
    };
    let access = match object.get("access") {
        Some(Value::Null) => None,
        Some(value) => Some(match value.as_str() {
            Some("readable") => AccessResult::Readable,
            Some("rate-limited") => AccessResult::RateLimited,
            Some("captcha") => AccessResult::Captcha,
            Some("authentication-blocked") => AccessResult::AuthenticationBlocked,
            Some("timeout") => AccessResult::Timeout,
            Some("transport-error") => AccessResult::TransportError,
            Some("page-gone") => AccessResult::PageGone,
            _ => return Err("invalid-access"),
        }),
        None => return Err("missing-required"),
    };
    let listing = match object.get("listing") {
        Some(Value::Null) => ListingDisposition::Uncertain,
        Some(value) => match value.as_str() {
            Some("present") => ListingDisposition::Present,
            Some("removed") => ListingDisposition::Removed,
            Some("uncertain") => ListingDisposition::Uncertain,
            _ => return Err("invalid-listing"),
        },
        None => return Err("missing-required"),
    };
    let availability = match object.get("availability") {
        Some(Value::Null) => SourceAvailability::Unknown,
        Some(value) => match value.as_str() {
            Some("available") => SourceAvailability::Available,
            Some("reserved") => SourceAvailability::Reserved,
            Some("unavailable") => SourceAvailability::Unavailable,
            Some("unknown") => SourceAvailability::Unknown,
            _ => return Err("invalid-availability"),
        },
        None => return Err("missing-required"),
    };
    let source_statement = match optional_opaque(object.get("source_statement"))? {
        None => None,
        Some(text) => Some(SourceStatement::new(&text).map_err(|_| "invalid-text")?),
    };
    optional_opaque(object.get("claim_text"))?;
    parse_price(object.get("price"))?;
    Ok(Facts {
        language,
        access,
        listing,
        availability,
        source_statement,
    })
}

fn parse_price(value: Option<&Value>) -> Result<(), &'static str> {
    match value {
        Some(Value::Null) => Ok(()),
        Some(Value::Object(object)) => {
            exact_keys(object, PRICE_KEYS)?;
            let amount = optional_opaque(object.get("amount"))?;
            if amount
                .as_ref()
                .is_some_and(|amount| !is_nonnegative_decimal(amount))
            {
                return Err("invalid-money");
            }
            let currency = match object.get("currency") {
                Some(Value::Null) => None,
                Some(Value::String(text)) if is_currency(text) => Some(text.as_str()),
                _ => return Err("invalid-money"),
            };
            match object.get("basis").and_then(Value::as_str) {
                Some("per-copy" | "per-lot" | "uncertain") => {}
                _ => return Err("invalid-money"),
            }
            if amount.is_some() && currency.is_none() {
                return Err("invalid-money");
            }
            Ok(())
        }
        _ => Err("invalid-money"),
    }
}

fn parse_evidence(value: Option<&Value>) -> Result<Vec<EvidenceItem>, &'static str> {
    let items = value.and_then(Value::as_array).ok_or("invalid-evidence")?;
    let mut parsed = Vec::with_capacity(items.len());
    for item in items {
        let object = item.as_object().ok_or("invalid-evidence")?;
        exact_keys(object, EVIDENCE_KEYS)?;
        let reference = optional_opaque(object.get("reference"))?;
        match object.get("reported_method").and_then(Value::as_str) {
            Some("manual" | "direct-browser" | "agent-only" | "unknown") => {}
            _ => return Err("invalid-evidence"),
        }
        let state = match object.get("state").and_then(Value::as_str) {
            Some(
                state @ ("present" | "missing" | "partial" | "obscured" | "inaccessible"
                | "unknown"),
            ) => state.to_owned(),
            _ => return Err("invalid-evidence"),
        };
        let supports = object
            .get("supports")
            .and_then(Value::as_array)
            .ok_or("invalid-evidence")?;
        let mut names = Vec::with_capacity(supports.len());
        for claim in supports {
            let Some(name) = claim.as_str() else {
                return Err("dangling-evidence-claim");
            };
            if !CLAIMS.contains(&name) {
                return Err("dangling-evidence-claim");
            }
            names.push(name.to_owned());
        }
        optional_opaque(object.get("excerpt"))?;
        parsed.push(EvidenceItem {
            reference,
            state,
            supports: names,
        });
    }
    Ok(parsed)
}

fn parse_uncertainty(value: Option<&Value>) -> Result<Option<String>, &'static str> {
    let object = required_object(value)?;
    exact_keys(object, UNCERTAINTY_KEYS)?;
    let rationale = optional_opaque(object.get("rationale"))?;
    optional_opaque(object.get("reported_confidence"))?;
    Ok(rationale)
}

fn parse_provenance(value: Option<&Value>) -> Result<Provenance, &'static str> {
    let object = required_object(value)?;
    exact_keys(object, PROVENANCE_KEYS)?;
    Ok(Provenance {
        requested_url: optional_opaque(object.get("requested_url"))?,
        resolved_url: optional_opaque(object.get("resolved_url"))?,
        redirect_reference: optional_opaque(object.get("redirect_reference"))?,
    })
}

fn validate_context(value: &Value) -> Result<(), &'static str> {
    let object = value.as_object().ok_or("invalid-context")?;
    exact_keys(object, CONTEXT_KEYS)?;
    optional_named(object.get("agent"))?;
    optional_named(object.get("model"))?;
    optional_named_list(object.get("skills"))?;
    optional_named_list(object.get("tools"))?;
    optional_opaque(object.get("context"))?;
    Ok(())
}

fn optional_named(value: Option<&Value>) -> Result<(), &'static str> {
    match value {
        Some(Value::Null) => Ok(()),
        Some(Value::Object(object)) => {
            exact_keys(object, NAMED_KEYS)?;
            required_opaque(object.get("name"))?;
            optional_opaque(object.get("version"))?;
            Ok(())
        }
        _ => Err("invalid-context"),
    }
}

fn optional_named_list(value: Option<&Value>) -> Result<(), &'static str> {
    match value {
        Some(Value::Null) => Ok(()),
        Some(Value::Array(items)) => {
            for item in items {
                optional_named(Some(item))?;
                if item.is_null() {
                    return Err("invalid-context");
                }
            }
            Ok(())
        }
        _ => Err("invalid-context"),
    }
}

fn exact_keys(object: &Map<String, Value>, keys: &[&str]) -> Result<(), &'static str> {
    if object.len() == keys.len() && keys.iter().all(|key| object.contains_key(*key)) {
        Ok(())
    } else if object.keys().any(|key| !keys.contains(&key.as_str())) {
        Err("unknown-field")
    } else {
        Err("missing-required")
    }
}

fn required_object(value: Option<&Value>) -> Result<&Map<String, Value>, &'static str> {
    value.and_then(Value::as_object).ok_or("missing-required")
}

fn required_identifier(value: Option<&Value>) -> Result<String, ()> {
    let text = value.and_then(Value::as_str).ok_or(())?;
    if is_identifier(text) {
        Ok(text.to_owned())
    } else {
        Err(())
    }
}

fn required_opaque(value: Option<&Value>) -> Result<String, &'static str> {
    optional_opaque(value)?.ok_or("invalid-text")
}

fn optional_opaque(value: Option<&Value>) -> Result<Option<String>, &'static str> {
    match value {
        Some(Value::Null) => Ok(None),
        Some(Value::String(text)) if !text.is_empty() && !text.contains('\0') => {
            Ok(Some(text.clone()))
        }
        _ => Err("invalid-text"),
    }
}

pub(crate) fn is_identifier(text: &str) -> bool {
    let mut chars = text.chars();
    if !chars.next().is_some_and(|c| c.is_ascii_alphanumeric()) {
        return false;
    }
    (1..=128).contains(&text.len())
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '-'))
}

fn is_currency(text: &str) -> bool {
    text.len() == 3 && text.bytes().all(|byte| byte.is_ascii_uppercase())
}

fn is_nonnegative_decimal(text: &str) -> bool {
    let mut parts = text.split('.');
    let Some(whole) = parts.next() else {
        return false;
    };
    if whole.is_empty() || !whole.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    match parts.next() {
        None => true,
        Some(fraction) => {
            !fraction.is_empty()
                && fraction.bytes().all(|byte| byte.is_ascii_digit())
                && parts.next().is_none()
        }
    }
}

fn run_outcome(value: &str) -> Option<&'static str> {
    Some(match value {
        "succeeded" => "succeeded",
        "partial" => "partial",
        "failed" => "failed",
        "unknown" => "unknown",
        _ => return None,
    })
}

fn version_class(value: &Value) -> VersionClass {
    let Some(number) = value.as_number() else {
        return VersionClass::Malformed;
    };
    match number.as_i64() {
        Some(1) => VersionClass::V1,
        Some(_) => VersionClass::Unsupported,
        None => VersionClass::Malformed,
    }
}

enum VersionClass {
    V1,
    Unsupported,
    Malformed,
}

fn canonical_of(value: &Value) -> Result<Vec<u8>, &'static str> {
    serde_json::to_vec(value).map_err(|_| "malformed")
}

fn opaque(reason: &'static str, canonical: Vec<u8>) -> Envelope {
    Envelope::Opaque {
        reason,
        canonical: Some(canonical),
    }
}

fn json_i64(value: &Value) -> Option<i64> {
    let number = value.as_number()?;
    if number.as_f64().is_some() && number.as_i64().is_none() && number.as_u64().is_none() {
        return None;
    }
    number
        .as_i64()
        .or_else(|| number.as_u64().and_then(|value| i64::try_from(value).ok()))
}

enum ParseFailure {
    Malformed,
    DuplicateKey,
    NestingLimit,
}

fn parse_strict(bytes: &[u8]) -> Result<Value, ParseFailure> {
    if std::str::from_utf8(bytes).is_err() {
        return Err(ParseFailure::Malformed);
    }
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let value = StrictSeed { depth: 1 }
        .deserialize(&mut deserializer)
        .map_err(classify_de)?;
    deserializer.end().map_err(classify_de)?;
    Ok(value)
}

fn classify_de(error: serde_json::Error) -> ParseFailure {
    let text = error.to_string();
    if text.contains(DUPLICATE_KEY) {
        ParseFailure::DuplicateKey
    } else if text.contains(NESTING_LIMIT) {
        ParseFailure::NestingLimit
    } else {
        ParseFailure::Malformed
    }
}

struct StrictSeed {
    depth: u32,
}

impl<'de> DeserializeSeed<'de> for StrictSeed {
    type Value = Value;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(StrictVisitor { depth: self.depth })
    }
}

struct StrictVisitor {
    depth: u32,
}

impl<'de> Visitor<'de> for StrictVisitor {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("json")
    }

    fn visit_bool<E: de::Error>(self, value: bool) -> Result<Self::Value, E> {
        Ok(Value::Bool(value))
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
        Ok(Value::Number(value.into()))
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> {
        Ok(Value::Number(value.into()))
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Self::Value, E> {
        let number = serde_json::Number::from_f64(value).ok_or_else(|| E::custom("malformed"))?;
        Ok(Value::Number(number))
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
        Ok(Value::String(value.to_owned()))
    }

    fn visit_string<E: de::Error>(self, value: String) -> Result<Self::Value, E> {
        Ok(Value::String(value))
    }

    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(Value::Null)
    }

    fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(Value::Null)
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        StrictSeed { depth: self.depth }.deserialize(deserializer)
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        if self.depth > MAX_DEPTH {
            return Err(de::Error::custom(NESTING_LIMIT));
        }
        let mut items = Vec::new();
        while let Some(item) = seq.next_element_seed(StrictSeed {
            depth: self.depth + 1,
        })? {
            items.push(item);
        }
        Ok(Value::Array(items))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        if self.depth > MAX_DEPTH {
            return Err(de::Error::custom(NESTING_LIMIT));
        }
        let mut object = Map::new();
        while let Some(key) = map.next_key::<String>()? {
            let value = map.next_value_seed(StrictSeed {
                depth: self.depth + 1,
            })?;
            if object.contains_key(&key) {
                return Err(de::Error::custom(DUPLICATE_KEY));
            }
            object.insert(key, value);
        }
        Ok(Value::Object(object))
    }
}
