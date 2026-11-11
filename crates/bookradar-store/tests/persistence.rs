//! Storage integrity for the local observation history.

#![forbid(unsafe_code)]

mod common;

use std::time::{Duration, SystemTime};

use bookradar_core::{
    AccessResult, ClaimCoverageState, EvidenceMethod, KnowledgeAssessment, LanguageEvidence,
    ListingDisposition, SourceAvailability, SubjectIdentity, TimestampAssertion,
};
use bookradar_store::{Fixture, ObservationRecord, OpenMode, ReviewDraft, Store, StoreError};

use common::{
    INTERVAL, Sample, T0, TempDir, append, create, known, record, reopen, supported_review, unix,
    unix_nanos,
};

#[test]
fn round_trip_reopen_preserves_every_supplied_field() {
    let dir = TempDir::new();
    let (path, mut store) = create(&dir);
    let mut full = Sample::qualifying(1, T0);
    full.observed = TimestampAssertion::with_unknown_offset(
        Some("  synthetic:observed  "),
        Some(unix_nanos(T0, 999_999_999)),
    )
    .expect("observed");
    full.captured = TimestampAssertion::known(
        Some("synthetic:captured"),
        -3_600,
        unix_nanos(-1, 500_000_000),
    )
    .expect("captured");
    full.ingested = unix_nanos(T0, 25);
    full.requested_url = Some("  synthetic:requested  ".to_owned());
    full.resolved_url = Some("synthetic:resolved".to_owned());
    full.redirect = Some("synthetic:redirect".to_owned());
    full.uncertainty = Some(" synthetic:uncertain ".to_owned());
    full.statement = Some("  synthetic:statement  ".to_owned());
    full.review = Some(supported_review(unix_nanos(T0, 40), 1));
    append(&mut store, &full).expect("append");

    let mut absent = Sample::qualifying(2, T0 + 1);
    absent.evidence = None;
    absent.statement = None;
    absent.review = None;
    absent.observed = TimestampAssertion::unknown();
    absent.captured = TimestampAssertion::unknown();
    append(&mut store, &absent).expect("absent");

    let mut missing_reference = Sample::qualifying(3, T0 + 2);
    missing_reference.evidence = Some(bookradar_core::EvidenceSupport::new(
        None,
        EvidenceMethod::Manual,
        bookradar_core::ClaimCoverage::new(
            ClaimCoverageState::Covered,
            ClaimCoverageState::Missing,
            ClaimCoverageState::Covered,
        ),
    ));
    append(&mut store, &missing_reference).expect("missing reference");
    drop(store);

    let mut store = reopen(&path);
    let entries = store
        .history(bookradar_store::synthetic_offer())
        .expect("history");
    assert_eq!(entries.len(), 3);
    let full_entry = entries
        .iter()
        .find(|entry| entry.record().observation().reference().observation().get() == 1)
        .expect("full");
    assert_eq!(full_entry.record(), &record(&full).expect("full"));
    assert_eq!(full_entry.reviews().len(), 1);
    assert_eq!(full_entry.reviews()[0].revision(), 1);
    assert_eq!(full_entry.reviews()[0].assessed_at(), unix_nanos(T0, 40));
    let absent_entry = entries
        .iter()
        .find(|entry| entry.record().observation().reference().observation().get() == 2)
        .expect("absent");
    assert_eq!(absent_entry.record(), &record(&absent).expect("absent"));
    assert!(absent_entry.reviews().is_empty());
    assert!(absent_entry.record().observation().evidence().is_none());
    assert_eq!(
        absent_entry.record().observation().observed_at(),
        &TimestampAssertion::unknown()
    );
    let missing_reference_entry = entries
        .iter()
        .find(|entry| entry.record().observation().reference().observation().get() == 3)
        .expect("third");
    assert!(
        missing_reference_entry
            .record()
            .observation()
            .evidence()
            .unwrap()
            .reference()
            .is_none()
    );
    assert_ne!(
        full_entry.record().observation().observed_at(),
        &TimestampAssertion::unknown()
    );
}

