//! Presentation of loaded observation and research snapshots.

#![forbid(unsafe_code)]

mod common;

use std::fs;
use std::time::{Duration, SystemTime};

use bookradar_core::{
    AccessResult, ClaimCoverage, ClaimCoverageState, EvidenceMethod, EvidenceReference,
    EvidenceSupport, KnowledgeAssessment, LanguageEvidence, ListingDisposition, SourceAvailability,
    SubjectIdentity, TimestampAssertion,
};
use bookradar_store::{
    CandidateQuery, Fixture, OpenMode, PromotionRequest, ResearchStore, ReviewDraft, Store,
    append_fixture, promote, synthetic_offer,
};
use bookradar_ui::render_html;

use common::{
    INTERVAL, T0, TempDir, append, config_at, create_ledger, create_store, import_bytes, load_at,
    manual_evidence, patch_be_u32, research_fixture, section, unix, unix_nanos,
};

const SOLD: &[&str] = &["Sold out", "In stock"];

#[test]
fn renders_both_stores_and_pinned_context() {
    let dir = TempDir::new();
    let (store_path, mut store) = create_store(&dir, "observations.sqlite");
    for fixture in [
        Fixture::Available,
        Fixture::Blocked,
        Fixture::Recovered,
        Fixture::Older,
    ] {
        append_fixture(&mut store, fixture).expect("fixture");
    }
    drop(store);
    let (ledger_path, mut ledger) = create_ledger(&dir, "research.sqlite");
    let partial = research_fixture("partial.json");
    import_bytes(&mut ledger, &partial, 1_793_545_201);
    import_bytes(&mut ledger, &partial, 1_793_545_202);
    import_bytes(
        &mut ledger,
        &research_fixture("conflict.json"),
        1_793_545_203,
    );
    drop(ledger);

    let html = render_html(&load_at(
        &store_path,
        &ledger_path,
        unix(1_793_548_800),
        1_793_548_800,
    ));
    assert!(html.contains("<h1>BookRadar</h1>"));
    assert!(html.contains("Synthetic data · Local · Read-only"));
    assert!(html.contains("Evaluation instant — Unix 1793548800"));
    assert!(html.contains("remain fixed until restart"));
    for id in [
        "current-assessment",
        "observation-history",
        "research-ledger",
        "research-current-candidates",
    ] {
        assert!(html.contains(&format!("id=\"{id}\"")), "{id}");
    }
    assert!(html.contains("source 1"));
    assert!(html.contains("offer 1"));
    assert!(html.contains("synthetic:producer"));
    assert!(html.contains("synthetic:source"));
    assert!(html.contains("OFFER-A"));
    assert!(html.contains("generation-1"));
    assert!(section(&html, "observation-history").contains("Observation 4"));
    assert!(section(&html, "current-assessment").contains("Candidate observations"));
    assert!(
        section(&html, "current-assessment").contains(">3<")
            || section(&html, "current-assessment").contains("Candidate 3")
            || section(&html, "current-assessment").contains(">3</")
    );
    let current = section(&html, "current-assessment");
    assert!(current.contains("3"), "{current}");
    let ledger_html = section(&html, "research-ledger");
    assert!(ledger_html.contains("Envelopes"));
    assert!(ledger_html.contains(">2<") || ledger_html.contains("2"));
    assert!(ledger_html.contains("Runs"));
    assert!(ledger_html.contains("Findings"));
    assert!(ledger_html.contains("Projected observations"));
    assert!(ledger_html.contains("Reviews"));
    assert!(ledger_html.contains("<dd class=\"preserve\">2</dd>"));
    assert!(ledger_html.contains("<dd class=\"preserve\">1</dd>"));
    assert!(ledger_html.contains("<dd class=\"preserve\">4</dd>"));
    assert!(ledger_html.contains("<dd class=\"preserve\">0</dd>"));
    assert_no_stock(&html);
}

