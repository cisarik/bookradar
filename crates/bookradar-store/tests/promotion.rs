//! Governed promotion and application-store review revisions.

#![forbid(unsafe_code)]

mod common;

use std::fs;
use std::path::PathBuf;

use bookradar_core::{
    AccessResult, ClaimCoverage, ClaimCoverageState, ConflictState, EditionId, Eligibility,
    EvidenceMethod, EvidenceReference, EvidenceSupport, KnowledgeAssessment, LanguageEvidence,
    ListingDisposition, ObservationId, ObservationRef, ReviewerApproval, SourceAvailability,
    SubjectIdentity,
};
use bookradar_store::{
    CandidateQuery, ImportContext, OpenMode, PromotionRequest, ResearchStore, ReviewDraft, Store,
    append_application_review, promote, render_current_labels,
};

use common::{Sample, SampleReview, TempDir, append, offer, unix};

const OBSERVED: i64 = 1_793_523_600;

#[test]
fn promote_reviewer_attested_candidate_is_checked_available() {
    let dir = TempDir::new();
    let ledger_path = import_fixture(&dir, "newer.json");
    let mut ledger = ResearchStore::open(&ledger_path, OpenMode::ReadOnly).expect("ledger");
    let store_path = create_store(&dir);
    let request = request(1, OBSERVED);
    promote(&mut ledger, &store_path, &request).expect("promote");
    drop(ledger);

    let mut store = Store::open(&store_path, OpenMode::ReadOnly).expect("store");
    let history = store.history(offer(1, 1)).expect("history");
    assert_eq!(history.len(), 1);
    let record = history[0].record();
    assert_eq!(
        record.provenance_reference(),
        "research:synthetic:producer/synthetic:run/synthetic:newer@1/0"
    );
    assert_eq!(record.observation().language(), LanguageEvidence::Slovak);
    assert_eq!(
        record.observation().evidence().expect("evidence").method(),
        EvidenceMethod::Manual
    );
    assert_eq!(
        record
            .observation()
            .source_statement()
            .expect("statement")
            .as_str(),
        "reviewer:statement"
    );
    assert!(record.requested_url().is_none());
    assert_eq!(history[0].reviews().len(), 1);
    assert_eq!(
        history[0].reviews()[0].knowledge(),
        KnowledgeAssessment::Supported
    );
    let now = unix(OBSERVED + 1_000);
    let view = store
        .current(offer(1, 1), EditionId::new(10).expect("edition"), now)
        .expect("current");
    assert!(matches!(
        view.candidates()[0].assessment().eligibility(),
        Eligibility::Eligible { .. }
    ));
    let labels = render_current_labels(&view, EditionId::new(10).expect("edition"), now);
    assert!(labels.iter().any(|line| line.contains("Checked available")));
}

#[test]
fn library_refuses_agent_only_even_with_approval() {
    let dir = TempDir::new();
    let ledger_path = import_fixture(&dir, "newer.json");
    let mut ledger = ResearchStore::open(&ledger_path, OpenMode::ReadOnly).expect("ledger");
    let store_path = dir.child("absent.sqlite");
    let mut request = request(1, OBSERVED);
    request.evidence_method = EvidenceMethod::AgentOnly;
    let error = promote(&mut ledger, &store_path, &request).expect_err("refused");
    assert_eq!(error.category(), "agent-only-evidence");
    assert!(!store_path.exists());
}

#[test]
fn incomplete_attestation_writes_nothing() {
    let dir = TempDir::new();
    let ledger_path = import_fixture(&dir, "newer.json");
    let cases = [
        request_without_approval(),
        request_without_coverage(),
        request_with_language(LanguageEvidence::Czech),
        request_with_subject(SubjectIdentity::Unresolved),
    ];
    for (index, request) in cases.into_iter().enumerate() {
        let mut ledger = ResearchStore::open(&ledger_path, OpenMode::ReadOnly).expect("ledger");
        let name = format!("absent-{index}.sqlite");
        let store_path = dir.child(&name);
        let error = promote(&mut ledger, &store_path, &request).expect_err("refused");
        assert_eq!(error.category(), "incomplete-attestation", "{index}");
        assert!(!store_path.exists(), "{index}");
    }
}

