//! Reviewer-governed promotion from a research ledger into an application store.
//!
//! The ledger is only read. Fact fields come from the caller, never from
//! canonical research JSON.

#![forbid(unsafe_code)]

use std::path::Path;
use std::time::SystemTime;

use bookradar_core::{
    AccessResult, ClaimCoverage, ClaimCoverageState, EvidenceMethod, EvidenceReference,
    EvidenceSupport, KnowledgeAssessment, LanguageEvidence, ListingDisposition, Observation,
    ObservationId, ObservationRef, OfferId, OfferRef, ReviewerApproval, SourceAvailability,
    SourceId, SourceStatement, SubjectIdentity, TimestampAssertion,
};

use crate::research::{CandidateQuery, ResearchStore};
use crate::store::{ObservationRecord, OpenMode, ReviewDraft, Store, StoreError};

/// A closed refusal or a store failure. Refusal categories are stable tokens.
#[derive(Debug)]
pub enum ReviewWorkflowError {
    Store(StoreError),
    Refused { reason: &'static str },
}

impl ReviewWorkflowError {
    pub fn category(&self) -> &'static str {
        match self {
            Self::Store(error) => error.category(),
            Self::Refused { reason } => reason,
        }
    }
}

impl std::fmt::Display for ReviewWorkflowError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Store(error) => write!(formatter, "{error}"),
            Self::Refused { reason } => write!(formatter, "promotion refused ({reason})"),
        }
    }
}

impl From<StoreError> for ReviewWorkflowError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

/// Caller-attested facts for one positive promotion. Nothing is copied from JSON.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PromotionRequest {
    pub query: CandidateQuery,
    pub source_id: u64,
    pub offer_id: u64,
    pub observation_id: u64,
    pub subject: SubjectIdentity,
    pub language: LanguageEvidence,
    pub access: AccessResult,
    pub listing: ListingDisposition,
    pub availability: SourceAvailability,
    pub source_statement: String,
    pub evidence_method: EvidenceMethod,
    pub evidence_reference: String,
    pub cover_subject: bool,
    pub cover_language: bool,
    pub cover_availability: bool,
    pub reviewer_approval: Option<String>,
    pub assessor: String,
    pub rationale: String,
    pub observed_at: SystemTime,
    pub observed_offset: i32,
    pub captured_at: SystemTime,
    pub captured_offset: i32,
    pub ingested_at: SystemTime,
    pub assessed_at: SystemTime,
    pub recorded_at: SystemTime,
}

struct PreparedPromotion {
    record: ObservationRecord,
    review: ReviewDraft,
}

/// Reads the candidate, checks the attestation, then appends in one store transaction.
///
/// A refusal does not open `store_path`.
pub fn promote(
    ledger: &mut ResearchStore,
    store_path: &Path,
    request: &PromotionRequest,
) -> Result<(), ReviewWorkflowError> {
    let prepared = prepare_promotion(ledger, request)?;
    let mut store = Store::open(store_path, OpenMode::ReadWrite)?;
    store.append(&prepared.record, Some(&prepared.review))?;
    Ok(())
}

/// Appends one application-store review revision. This does not open a ledger.
pub fn append_application_review(
    store: &mut Store,
    observation: ObservationRef,
    expected_revision: u64,
    draft: &ReviewDraft,
) -> Result<u64, ReviewWorkflowError> {
    Ok(store.append_review(observation, expected_revision, draft)?)
}

fn prepare_promotion(
    ledger: &mut ResearchStore,
    request: &PromotionRequest,
) -> Result<PreparedPromotion, ReviewWorkflowError> {
    let Some(candidate) = ledger.candidate(&request.query)? else {
        return Err(refused("unknown-finding"));
    };
    if candidate.outcome != "admitted" {
        return Err(refused("not-admitted"));
    }
    if request.evidence_method == EvidenceMethod::AgentOnly {
        return Err(refused("agent-only-evidence"));
    }
    attest(request)?;
    let record = observation_record(request)?;
    let approval = request
        .reviewer_approval
        .as_deref()
        .filter(|text| nonempty(text))
        .ok_or_else(|| refused("incomplete-attestation"))?;
    let review = ReviewDraft::try_new(
        KnowledgeAssessment::Supported,
        Some(ReviewerApproval::new(approval).map_err(|_| refused("incomplete-attestation"))?),
        request.assessor.clone(),
        request.rationale.clone(),
        request.assessed_at,
        request.recorded_at,
    )?;
    Ok(PreparedPromotion { record, review })
}