#[test]
fn renders_every_current_truth_label() {
    let fresh = page_at(&[SampleSpec::Qualifying(1, T0)], nanos_before_expiry());
    assert!(fresh.contains("Checked available — Unix 1793523600; expires at Unix 1793610000"));
    assert!(!fresh.contains("Last observed available"));

    let expired = page_at(&[SampleSpec::Qualifying(1, T0)], unix(T0 + INTERVAL));
    assert!(expired.contains("Last observed available — Unix 1793523600"));
    assert!(!expired.contains("Checked available"));

    let reserved = page_at(
        &[
            SampleSpec::Qualifying(1, T0),
            SampleSpec::Reserved(2, T0 + 3_600),
        ],
        unix(T0 + 7_200),
    );
    assert!(reserved.contains("Last observed reserved — Unix 1793527200"));

    let unavailable = page_at(
        &[
            SampleSpec::Qualifying(1, T0),
            SampleSpec::Unavailable(2, T0 + 3_600),
        ],
        unix(T0 + 7_200),
    );
    assert!(unavailable.contains("Last observed unavailable — Unix 1793527200"));

    let blocked = page_at(
        &[
            SampleSpec::Qualifying(1, T0),
            SampleSpec::Failure(2, T0 + 3_600, AccessResult::RateLimited),
        ],
        unix(T0 + 7_200),
    );
    assert!(blocked.contains("Could not recheck — Unix 1793527200"));
    assert!(blocked.contains("Last observed available — Unix 1793523600"));

    let removed = page_at(
        &[
            SampleSpec::Qualifying(1, T0),
            SampleSpec::Failure(2, T0 + 3_600, AccessResult::PageGone),
        ],
        unix(T0 + 7_200),
    );
    assert!(removed.contains("Listing removed — Unix 1793527200"));
    assert!(removed.contains("Last observed available — Unix 1793523600"));

    let conflict = page_at(
        &[
            SampleSpec::Qualifying(1, T0),
            SampleSpec::Unavailable(2, T0),
        ],
        unix(T0 + 10),
    );
    assert!(conflict.contains("Unverified — unresolved conflict"));
    assert!(!conflict.contains("Last observed"));

    let unordered = page_at(&[SampleSpec::UnknownTime(1)], unix(T0 + 10));
    assert!(unordered.contains("Unverified — observation order unknown"));
    assert!(!unordered.contains("Checked available"));

    let future = page_at(
        &[
            SampleSpec::Qualifying(1, T0),
            SampleSpec::Qualifying(2, T0 + 10_000),
        ],
        unix(T0 + 100),
    );
    assert!(future.contains("Unverified — observation time in future"));
    assert!(!future.contains("Checked available"));
    assert!(!future.contains("Last observed available"));

    let offset = page_at(&[SampleSpec::UnknownOffset(1, T0)], unix(T0 + 10));
    assert!(offset.contains("Unverified — observation offset unknown"));

    let unknown_availability = page_at(&[SampleSpec::UnknownAvailability(1, T0)], unix(T0 + 60));
    assert!(unknown_availability.contains("Unverified — source availability unknown"));

    let insufficient = page_at(&[SampleSpec::UnreviewedUnavailable(1, T0)], unix(T0 + 10));
    assert!(insufficient.contains("Unverified — insufficient support"));
    assert!(!insufficient.contains("Last observed unavailable"));

    let far = far_instant().expect("representable far instant");
    let time_unknown = page_at(&[SampleSpec::Far(1, far)], far);
    assert!(
        time_unknown.contains("Unverified — observation time unknown"),
        "{time_unknown}"
    );

    for page in [
        &fresh,
        &expired,
        &reserved,
        &unavailable,
        &blocked,
        &removed,
        &conflict,
        &unordered,
        &future,
        &offset,
        &unknown_availability,
        &insufficient,
        &time_unknown,
    ] {
        assert_no_stock(page);
    }
}