#[test]
fn stale_observation_is_stored_but_not_eligible() {
    let dir = TempDir::new();
    let ledger_path = import_fixture(&dir, "newer.json");
    let mut ledger = ResearchStore::open(&ledger_path, OpenMode::ReadOnly).expect("ledger");
    let store_path = create_store(&dir);
    promote(&mut ledger, &store_path, &request(1, OBSERVED)).expect("promote");
    let mut store = Store::open(&store_path, OpenMode::ReadOnly).expect("store");
    let now = unix(OBSERVED + 86_400);
    let target = EditionId::new(10).expect("edition");
    let view = store.current(offer(1, 1), target, now).expect("current");
    assert!(matches!(
        view.candidates()[0].assessment().eligibility(),
        Eligibility::NotEligible
    ));
    let labels = render_current_labels(&view, target, now);
    assert!(
        labels
            .iter()
            .all(|line| !line.contains("Checked available"))
    );
    assert_eq!(store.history(offer(1, 1)).expect("history").len(), 1);
}

#[test]
fn rejected_quarantined_replayed_and_unknown_write_nothing() {
    let dir = TempDir::new();
    let ledger_path = dir.child("ledger.sqlite");
    let mut ledger = ResearchStore::create(&ledger_path).expect("create");
    import_bytes(&mut ledger, &fixture("partial.json"), 100);
    let mut replay = fixture("newer.json");
    import_bytes(&mut ledger, &replay, 101);
    replace_once(&mut replay, b"\"succeeded\"", b"\"failed\"");
    import_bytes(&mut ledger, &replay, 102);
    drop(ledger);

    let store_path = dir.child("absent.sqlite");
    let cases = [
        (query("F1", 1, 1), "unknown-finding"),
        (query("F3", 1, 2), "not-admitted"),
        (query("synthetic:newer", 3, 0), "not-admitted"),
        (query("synthetic:missing", 9, 0), "unknown-finding"),
    ];
    for (index, (query, category)) in cases.into_iter().enumerate() {
        let mut ledger = ResearchStore::open(&ledger_path, OpenMode::ReadOnly).expect("ledger");
        let mut request = request(1, OBSERVED);
        request.query = query;
        let error = promote(&mut ledger, &store_path, &request).expect_err("refused");
        assert_eq!(error.category(), category, "{index}");
        assert!(!store_path.exists(), "{index}");
    }
}

#[test]
fn duplicate_observation_id_rolls_back() {
    let dir = TempDir::new();
    let ledger_path = import_fixture(&dir, "newer.json");
    let store_path = create_store(&dir);
    let mut ledger = ResearchStore::open(&ledger_path, OpenMode::ReadOnly).expect("ledger");
    promote(&mut ledger, &store_path, &request(1, OBSERVED)).expect("first");
    let error = promote(&mut ledger, &store_path, &request(1, OBSERVED)).expect_err("duplicate");
    assert_eq!(error.category(), "duplicate-key");
    let mut store = Store::open(&store_path, OpenMode::ReadOnly).expect("store");
    let history = store.history(offer(1, 1)).expect("history");
    assert_eq!(history.len(), 1);
    assert_eq!(
        history[0].record().provenance_reference(),
        "research:synthetic:producer/synthetic:run/synthetic:newer@1/0"
    );
    drop(store);
    promote(&mut ledger, &store_path, &request(2, OBSERVED)).expect("second id");
    let mut store = Store::open(&store_path, OpenMode::ReadOnly).expect("reopen");
    assert_eq!(store.history(offer(1, 1)).expect("history").len(), 2);
}

