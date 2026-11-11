#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use bookradar_core::{
    AccessResult, ClaimCoverage, ClaimCoverageState, ConflictState, EditionId, Eligibility,
    EvidenceMethod, EvidenceReference, EvidenceSupport, KnowledgeAssessment, LanguageEvidence,
    ListingDisposition, Observation, ObservationId, ObservationRef, OfferId, OfferRef,
    RecheckState, ReviewerApproval, SourceAvailability, SourceId, SourceStatement, SubjectIdentity,
    TemporalAssessment, TimestampAssertion,
};
use bookradar_store::{
    ObservationRecord, OpenMode, ReviewDraft, Store, StoreError, synthetic_offer, synthetic_target,
};

pub const T0: i64 = 1_793_523_600;
pub const INTERVAL: i64 = 86_400;

pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    pub fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        loop {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("bookradar-store-{}-{n}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => return Self { path },
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("temp dir: {error}"),
            }
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn child(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

pub fn unix(seconds: i64) -> SystemTime {
    unix_nanos(seconds, 0)
}

pub fn unix_nanos(seconds: i64, nanos: u32) -> SystemTime {
    if seconds >= 0 {
        SystemTime::UNIX_EPOCH + Duration::new(seconds as u64, nanos)
    } else {
        let absolute = seconds.unsigned_abs();
        let base = SystemTime::UNIX_EPOCH - Duration::from_secs(absolute);
        base + Duration::from_nanos(u64::from(nanos))
    }
}

pub fn known(seconds: i64, nanos: u32) -> TimestampAssertion {
    TimestampAssertion::known(
        Some(&format!("synthetic:{seconds}.{nanos:09}+01:00")),
        3_600,
        unix_nanos(seconds, nanos),
    )
    .expect("known time")
}

pub fn offer(source: u64, offer: u64) -> OfferRef {
    OfferRef::new(
        SourceId::new(source).expect("source"),
        OfferId::new(offer).expect("offer"),
    )
}

pub fn edition(id: u64) -> EditionId {
    EditionId::new(id).expect("edition")
}

pub struct Sample {
    pub id: u64,
    pub source: u64,
    pub offer_id: u64,
    pub subject: SubjectIdentity,
    pub language: LanguageEvidence,
    pub observed: TimestampAssertion,
    pub captured: TimestampAssertion,
    pub ingested: SystemTime,
    pub access: AccessResult,
    pub listing: ListingDisposition,
    pub availability: SourceAvailability,
    pub statement: Option<String>,
    pub evidence: Option<EvidenceSupport>,
    pub requested_url: Option<String>,
    pub resolved_url: Option<String>,
    pub redirect: Option<String>,
    pub provenance: String,
    pub uncertainty: Option<String>,
    pub review: Option<SampleReview>,
}

pub struct SampleReview {
    pub knowledge: KnowledgeAssessment,
    pub approval: Option<String>,
    pub assessor: String,
    pub rationale: String,
    pub assessed: SystemTime,
    pub recorded: SystemTime,
}

impl Sample {
    pub fn qualifying(id: u64, seconds: i64) -> Self {
        Self {
            id,
            source: 1,
            offer_id: 1,
            subject: SubjectIdentity::Resolved(synthetic_target()),
            language: LanguageEvidence::Slovak,
            observed: known(seconds, 0),
            captured: known(seconds, 0),
            ingested: unix(seconds + 10),
            access: AccessResult::Readable,
            listing: ListingDisposition::Present,
            availability: SourceAvailability::Available,
            statement: Some(format!("synthetic:statement:{id}")),
            evidence: Some(manual_evidence(&format!("synthetic:evidence:{id}"))),
            requested_url: None,
            resolved_url: None,
            redirect: None,
            provenance: format!("synthetic:provenance:{id}"),
            uncertainty: None,
            review: Some(supported_review(unix(seconds + 10), id)),
        }
    }
}

pub fn supported_review(at: SystemTime, id: u64) -> SampleReview {
    SampleReview {
        knowledge: KnowledgeAssessment::Supported,
        approval: Some("synthetic:reviewer".to_owned()),
        assessor: "synthetic:assessor".to_owned(),
        rationale: format!("synthetic:rationale:{id}"),
        assessed: at,
        recorded: at,
    }
}

pub fn manual_evidence(reference: &str) -> EvidenceSupport {
    let covered = ClaimCoverageState::Covered;
    EvidenceSupport::new(
        Some(EvidenceReference::new(reference).expect("reference")),
        EvidenceMethod::Manual,
        ClaimCoverage::new(covered, covered, covered),
    )
}

pub fn coverage(
    subject: ClaimCoverageState,
    language: ClaimCoverageState,
    availability: ClaimCoverageState,
) -> EvidenceSupport {
    EvidenceSupport::new(
        Some(EvidenceReference::new("synthetic:evidence").expect("reference")),
        EvidenceMethod::Manual,
        ClaimCoverage::new(subject, language, availability),
    )
}

pub fn create(dir: &TempDir) -> (PathBuf, Store) {
    let path = dir.child("store.sqlite");
    let store = Store::create(&path).expect("create");
    (path, store)
}

pub fn reopen(path: &Path) -> Store {
    Store::open(path, OpenMode::ReadWrite).expect("reopen")
}

pub fn append(store: &mut Store, sample: &Sample) -> Result<(), StoreError> {
    let record = record(sample)?;
    let review = sample.review.as_ref().map(review_draft).transpose()?;
    store.append(&record, review.as_ref())
}

pub fn record(sample: &Sample) -> Result<ObservationRecord, StoreError> {
    let observation = Observation::new(
        ObservationRef::new(
            offer(sample.source, sample.offer_id),
            ObservationId::new(sample.id).expect("id"),
        ),
        sample.subject,
        sample.language,
        sample.observed.clone(),
        sample.access,
        sample.listing,
        sample.availability,
        sample
            .statement
            .as_deref()
            .map(|text| SourceStatement::new(text).expect("statement")),
        sample.evidence.clone(),
    );
    ObservationRecord::try_new(
        observation,
        sample.ingested,
        sample.captured.clone(),
        sample.requested_url.clone(),
        sample.resolved_url.clone(),
        sample.redirect.clone(),
        sample.provenance.clone(),
        sample.uncertainty.clone(),
    )
}

fn review_draft(review: &SampleReview) -> Result<ReviewDraft, StoreError> {
    ReviewDraft::try_new(
        review.knowledge,
        review
            .approval
            .as_deref()
            .map(|text| ReviewerApproval::new(text).expect("approval")),
        review.assessor.clone(),
        review.rationale.clone(),
        review.assessed,
        review.recorded,
    )
}

pub fn current_at(store: &mut Store, seconds: i64) -> bookradar_store::CurrentView {
    store
        .current(synthetic_offer(), synthetic_target(), unix(seconds))
        .expect("current")
}

pub fn assert_context(
    view: &bookradar_store::CurrentView,
    candidate: u64,
    temporal: TemporalAssessment,
    conflict: ConflictState,
    recheck: RecheckState,
    eligible: bool,
) {
    let found = view
        .candidates()
        .iter()
        .find(|item| item.record().observation().reference().observation().get() == candidate)
        .expect("candidate");
    assert_eq!(found.assessment().temporal(), temporal);
    assert_eq!(found.context().conflict(), conflict);
    assert_eq!(found.context().recheck(), recheck);
    assert_eq!(view.conflict(), conflict);
    assert_eq!(view.recheck(), recheck);
    match found.assessment().eligibility() {
        Eligibility::Eligible { .. } => assert!(eligible),
        Eligibility::NotEligible => assert!(!eligible),
    }
}

pub fn render_at(store: &mut Store, seconds: i64) -> String {
    let now = unix(seconds);
    let view = store
        .current(synthetic_offer(), synthetic_target(), now)
        .expect("current");
    bookradar_store::render_current(&view, synthetic_offer(), synthetic_target(), now)
}