#[test]
fn preserves_unknown_time_identity_language_and_review_details() {
    let dir = TempDir::new();
    let (store_path, mut store) = create_store(&dir, "observations.sqlite");
    let mut pre_epoch = common::Sample::qualifying(1, T0);
    pre_epoch.subject = SubjectIdentity::Unresolved;
    pre_epoch.language = LanguageEvidence::Czech;
    pre_epoch.observed =
        TimestampAssertion::known(Some("synthetic:pre-epoch"), 0, unix_nanos(-1, 500_000_000))
            .expect("pre-epoch");
    pre_epoch.captured =
        TimestampAssertion::known(Some("synthetic:captured-nanos"), 3_600, unix_nanos(T0, 123))
            .expect("captured");
    pre_epoch.evidence = Some(EvidenceSupport::new(
        Some(EvidenceReference::new("synthetic:partial-evidence").expect("evidence")),
        EvidenceMethod::Manual,
        ClaimCoverage::new(
            ClaimCoverageState::Missing,
            ClaimCoverageState::Covered,
            ClaimCoverageState::Missing,
        ),
    ));
    pre_epoch.uncertainty = Some("synthetic:uncertainty".to_owned());
    pre_epoch.requested_url = Some("synthetic:requested".to_owned());
    pre_epoch.resolved_url = Some("synthetic:resolved".to_owned());
    pre_epoch.redirect = Some("synthetic:redirect".to_owned());
    append(&mut store, &pre_epoch).expect("pre-epoch");
    let second = ReviewDraft::try_new(
        KnowledgeAssessment::Disputed,
        None,
        "synthetic:second-assessor",
        "synthetic:second-rationale",
        unix(T0 + 30),
        unix(T0 + 40),
    )
    .expect("review");
    store
        .append_review(pre_epoch.reference(), 1, &second)
        .expect("second review");

    let mut offset = common::Sample::qualifying(2, T0 + 10);
    offset.observed = TimestampAssertion::with_unknown_offset(
        Some("synthetic:offset-original"),
        Some(unix(T0 + 10)),
    )
    .expect("offset");
    offset.language = LanguageEvidence::Unknown;
    offset.subject = SubjectIdentity::Conflicting;
    offset.review = None;
    append(&mut store, &offset).expect("offset");

    let mut absent = common::Sample::qualifying(3, T0 + 20);
    absent.observed = TimestampAssertion::unknown();
    absent.captured = TimestampAssertion::unknown();
    append(&mut store, &absent).expect("absent");
    drop(store);

    let (ledger_path, _ledger) = create_ledger(&dir, "research.sqlite");
    let html = render_html(&load_at(&store_path, &ledger_path, unix(T0 + 50), T0 + 50));
    let history = section(&html, "observation-history");
    assert!(history.contains("synthetic:pre-epoch"));
    assert!(history.contains("known, 0 seconds east of UTC"));
    assert!(history.contains("Unix -1.500000000"));
    assert!(history.contains("Signed seconds"));
    assert!(history.contains("-1"));
    assert!(history.contains("500000000"));
    assert!(history.contains("synthetic:captured-nanos"));
    assert!(history.contains("000000123") || history.contains("123"));
    assert!(history.contains("unresolved"));
    assert!(history.contains("conflicting"));
    assert!(history.contains("czech"));
    assert!(history.contains("unknown"));
    assert!(history.contains("missing"));
    assert!(history.contains("covered"));
    assert!(history.contains("synthetic:uncertainty"));
    assert!(history.contains("synthetic:requested"));
    assert!(history.contains("synthetic:resolved"));
    assert!(history.contains("synthetic:redirect"));
    assert!(history.contains("Revision 1"));
    assert!(history.contains("Revision 2"));
    assert!(history.contains("synthetic:rationale:1"));
    assert!(history.contains("synthetic:second-rationale"));
    assert!(history.contains("synthetic:reviewer"));
    assert!(history.contains("not recorded"));
    assert!(history.contains("synthetic:second-assessor"));
    assert!(history.contains("No review recorded."));
    assert!(history.contains("synthetic:offset-original"));
    assert!(history.contains("Ingestion time:"));
    assert!(history.contains("Source-reported availability"));
}