#[test]
fn ledger_bytes_unchanged_by_show_and_promote() {
    let dir = TempDir::new();
    let ledger_path = dir.child("ledger.sqlite");
    let mut ledger = ResearchStore::create(&ledger_path).expect("create");
    import_bytes(&mut ledger, &fixture("partial.json"), 100);
    import_bytes(&mut ledger, &fixture("conflict.json"), 101);
    let before = ledger.inspect().expect("before");
    drop(ledger);
    let bytes_before = fs::read(&ledger_path).expect("bytes");

    let mut ledger = ResearchStore::open(&ledger_path, OpenMode::ReadOnly).expect("ledger");
    let candidate = ledger
        .candidate(&query("F1", 1, 0))
        .expect("candidate")
        .expect("row");
    assert_eq!(candidate.outcome, "admitted");
    assert_eq!(candidate.ledger_knowledge.as_deref(), Some("disputed"));
    let store_path = create_store(&dir);
    let mut request = request(1, OBSERVED);
    request.query = query("F1", 1, 0);
    promote(&mut ledger, &store_path, &request).expect("promote disputed admitted finding");
    let after = ledger.inspect().expect("after");
    drop(ledger);
    assert_eq!(before.inputs, after.inputs);
    assert_eq!(before.findings_rows, after.findings_rows);
    assert_eq!(before.runs, after.runs);
    assert_eq!(fs::read(&ledger_path).expect("bytes after"), bytes_before);

    let mut store = Store::open(&store_path, OpenMode::ReadOnly).expect("store");
    let now = unix(OBSERVED + 1_000);
    let view = store
        .current(offer(1, 1), EditionId::new(10).expect("edition"), now)
        .expect("current");
    assert!(matches!(
        view.candidates()[0].assessment().eligibility(),
        Eligibility::Eligible { .. }
    ));
}

#[test]
fn rejected_revision_clears_equal_time_conflict() {
    let dir = TempDir::new();
    let (path, mut store) = common::create(&dir);
    let available = Sample::qualifying(1, OBSERVED);
    let mut unavailable = Sample::qualifying(2, OBSERVED);
    unavailable.availability = SourceAvailability::Unavailable;
    unavailable.statement = Some("reviewer:unavailable".to_owned());
    append(&mut store, &available).expect("available");
    append(&mut store, &unavailable).expect("unavailable");
    let before = store
        .current(
            offer(1, 1),
            EditionId::new(10).expect("edition"),
            unix(OBSERVED + 20),
        )
        .expect("before");
    assert_eq!(before.conflict(), ConflictState::Unresolved);
    let revision = append_application_review(
        &mut store,
        observation_ref(&unavailable),
        1,
        &review_at(KnowledgeAssessment::Rejected, None, OBSERVED + 30),
    )
    .expect("reject");
    assert_eq!(revision, 2);
    let after = store
        .current(
            offer(1, 1),
            EditionId::new(10).expect("edition"),
            unix(OBSERVED + 40),
        )
        .expect("after");
    assert_eq!(after.conflict(), ConflictState::Clear);
    let history = store.history(offer(1, 1)).expect("history");
    let rejected = history
        .iter()
        .find(|entry| entry.record().observation().reference().observation().get() == 2)
        .expect("observation 2");
    assert_eq!(rejected.reviews().len(), 2);
    assert_eq!(
        rejected.reviews()[0].knowledge(),
        KnowledgeAssessment::Supported
    );
    assert_eq!(
        rejected.reviews()[1].knowledge(),
        KnowledgeAssessment::Rejected
    );
    drop(store);
    assert!(path.exists());
}