fn attest(request: &PromotionRequest) -> Result<(), ReviewWorkflowError> {
    let SubjectIdentity::Resolved(_) = request.subject else {
        return Err(refused("incomplete-attestation"));
    };
    if request.language != LanguageEvidence::Slovak
        || request.access != AccessResult::Readable
        || request.listing != ListingDisposition::Present
        || request.availability != SourceAvailability::Available
        || !request.cover_subject
        || !request.cover_language
        || !request.cover_availability
        || !nonempty(&request.source_statement)
        || !nonempty(&request.evidence_reference)
        || !nonempty(&request.assessor)
        || !nonempty(&request.rationale)
        || request
            .reviewer_approval
            .as_deref()
            .is_none_or(|text| !nonempty(text))
    {
        return Err(refused("incomplete-attestation"));
    }
    if !matches!(
        request.evidence_method,
        EvidenceMethod::DirectBrowser | EvidenceMethod::Manual
    ) {
        return Err(refused("incomplete-attestation"));
    }
    stored_id(request.source_id)?;
    stored_id(request.offer_id)?;
    stored_id(request.observation_id)?;
    Ok(())
}

fn observation_record(
    request: &PromotionRequest,
) -> Result<ObservationRecord, ReviewWorkflowError> {
    let SubjectIdentity::Resolved(edition) = request.subject else {
        return Err(refused("incomplete-attestation"));
    };
    let source = SourceId::new(stored_id(request.source_id)?)
        .map_err(|_| refused("incomplete-attestation"))?;
    let offer = OfferId::new(stored_id(request.offer_id)?)
        .map_err(|_| refused("incomplete-attestation"))?;
    let observation_id = ObservationId::new(stored_id(request.observation_id)?)
        .map_err(|_| refused("incomplete-attestation"))?;
    let observed = stamp(request.observed_offset, request.observed_at)?;
    let captured = stamp(request.captured_offset, request.captured_at)?;
    let evidence = EvidenceSupport::new(
        Some(
            EvidenceReference::new(&request.evidence_reference)
                .map_err(|_| refused("incomplete-attestation"))?,
        ),
        request.evidence_method,
        ClaimCoverage::new(
            coverage(request.cover_subject),
            coverage(request.cover_language),
            coverage(request.cover_availability),
        ),
    );
    let observation = Observation::new(
        ObservationRef::new(OfferRef::new(source, offer), observation_id),
        SubjectIdentity::Resolved(edition),
        request.language,
        observed,
        request.access,
        request.listing,
        request.availability,
        Some(
            SourceStatement::new(&request.source_statement)
                .map_err(|_| refused("incomplete-attestation"))?,
        ),
        Some(evidence),
    );
    Ok(ObservationRecord::try_new(
        observation,
        request.ingested_at,
        captured,
        None,
        None,
        None,
        provenance(request),
        None,
    )?)
}

fn provenance(request: &PromotionRequest) -> String {
    format!(
        "research:{}/{}/{}@{}/{}",
        request.query.producer_id,
        request.query.run_id,
        request.query.finding_id,
        request.query.receipt_id,
        request.query.position
    )
}

fn stamp(offset: i32, instant: SystemTime) -> Result<TimestampAssertion, ReviewWorkflowError> {
    TimestampAssertion::known(None, offset, instant).map_err(|_| refused("incomplete-attestation"))
}

fn coverage(covered: bool) -> ClaimCoverageState {
    if covered {
        ClaimCoverageState::Covered
    } else {
        ClaimCoverageState::Missing
    }
}

fn stored_id(value: u64) -> Result<u64, ReviewWorkflowError> {
    if value == 0 || i64::try_from(value).is_err() {
        Err(refused("incomplete-attestation"))
    } else {
        Ok(value)
    }
}

fn nonempty(value: &str) -> bool {
    !value.is_empty() && !value.contains('\0')
}

fn refused(reason: &'static str) -> ReviewWorkflowError {
    ReviewWorkflowError::Refused { reason }
}