#[test]
fn escapes_hostile_observation_and_research_text() {
    let hostile = r#"<script>alert("xss")</script> & 'cmd'"#;
    let dir = TempDir::new();
    let (store_path, mut store) = create_store(&dir, "observations.sqlite");
    let mut sample = common::Sample::qualifying(1, T0);
    sample.statement = Some(hostile.to_owned());
    sample.provenance = hostile.to_owned();
    sample.uncertainty = Some(hostile.to_owned());
    sample.requested_url = Some(hostile.to_owned());
    sample.resolved_url = Some(format!("https://evil.example/{hostile}"));
    sample.redirect = Some(hostile.to_owned());
    sample.evidence = Some(manual_evidence(hostile));
    sample.review.as_mut().expect("review").rationale = hostile.to_owned();
    append(&mut store, &sample).expect("append");
    drop(store);

    let (ledger_path, mut ledger) = create_ledger(&dir, "research.sqlite");
    import_bytes(&mut ledger, &hostile_research(hostile), 1_793_545_201);
    drop(ledger);
    let html = render_html(&load_at(&store_path, &ledger_path, unix(T0 + 10), T0 + 10));
    assert!(!html.contains("<script"));
    assert!(!html.contains("<img"));
    assert!(
        html.contains("&lt;script&gt;alert(&quot;xss&quot;)&lt;/script&gt; &amp; &#39;cmd&#39;")
    );
    assert!(!html.contains("href=\"https://evil.example"));
    assert!(!html.contains("href=\"&lt;script"));
    let research = section(&html, "research-current-candidates");
    assert!(research.contains("&lt;script&gt;"));
    assert!(!research.contains("<script"));
}

#[test]
fn renders_research_outcomes_without_promoting_candidates() {
    let dir = TempDir::new();
    let (store_path, _store) = create_store(&dir, "observations.sqlite");
    let (ledger_path, mut ledger) = create_ledger(&dir, "research.sqlite");
    let partial = research_fixture("partial.json");
    import_bytes(&mut ledger, &partial, 100);
    import_bytes(&mut ledger, &partial, 101);
    let changed = String::from_utf8(partial.clone()).expect("utf8").replace(
        "\"reported_run_outcome\": \"partial\"",
        "\"reported_run_outcome\": \"failed\"",
    );
    import_bytes(&mut ledger, changed.as_bytes(), 102);
    drop(ledger);

    let unreviewed = render_html(&load_at(
        &store_path,
        &ledger_path,
        unix(1_793_548_800),
        1_793_548_800,
    ));
    let ledger_html = section(&unreviewed, "research-ledger");
    assert!(ledger_html.contains("admitted"));
    assert!(ledger_html.contains("replayed"));
    assert!(ledger_html.contains("rejected"));
    assert!(ledger_html.contains("quarantined"));
    assert!(ledger_html.contains("not projected"));
    assert!(
        ledger_html.contains("unknown")
            || ledger_html.contains("partial")
            || ledger_html.contains("failed")
    );
    let candidates = section(&unreviewed, "research-current-candidates");
    assert!(candidates.contains("Research candidate — unreviewed"));
    assert!(candidates.contains("Unverified"));
    assert!(candidates.contains("agent-only"));
    assert!(candidates.contains("not-eligible"));
    assert!(candidates.contains("not recorded"));
    assert!(!candidates.contains("Checked available"));
    assert!(!candidates.contains("Last observed"));

    let mut ledger = ResearchStore::open(&ledger_path, OpenMode::ReadWrite).expect("reopen");
    import_bytes(&mut ledger, &research_fixture("conflict.json"), 103);
    drop(ledger);
    let disputed = render_html(&load_at(
        &store_path,
        &ledger_path,
        unix(1_793_548_800),
        1_793_548_800,
    ));
    let disputed_candidates = section(&disputed, "research-current-candidates");
    assert!(disputed_candidates.contains("Research candidate — disputed"));
    assert!(disputed_candidates.contains("unresolved"));
    assert!(disputed_candidates.contains("not-eligible"));
    assert!(!disputed_candidates.contains("Checked available"));

    let mut unexpected = load_at(&store_path, &ledger_path, unix(T0 + 10), T0 + 10);
    let eligible = eligible_view();
    if let bookradar_ui::Slot::Ready(reading) = &mut unexpected.research {
        reading.current = eligible;
    }
    let suppressed = render_html(&unexpected);
    let suppressed_candidates = section(&suppressed, "research-current-candidates");
    assert!(
        suppressed_candidates
            .contains("Unexpected research assessment — verification display suppressed")
    );
    assert!(!suppressed_candidates.contains("Checked available"));
    assert!(!suppressed_candidates.contains("Research candidate —"));
}