#[test]
fn supported_revision_supersedes_disputed_observation() {
    let dir = TempDir::new();
    let (_path, mut store) = common::create(&dir);
    let mut sample = Sample::qualifying(1, OBSERVED);
    sample.review = Some(SampleReview {
        knowledge: KnowledgeAssessment::Disputed,
        approval: None,
        assessor: "reviewer:assessor".to_owned(),
        rationale: "reviewer:dispute".to_owned(),
        assessed: unix(OBSERVED + 10),
        recorded: unix(OBSERVED + 10),
    });
    append(&mut store, &sample).expect("append");
    let before = store
        .current(
            offer(1, 1),
            EditionId::new(10).expect("edition"),
            unix(OBSERVED + 20),
        )
        .expect("before");
    assert_eq!(before.conflict(), ConflictState::Unresolved);
    append_application_review(
        &mut store,
        observation_ref(&sample),
        1,
        &review_at(
            KnowledgeAssessment::Supported,
            Some("reviewer:approval"),
            OBSERVED + 30,
        ),
    )
    .expect("support");
    let after = store
        .current(
            offer(1, 1),
            EditionId::new(10).expect("edition"),
            unix(OBSERVED + 40),
        )
        .expect("after");
    assert_eq!(after.conflict(), ConflictState::Clear);
    let history = store.history(offer(1, 1)).expect("history");
    assert_eq!(history[0].reviews().len(), 2);
    assert_eq!(
        history[0].reviews()[0].knowledge(),
        KnowledgeAssessment::Disputed
    );
    assert_eq!(
        history[0].reviews()[1].knowledge(),
        KnowledgeAssessment::Supported
    );
    assert!(history[0].reviews()[0].revision() < history[0].reviews()[1].revision());
}

#[test]
fn wrong_expected_revision_is_review_conflict() {
    let dir = TempDir::new();
    let (_path, mut store) = common::create(&dir);
    let sample = Sample::qualifying(1, OBSERVED);
    append(&mut store, &sample).expect("append");
    let error = append_application_review(
        &mut store,
        observation_ref(&sample),
        0,
        &review_at(KnowledgeAssessment::Rejected, None, OBSERVED + 30),
    )
    .expect_err("conflict");
    assert_eq!(error.category(), "review-conflict");
    assert_eq!(
        store.history(offer(1, 1)).expect("history")[0]
            .reviews()
            .len(),
        1
    );
}

#[test]
fn regressed_assessed_at_is_storage() {
    let dir = TempDir::new();
    let (_path, mut store) = common::create(&dir);
    let sample = Sample::qualifying(1, OBSERVED);
    append(&mut store, &sample).expect("append");
    let error = append_application_review(
        &mut store,
        observation_ref(&sample),
        1,
        &review_at(KnowledgeAssessment::Rejected, None, OBSERVED + 9),
    )
    .expect_err("regressed");
    assert_eq!(error.category(), "storage");
    assert!(error.to_string().contains("assessment-time-regressed"));
    assert_eq!(
        store.history(offer(1, 1)).expect("history")[0]
            .reviews()
            .len(),
        1
    );
}

#[test]
fn supported_review_keeps_agent_only_not_eligible() {
    let dir = TempDir::new();
    let (_path, mut store) = common::create(&dir);
    let mut sample = Sample::qualifying(1, OBSERVED);
    let covered = ClaimCoverageState::Covered;
    sample.evidence = Some(EvidenceSupport::new(
        Some(EvidenceReference::new("synthetic:agent").expect("reference")),
        EvidenceMethod::AgentOnly,
        ClaimCoverage::new(covered, covered, covered),
    ));
    sample.review = None;
    append(&mut store, &sample).expect("append");
    append_application_review(
        &mut store,
        observation_ref(&sample),
        0,
        &review_at(
            KnowledgeAssessment::Supported,
            Some("reviewer:approval"),
            OBSERVED + 20,
        ),
    )
    .expect("review");
    let now = unix(OBSERVED + 30);
    let target = EditionId::new(10).expect("edition");
    let view = store.current(offer(1, 1), target, now).expect("current");
    assert!(matches!(
        view.candidates()[0].assessment().eligibility(),
        Eligibility::NotEligible
    ));
    let labels = render_current_labels(&view, target, now);
    assert!(
        labels
            .iter()
            .all(|line| !line.contains("Checked available"))
    );
}