#[test]
fn duplicate_and_empty_reference_leave_history_unchanged() {
    let dir = TempDir::new();
    let (_path, mut store) = create(&dir);
    append(&mut store, &Sample::qualifying(1, T0)).expect("append");
    assert!(matches!(
        append(&mut store, &Sample::qualifying(1, T0 + 1)),
        Err(StoreError::DuplicateKey)
    ));
    assert!(
        ObservationRecord::try_new(
            Sample::qualifying(2, T0).let_observation(),
            unix(T0),
            known(T0, 0),
            None,
            None,
            None,
            "",
            None,
        )
        .is_err()
    );
    let history = store
        .history(bookradar_store::synthetic_offer())
        .expect("history");
    assert_eq!(history.len(), 1);
    assert_eq!(
        history[0]
            .record()
            .observation()
            .reference()
            .observation()
            .get(),
        1
    );
}

#[test]
fn codec_boundaries_reject_unrepresentable_values() {
    let dir = TempDir::new();
    let (_path, mut store) = create(&dir);
    let mut fractional = Sample::qualifying(1, T0);
    fractional.observed = known(T0, 999_999_999);
    fractional.captured = TimestampAssertion::known(
        Some("pre-epoch"),
        0,
        SystemTime::UNIX_EPOCH - Duration::from_millis(500),
    )
    .expect("pre-epoch");
    append(&mut store, &fractional).expect("fraction");
    let mut unknown_offset = Sample::qualifying(2, T0);
    unknown_offset.observed =
        TimestampAssertion::with_unknown_offset(Some("offset-unknown"), Some(unix(T0)))
            .expect("offset");
    append(&mut store, &unknown_offset).expect("offset");
    drop(store);

    let dir = TempDir::new();
    let (_path, mut store) = create(&dir);
    let mut oversized = Sample::qualifying(1, T0);
    oversized.id = 1 << 63;
    assert!(matches!(
        append(&mut store, &oversized),
        Err(StoreError::OutOfStorageRange)
    ));
    assert!(
        store
            .history(bookradar_store::synthetic_offer())
            .expect("history")
            .is_empty()
    );
    if let Some(instant) =
        SystemTime::UNIX_EPOCH.checked_add(Duration::new((i64::MAX as u64) + 1, 0))
    {
        let mut far = Sample::qualifying(2, T0);
        far.ingested = instant;
        assert!(matches!(
            append(&mut store, &far),
            Err(StoreError::OutOfStorageRange)
        ));
    }
}

#[test]
fn review_revisions_conflict_and_do_not_cross_observations() {
    let dir = TempDir::new();
    let (path, mut store) = create(&dir);
    let mut older = Sample::qualifying(1, T0);
    older.review = None;
    append(&mut store, &older).expect("older");
    let mut newer = Sample::qualifying(2, T0 + 10);
    append(&mut store, &newer).expect("newer");
    let first = review_at(unix(T0 + 5), "first");
    assert_eq!(
        store
            .append_review(older.reference(), 0, &first)
            .expect("revision 1"),
        1
    );
    assert!(matches!(
        store.append_review(older.reference(), 0, &review_at(unix(T0 + 6), "stale")),
        Err(StoreError::ReviewRevisionConflict)
    ));
    assert!(matches!(
        store.append_review(older.reference(), 1, &review_at(unix(T0 + 4), "backward")),
        Err(StoreError::InvalidReview {
            reason: "assessment-time-regressed"
        })
    ));
    store
        .append_review(older.reference(), 1, &review_at(unix(T0 + 6), "second"))
        .expect("revision 2");
    newer.review = Some(supported_review(unix(T0 + 30), 2));
    let imported = Sample::qualifying(3, T0 - 50);
    append(&mut store, &imported).expect("import");
    drop(store);
    let mut store = reopen(&path);
    let history = store
        .history(bookradar_store::synthetic_offer())
        .expect("history");
    let observation_two = history
        .iter()
        .find(|entry| entry.record().observation().reference().observation().get() == 2)
        .expect("two");
    assert_eq!(observation_two.reviews().len(), 1);
    assert_eq!(
        observation_two.reviews()[0].knowledge(),
        KnowledgeAssessment::Supported
    );
    let observation_one = history
        .iter()
        .find(|entry| entry.record().observation().reference().observation().get() == 1)
        .expect("one");
    assert_eq!(observation_one.reviews().len(), 2);
    assert_eq!(observation_one.reviews()[1].rationale(), "second");
}