#[test]
fn distinguishes_empty_missing_and_no_candidate_states() {
    let dir = TempDir::new();
    let (store_path, _store) = create_store(&dir, "observations.sqlite");
    let (ledger_path, _ledger) = create_ledger(&dir, "research.sqlite");
    let empty = render_html(&load_at(&store_path, &ledger_path, unix(T0), T0));
    assert!(section(&empty, "current-assessment").contains("No observations for this offer."));
    assert!(section(&empty, "observation-history").contains("No observations for this offer."));
    assert!(section(&empty, "research-ledger").contains("Research ledger is empty."));
    assert!(section(&empty, "research-ledger").contains(">0<"));
    assert!(
        section(&empty, "research-current-candidates")
            .contains("No research candidates for this query.")
    );
    assert!(!section(&empty, "current-assessment").contains("Checked available"));

    let missing_store = dir.child("missing-observations.sqlite");
    let missing_ledger = dir.child("missing-research.sqlite");
    let missing = render_html(&load_at(&missing_store, &missing_ledger, unix(T0), T0));
    assert!(missing.contains("Database not found (not-found)."));
    assert!(!missing_store.exists());
    assert!(!missing_ledger.exists());
    assert_eq!(
        missing.matches("Database not found (not-found).").count(),
        4
    );

    let (healthy_store, mut store) = create_store(&dir, "healthy.sqlite");
    append(&mut store, &common::Sample::qualifying(1, T0)).expect("append");
    drop(store);
    let (populated, mut ledger) = create_ledger(&dir, "populated.sqlite");
    import_bytes(&mut ledger, &research_fixture("partial.json"), 100);
    drop(ledger);
    let mut config = config_at(
        &healthy_store,
        &populated,
        unix(1_793_548_800),
        1_793_548_800,
    );
    config.listing_ref = "OTHER-OFFER".to_owned();
    let html = render_html(&bookradar_ui::load(&config));
    assert!(section(&html, "current-assessment").contains("Checked available"));
    assert!(section(&html, "research-ledger").contains("admitted"));
    assert!(
        section(&html, "research-current-candidates")
            .contains("No research candidates for this query.")
    );
    assert!(!section(&html, "research-current-candidates").contains("Research candidate —"));
}