fn request(id: u64, observed: i64) -> PromotionRequest {
    PromotionRequest {
        query: query("synthetic:newer", 1, 0),
        source_id: 1,
        offer_id: 1,
        observation_id: id,
        subject: SubjectIdentity::Resolved(EditionId::new(10).expect("edition")),
        language: LanguageEvidence::Slovak,
        access: AccessResult::Readable,
        listing: ListingDisposition::Present,
        availability: SourceAvailability::Available,
        source_statement: "reviewer:statement".to_owned(),
        evidence_method: EvidenceMethod::Manual,
        evidence_reference: "reviewer:evidence".to_owned(),
        cover_subject: true,
        cover_language: true,
        cover_availability: true,
        reviewer_approval: Some("reviewer:approval".to_owned()),
        assessor: "reviewer:assessor".to_owned(),
        rationale: "reviewer:rationale".to_owned(),
        observed_at: unix(observed),
        observed_offset: 3_600,
        captured_at: unix(observed),
        captured_offset: 3_600,
        ingested_at: unix(observed + 10),
        assessed_at: unix(observed + 10),
        recorded_at: unix(observed + 10),
    }
}

fn request_without_approval() -> PromotionRequest {
    let mut request = request(1, OBSERVED);
    request.reviewer_approval = None;
    request
}

fn request_without_coverage() -> PromotionRequest {
    let mut request = request(1, OBSERVED);
    request.cover_language = false;
    request
}

fn request_with_language(language: LanguageEvidence) -> PromotionRequest {
    let mut request = request(1, OBSERVED);
    request.language = language;
    request
}

fn request_with_subject(subject: SubjectIdentity) -> PromotionRequest {
    let mut request = request(1, OBSERVED);
    request.subject = subject;
    request
}

fn query(finding: &str, receipt: u64, position: u64) -> CandidateQuery {
    CandidateQuery {
        producer_id: "synthetic:producer".to_owned(),
        run_id: "synthetic:run".to_owned(),
        finding_id: finding.to_owned(),
        receipt_id: receipt,
        position,
    }
}

fn review_at(knowledge: KnowledgeAssessment, approval: Option<&str>, at: i64) -> ReviewDraft {
    ReviewDraft::try_new(
        knowledge,
        approval.map(|text| ReviewerApproval::new(text).expect("approval")),
        "reviewer:assessor",
        "reviewer:later",
        unix(at),
        unix(at),
    )
    .expect("draft")
}

fn import_fixture(dir: &TempDir, name: &str) -> PathBuf {
    let path = dir.child("ledger.sqlite");
    let mut ledger = ResearchStore::create(&path).expect("create");
    import_bytes(&mut ledger, &fixture(name), 100);
    drop(ledger);
    path
}

fn import_bytes(ledger: &mut ResearchStore, bytes: &[u8], at: i64) {
    ledger
        .import(bytes, ImportContext::new(unix(at)))
        .unwrap_or_else(|error| panic!("import: {error}"));
}

fn fixture(name: &str) -> Vec<u8> {
    fs::read(fixture_path(name)).unwrap_or_else(|error| panic!("fixture {name}: {error}"))
}

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/research")
        .join(name)
}

fn create_store(dir: &TempDir) -> PathBuf {
    let path = dir.child("store.sqlite");
    drop(Store::create(&path).expect("create store"));
    path
}

fn observation_ref(sample: &Sample) -> ObservationRef {
    ObservationRef::new(
        offer(sample.source, sample.offer_id),
        ObservationId::new(sample.id).expect("id"),
    )
}

fn replace_once(bytes: &mut Vec<u8>, from: &[u8], to: &[u8]) {
    let start = bytes
        .windows(from.len())
        .position(|window| window == from)
        .expect("pattern");
    bytes.splice(start..start + from.len(), to.iter().copied());
}