#[test]
fn evidence_and_review_failures_are_persisted() {
    let dir = TempDir::new();
    let (_path, mut store) = create(&dir);
    for (id, sample) in failure_samples() {
        append(&mut store, &sample).expect("append");
        let view = common::current_at(&mut store, T0 + INTERVAL);
        assert!(
            view.candidates().iter().any(|candidate| {
                candidate
                    .record()
                    .observation()
                    .reference()
                    .observation()
                    .get()
                    == id
                    && matches!(
                        candidate.assessment().eligibility(),
                        bookradar_core::Eligibility::NotEligible
                    )
            }),
            "observation {id} stayed eligible"
        );
    }
    assert_eq!(
        store
            .history(bookradar_store::synthetic_offer())
            .expect("history")
            .len(),
        9
    );
}

fn failure_samples() -> Vec<(u64, Sample)> {
    let mut samples = Vec::new();
    let mut agent = Sample::qualifying(1, T0);
    agent.evidence = Some(bookradar_core::EvidenceSupport::new(
        Some(bookradar_core::EvidenceReference::new("synthetic:agent").expect("ref")),
        EvidenceMethod::AgentOnly,
        bookradar_core::ClaimCoverage::new(
            ClaimCoverageState::Covered,
            ClaimCoverageState::Covered,
            ClaimCoverageState::Covered,
        ),
    ));
    samples.push((1, agent));
    let mut missing_evidence = Sample::qualifying(2, T0 + 1);
    missing_evidence.evidence = None;
    samples.push((2, missing_evidence));
    let mut missing_reference = Sample::qualifying(3, T0 + 2);
    missing_reference.evidence = Some(bookradar_core::EvidenceSupport::new(
        None,
        EvidenceMethod::Manual,
        bookradar_core::ClaimCoverage::new(
            ClaimCoverageState::Covered,
            ClaimCoverageState::Covered,
            ClaimCoverageState::Covered,
        ),
    ));
    samples.push((3, missing_reference));
    let mut missing_statement = Sample::qualifying(4, T0 + 3);
    missing_statement.statement = None;
    samples.push((4, missing_statement));
    let mut missing_approval = Sample::qualifying(5, T0 + 4);
    missing_approval.review.as_mut().expect("review").approval = None;
    samples.push((5, missing_approval));
    for (offset, (subject, language, availability)) in [
        (
            ClaimCoverageState::Missing,
            ClaimCoverageState::Covered,
            ClaimCoverageState::Covered,
        ),
        (
            ClaimCoverageState::Covered,
            ClaimCoverageState::Missing,
            ClaimCoverageState::Covered,
        ),
        (
            ClaimCoverageState::Covered,
            ClaimCoverageState::Covered,
            ClaimCoverageState::Missing,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let id = 6 + offset as u64;
        let mut sample = Sample::qualifying(id, T0 + 5 + offset as i64);
        sample.evidence = Some(common::coverage(subject, language, availability));
        samples.push((id, sample));
    }
    let mut disputed = Sample::qualifying(9, T0 + 9);
    disputed.review.as_mut().expect("review").knowledge = KnowledgeAssessment::Disputed;
    samples.push((9, disputed));
    samples
}

trait ObservationAccess {
    fn let_observation(&self) -> bookradar_core::Observation;
    fn reference(&self) -> bookradar_core::ObservationRef;
}

impl ObservationAccess for Sample {
    fn let_observation(&self) -> bookradar_core::Observation {
        record(self).expect("record").observation().clone()
    }

    fn reference(&self) -> bookradar_core::ObservationRef {
        self.let_observation().reference()
    }
}

fn review_at(at: SystemTime, rationale: &str) -> ReviewDraft {
    ReviewDraft::try_new(
        KnowledgeAssessment::Rejected,
        Some(bookradar_core::ReviewerApproval::new("synthetic:reviewer").expect("approval")),
        "synthetic:assessor",
        rationale,
        at,
        at,
    )
    .expect("review")
}

#[test]
fn read_only_open_does_not_require_a_second_connection_mode() {
    let dir = TempDir::new();
    let (path, store) = create(&dir);
    drop(store);
    let _ = Store::open(&path, OpenMode::ReadOnly).expect("read only");
    let _ = (
        AccessResult::Readable,
        LanguageEvidence::Slovak,
        ListingDisposition::Present,
    );
    let _ = (
        SourceAvailability::Available,
        SubjectIdentity::Unresolved,
        Fixture::Available,
    );
}