#[test]
fn renders_corrupt_swapped_and_unsupported_schema_errors() {
    let dir = TempDir::new();
    let (store_path, _store) = create_store(&dir, "observations.sqlite");
    let (ledger_path, _ledger) = create_ledger(&dir, "research.sqlite");
    let store_bytes = fs::read(&store_path).expect("store bytes");
    let ledger_bytes = fs::read(&ledger_path).expect("ledger bytes");

    let corrupt = dir.child("corrupt.sqlite");
    fs::write(&corrupt, b"not a database").expect("corrupt");
    let corrupt_before = fs::read(&corrupt).expect("corrupt bytes");
    let version0 = dir.child("version0.sqlite");
    fs::copy(&store_path, &version0).expect("copy");
    patch_be_u32(&version0, 60, 0);
    let version2 = dir.child("version2.sqlite");
    fs::copy(&ledger_path, &version2).expect("copy");
    patch_be_u32(&version2, 60, 2);
    let wrong_id = dir.child("wrong-id.sqlite");
    fs::copy(&store_path, &wrong_id).expect("copy");
    patch_be_u32(&wrong_id, 68, 1);

    let healthy_ledger = dir.child("healthy-ledger.sqlite");
    fs::copy(&ledger_path, &healthy_ledger).expect("copy");
    let corrupt_page = render_html(&load_at(&corrupt, &healthy_ledger, unix(T0), T0));
    assert!(corrupt_page.contains("Unsupported database (unsupported-schema: corrupt-database)."));
    assert!(section(&corrupt_page, "research-ledger").contains("Research ledger is empty."));
    assert_eq!(fs::read(&corrupt).expect("after"), corrupt_before);

    let version_page = render_html(&load_at(&version0, &version2, unix(T0), T0));
    assert!(version_page.contains("Unsupported database (unsupported-schema: user-version)."));
    assert_eq!(
        version_page
            .matches("unsupported-schema: user-version")
            .count(),
        4
    );

    let id_page = render_html(&load_at(&wrong_id, &ledger_path, unix(T0), T0));
    assert!(id_page.contains("Unsupported database (unsupported-schema: application-id)."));

    let swapped = render_html(&load_at(&ledger_path, &store_path, unix(T0), T0));
    assert!(swapped.contains("Unsupported database (unsupported-schema: application-id)."));
    assert_eq!(fs::read(&store_path).expect("store"), store_bytes);
    assert_eq!(fs::read(&ledger_path).expect("ledger"), ledger_bytes);
    assert!(!corrupt_page.contains("Checked available"));
    assert!(!swapped.contains("Checked available"));
}

#[test]
fn rendering_is_byte_identical_for_identical_inputs() {
    let dir = TempDir::new();
    let (store_path, mut store) = create_store(&dir, "observations.sqlite");
    append(&mut store, &common::Sample::qualifying(1, T0)).expect("append");
    drop(store);
    let (ledger_path, mut ledger) = create_ledger(&dir, "research.sqlite");
    import_bytes(&mut ledger, &research_fixture("partial.json"), 100);
    drop(ledger);
    let now = unix(T0 + 10);
    let first = render_html(&load_at(&store_path, &ledger_path, now, T0 + 10));
    let second = render_html(&load_at(&store_path, &ledger_path, now, T0 + 10));
    assert_eq!(first, second);
    let dashboard = load_at(&store_path, &ledger_path, now, T0 + 10);
    assert_eq!(render_html(&dashboard), render_html(&dashboard));
}

enum SampleSpec {
    Qualifying(u64, i64),
    Reserved(u64, i64),
    Unavailable(u64, i64),
    Failure(u64, i64, AccessResult),
    UnknownTime(u64),
    UnknownOffset(u64, i64),
    UnknownAvailability(u64, i64),
    UnreviewedUnavailable(u64, i64),
    Far(u64, SystemTime),
}

fn page_at(specs: &[SampleSpec], now: SystemTime) -> String {
    let dir = TempDir::new();
    let (store_path, mut store) = create_store(&dir, "observations.sqlite");
    for spec in specs {
        append(&mut store, &spec.sample()).unwrap_or_else(|error| panic!("{error}"));
    }
    drop(store);
    let (ledger_path, _ledger) = create_ledger(&dir, "research.sqlite");
    render_html(&load_at(&store_path, &ledger_path, now, T0))
}

impl SampleSpec {
    fn sample(&self) -> common::Sample {
        match *self {
            Self::Qualifying(id, seconds) => common::Sample::qualifying(id, seconds),
            Self::Reserved(id, seconds) => {
                let mut sample = common::Sample::qualifying(id, seconds);
                sample.availability = SourceAvailability::Reserved;
                sample.statement = Some("synthetic:reserved".to_owned());
                sample
            }
            Self::Unavailable(id, seconds) => {
                let mut sample = common::Sample::qualifying(id, seconds);
                sample.availability = SourceAvailability::Unavailable;
                sample.statement = Some("synthetic:unavailable".to_owned());
                sample
            }
            Self::Failure(id, seconds, access) => {
                let mut sample = common::Sample::qualifying(id, seconds);
                sample.access = access;
                sample.listing = if access == AccessResult::PageGone {
                    ListingDisposition::Removed
                } else {
                    ListingDisposition::Uncertain
                };
                sample.availability = SourceAvailability::Unknown;
                sample.statement = None;
                sample
            }
            Self::UnknownTime(id) => {
                let mut sample = common::Sample::qualifying(id, T0);
                sample.observed = TimestampAssertion::unknown();
                sample
            }
            Self::UnknownOffset(id, seconds) => {
                let mut sample = common::Sample::qualifying(id, seconds);
                sample.observed = TimestampAssertion::with_unknown_offset(
                    Some("synthetic:offset"),
                    Some(unix(seconds)),
                )
                .expect("offset");
                sample
            }
            Self::UnknownAvailability(id, seconds) => {
                let mut sample = common::Sample::qualifying(id, seconds);
                sample.listing = ListingDisposition::Uncertain;
                sample.availability = SourceAvailability::Unknown;
                sample.statement = None;
                sample.redirect = Some("synthetic:redirect".to_owned());
                sample
            }
            Self::UnreviewedUnavailable(id, seconds) => {
                let mut sample = common::Sample::qualifying(id, seconds);
                sample.availability = SourceAvailability::Unavailable;
                sample.statement = Some("synthetic:unavailable".to_owned());
                sample.review = None;
                sample
            }
            Self::Far(id, instant) => {
                let mut sample = common::Sample::qualifying(id, T0);
                sample.observed =
                    TimestampAssertion::known(Some("synthetic:far"), 0, instant).expect("far");
                sample.ingested = instant;
                sample.review.as_mut().expect("review").assessed = instant;
                sample.review.as_mut().expect("review").recorded = instant;
                sample
            }
        }
    }
}

fn nanos_before_expiry() -> SystemTime {
    unix(T0 + INTERVAL) - Duration::from_nanos(1)
}

fn far_instant() -> Option<SystemTime> {
    SystemTime::UNIX_EPOCH.checked_add(Duration::new(i64::MAX as u64, 0))
}

fn eligible_view() -> bookradar_store::CurrentView {
    let dir = TempDir::new();
    let (path, mut store) = create_store(&dir, "eligible.sqlite");
    append(&mut store, &common::Sample::qualifying(1, T0)).expect("append");
    drop(store);
    let mut store = Store::open(&path, OpenMode::ReadOnly).expect("open");
    store
        .current(
            synthetic_offer(),
            bookradar_store::synthetic_target(),
            unix(T0 + 10),
        )
        .expect("current")
}

fn hostile_research(text: &str) -> Vec<u8> {
    let statement = json_string(text);
    format!(
        r#"{{"contract":"bookradar-research","contract_version":1,"producer_id":"synthetic:producer","run_id":"synthetic:run","run_context":{{"agent":{{"name":"synthetic-agent","version":"0"}},"model":{{"name":"synthetic-model","version":null}},"skills":[],"tools":null,"context":"synthetic-context"}},"reported_run_outcome":"succeeded","findings":[{{"finding_id":"F1","source_ref":"synthetic:source","listing_ref":"OFFER-A","listing_generation":"generation-1","subject":{{"state":"candidate","candidate_ref":"synthetic:candidate","identifiers":[]}},"times":{{"observed_at":{{"original":"synthetic:observed","offset_seconds":3600,"instant":{{"unix_seconds":1793541600,"nanoseconds":0}}}},"accessed_at":{{"original":null,"offset_seconds":null,"instant":null}},"captured_at":{{"original":"synthetic:captured","offset_seconds":3600,"instant":{{"unix_seconds":1793541600,"nanoseconds":0}}}},"source_updated_at":{{"original":null,"offset_seconds":null,"instant":null}}}},"facts":{{"language":"slovak","access":"readable","listing":"present","availability":"available","source_statement":{statement},"price":null,"claim_text":null}},"evidence":[{{"reference":"synthetic:evidence","reported_method":"agent-only","state":"present","supports":["subject","language","availability"],"excerpt":null}}],"uncertainty":{{"rationale":{statement},"reported_confidence":null}},"source_provenance":{{"requested_url":{statement},"resolved_url":null,"redirect_reference":null}}}}]}}"#
    )
    .into_bytes()
}

fn json_string(text: &str) -> String {
    let mut out = String::from("\"");
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            _ => out.push(character),
        }
    }
    out.push('"');
    out
}

fn assert_no_stock(html: &str) {
    for phrase in SOLD {
        assert!(!html.contains(phrase), "{phrase}");
    }
}

#[test]
fn renders_non_default_offer() {
    let dir = TempDir::new();
    let (store_path, mut store) = create_store(&dir, "observations.sqlite");
    let (ledger_path, _ledger) = create_ledger(&dir, "research.sqlite");
    let mut sample = common::Sample::qualifying(7, T0);
    sample.source = 2;
    sample.offer_id = 7;
    append(&mut store, &sample).expect("append");
    drop(store);
    let now = unix(T0 + 1_000);
    let mut selected = config_at(&store_path, &ledger_path, now, T0 + 1_000);
    selected.source_id = 2;
    selected.offer_id = 7;
    let html = render_html(&bookradar_ui::load(&selected));
    assert!(section(&html, "query").contains("source 2, offer 7"));
    assert!(section(&html, "current-assessment").contains("Checked available"));
    let defaults = render_html(&load_at(&store_path, &ledger_path, now, T0 + 1_000));
    assert!(section(&defaults, "query").contains("source 1, offer 1"));
    assert!(section(&defaults, "current-assessment").contains("No observations for this offer."));
}

#[test]
fn promoted_offer_renders_checked_available() {
    let dir = TempDir::new();
    let (ledger_path, mut ledger) = create_ledger(&dir, "research.sqlite");
    import_bytes(&mut ledger, &research_fixture("newer.json"), 100);
    drop(ledger);
    let (store_path, store) = create_store(&dir, "observations.sqlite");
    drop(store);
    let mut ledger = ResearchStore::open(&ledger_path, OpenMode::ReadOnly).expect("ledger");
    let observed = T0;
    promote(&mut ledger, &store_path, &promotion(observed)).expect("promote");
    drop(ledger);
    let now = observed + 1_000;
    let html = render_html(&load_at(&store_path, &ledger_path, unix(now), now));
    assert!(section(&html, "query").contains("source 1, offer 1"));
    assert!(section(&html, "current-assessment").contains("Checked available"));
    assert!(!section(&html, "research-current-candidates").contains("Checked available"));
}

fn promotion(observed: i64) -> PromotionRequest {
    let at = unix(observed);
    PromotionRequest {
        query: CandidateQuery {
            producer_id: "synthetic:producer".to_owned(),
            run_id: "synthetic:run".to_owned(),
            finding_id: "synthetic:newer".to_owned(),
            receipt_id: 1,
            position: 0,
        },
        source_id: 1,
        offer_id: 1,
        observation_id: 1,
        subject: SubjectIdentity::Resolved(bookradar_core::EditionId::new(10).expect("edition")),
        language: LanguageEvidence::Slovak,
        access: bookradar_core::AccessResult::Readable,
        listing: bookradar_core::ListingDisposition::Present,
        availability: bookradar_core::SourceAvailability::Available,
        source_statement: "reviewer:statement".to_owned(),
        evidence_method: EvidenceMethod::Manual,
        evidence_reference: "reviewer:evidence".to_owned(),
        cover_subject: true,
        cover_language: true,
        cover_availability: true,
        reviewer_approval: Some("reviewer:approval".to_owned()),
        assessor: "reviewer:assessor".to_owned(),
        rationale: "reviewer:rationale".to_owned(),
        observed_at: at,
        observed_offset: 3_600,
        captured_at: at,
        captured_offset: 3_600,
        ingested_at: unix(observed + 10),
        assessed_at: unix(observed + 10),
        recorded_at: unix(observed + 10),
    }
}
