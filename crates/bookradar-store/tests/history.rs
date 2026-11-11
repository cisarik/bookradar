//! Current-candidate selection and negative labels.

#![forbid(unsafe_code)]

mod common;

use bookradar_core::ReviewerApproval;
use bookradar_core::{
    AccessResult, ConflictState, Eligibility, KnowledgeAssessment, LanguageEvidence,
    ListingDisposition, RecheckState, SourceAvailability, SubjectIdentity, TemporalAssessment,
    TimestampAssertion,
};
use bookradar_store::{Fixture, ReviewDraft, append_fixture, synthetic_offer};

use common::{
    INTERVAL, Sample, T0, TempDir, append, assert_context, create, current_at, render_at, reopen,
    unix,
};

#[test]
fn every_fixture_permutation_selects_recovered_candidate() {
    let mut fixtures = [
        Fixture::Available,
        Fixture::Blocked,
        Fixture::Recovered,
        Fixture::Older,
    ];
    permute(&mut fixtures, 0, &mut |order| {
        let dir = TempDir::new();
        let (_path, mut store) = create(&dir);
        for fixture in order {
            append_fixture(&mut store, *fixture).expect("fixture");
        }
        let now = 1_793_548_800;
        assert_context(
            &current_at(&mut store, now),
            3,
            TemporalAssessment::WithinInterval,
            ConflictState::Clear,
            RecheckState::Clear,
            true,
        );
        let history = store.history(synthetic_offer()).expect("history");
        let ids: Vec<_> = history
            .iter()
            .map(|entry| entry.record().observation().reference().observation().get())
            .collect();
        assert_eq!(ids, vec![4, 1, 2, 3]);
    });
}

#[test]
fn newer_unsuitable_content_does_not_fall_back() {
    let cases = [
        sample_change(2, T0 + 3_600, |sample| {
            sample.availability = SourceAvailability::Unavailable;
            sample.statement = Some("synthetic:unavailable".to_owned());
        }),
        sample_change(2, T0 + 3_600, |sample| {
            sample.availability = SourceAvailability::Reserved;
            sample.statement = Some("synthetic:reserved".to_owned());
        }),
        sample_change(2, T0 + 3_600, |sample| {
            sample.availability = SourceAvailability::Unknown;
            sample.statement = None;
        }),
        sample_change(2, T0 + 3_600, |sample| {
            sample.language = LanguageEvidence::Czech;
        }),
        sample_change(2, T0 + 3_600, |sample| {
            sample.subject = SubjectIdentity::Resolved(common::edition(11));
        }),
        sample_change(2, T0 + 3_600, |sample| {
            sample.review = None;
        }),
        sample_change(2, T0 + 3_600, |sample| {
            sample.review.as_mut().expect("review").knowledge = KnowledgeAssessment::Rejected;
        }),
    ];
    for newer in cases {
        let dir = TempDir::new();
        let (_path, mut store) = create(&dir);
        append(&mut store, &Sample::qualifying(1, T0)).expect("older");
        append(&mut store, &newer).expect("newer");
        let view = current_at(&mut store, T0 + 7_200);
        assert_eq!(candidate_ids(&view), vec![2]);
        assert!(matches!(
            view.candidates()[0].assessment().eligibility(),
            Eligibility::NotEligible
        ));
    }
}

#[test]
fn different_time_stock_changes_are_not_conflicts() {
    let dir = TempDir::new();
    let (path, mut store) = create(&dir);
    append(&mut store, &Sample::qualifying(1, T0)).expect("available");
    append(&mut store, &unavailable(2, T0 + 3_600)).expect("unavailable");
    let view = current_at(&mut store, T0 + 7_200);
    assert_context(
        &view,
        2,
        TemporalAssessment::WithinInterval,
        ConflictState::Clear,
        RecheckState::Clear,
        false,
    );
    assert!(
        render_at(&mut store, T0 + 7_200).ends_with("Last observed unavailable -- unix:1793527200")
    );
    drop(store);
    let mut store = reopen(&path);
    assert!(append(&mut store, &Sample::qualifying(1, T0)).is_err());
    let reverse_dir = TempDir::new();
    let (_reverse_path, mut reverse) = create(&reverse_dir);
    append(&mut reverse, &unavailable(2, T0 + 3_600)).expect("unavailable");
    append(&mut reverse, &Sample::qualifying(1, T0)).expect("available");
    let reversed = current_at(&mut reverse, T0 + 7_200);
    assert_context(
        &reversed,
        2,
        TemporalAssessment::WithinInterval,
        ConflictState::Clear,
        RecheckState::Clear,
        false,
    );

    let dir = TempDir::new();
    let (_path, mut store) = create(&dir);
    append(&mut store, &Sample::qualifying(1, T0)).expect("available");
    let mut reserved = Sample::qualifying(2, T0 + 3_600);
    reserved.availability = SourceAvailability::Reserved;
    reserved.statement = Some("synthetic:reserved".to_owned());
    append(&mut store, &reserved).expect("reserved");
    assert!(
        render_at(&mut store, T0 + 7_200).contains("Last observed reserved -- unix:1793527200")
    );
    assert_eq!(
        current_at(&mut store, T0 + 7_200).conflict(),
        ConflictState::Clear
    );
}

#[test]
fn page_gone_and_access_failures_keep_history_without_a_sale() {
    for access in [
        AccessResult::RateLimited,
        AccessResult::Captcha,
        AccessResult::Timeout,
        AccessResult::AuthenticationBlocked,
        AccessResult::TransportError,
    ] {
        let dir = TempDir::new();
        let (_path, mut store) = create(&dir);
        append(&mut store, &Sample::qualifying(1, T0)).expect("available");
        append(&mut store, &failure(2, T0 + 3_600, access)).expect("failure");
        let view = current_at(&mut store, T0 + 7_200);
        assert_context(
            &view,
            1,
            TemporalAssessment::WithinInterval,
            ConflictState::Clear,
            RecheckState::Failed,
            false,
        );
        let latest = &view.last_ordered_attempts()[0];
        assert_eq!(
            latest.observation().availability(),
            SourceAvailability::Unknown
        );
        assert_eq!(
            latest.observation().listing(),
            ListingDisposition::Uncertain
        );
        assert!(latest.observation().source_statement().is_none());
        let rendered = render_at(&mut store, T0 + 7_200);
        assert!(rendered.contains("Could not recheck -- unix:1793527200"));
        assert!(rendered.contains("Last observed available -- unix:1793523600"));
        assert!(!rendered.contains("Sold out"));
    }

    let dir = TempDir::new();
    let (_path, mut store) = create(&dir);
    append(&mut store, &Sample::qualifying(1, T0)).expect("available");
    let mut gone = failure(2, T0 + 3_600, AccessResult::PageGone);
    gone.listing = ListingDisposition::Removed;
    append(&mut store, &gone).expect("gone");
    let view = current_at(&mut store, T0 + 7_200);
    assert_eq!(view.conflict(), ConflictState::Clear);
    assert_eq!(view.recheck(), RecheckState::Failed);
    assert_eq!(
        view.last_ordered_attempts()[0].observation().listing(),
        ListingDisposition::Removed
    );
    let rendered = render_at(&mut store, T0 + 7_200);
    assert!(rendered.contains("Listing removed -- unix:1793527200"));
    assert!(rendered.contains("Last observed available -- unix:1793523600"));
    assert!(!rendered.contains("Sold out"));

    let dir = TempDir::new();
    let (_path, mut store) = create(&dir);
    let mut redirect = Sample::qualifying(1, T0);
    redirect.listing = ListingDisposition::Uncertain;
    redirect.availability = SourceAvailability::Unknown;
    redirect.statement = None;
    redirect.redirect = Some("synthetic:redirect".to_owned());
    append(&mut store, &redirect).expect("redirect");
    let rendered = render_at(&mut store, T0 + 60);
    assert!(rendered.contains("Unverified -- source availability unknown"));
    assert!(!rendered.contains("Sold out"));
}

#[test]
fn failure_then_recovery_uses_the_new_observation_expiry() {
    let dir = TempDir::new();
    let (_path, mut store) = create(&dir);
    append(&mut store, &Sample::qualifying(1, T0)).expect("available");
    append(&mut store, &failure(2, T0 + 3_600, AccessResult::Timeout)).expect("failure");
    assert_context(
        &current_at(&mut store, T0 + 3_700),
        1,
        TemporalAssessment::WithinInterval,
        ConflictState::Clear,
        RecheckState::Failed,
        false,
    );
    append(&mut store, &Sample::qualifying(3, T0 + 10_800)).expect("recovery");
    let view = current_at(&mut store, T0 + 10_900);
    assert_context(
        &view,
        3,
        TemporalAssessment::WithinInterval,
        ConflictState::Clear,
        RecheckState::Clear,
        true,
    );
    match view.candidates()[0].assessment().eligibility() {
        Eligibility::Eligible { expires_at, .. } => {
            assert_eq!(expires_at, unix(T0 + 97_200));
        }
        Eligibility::NotEligible => panic!("recovery should be eligible"),
    }
}

#[test]
fn exact_freshness_boundaries_ignore_review_and_ingestion() {
    let dir = TempDir::new();
    let (_path, mut store) = create(&dir);
    let mut sample = Sample::qualifying(1, T0);
    sample.ingested = unix(T0 + INTERVAL + 10_000);
    sample.review.as_mut().expect("review").assessed = unix(T0 + INTERVAL + 10_000);
    sample.review.as_mut().expect("review").recorded = unix(T0 + INTERVAL + 10_000);
    append(&mut store, &sample).expect("append");
    let before = store
        .current(
            synthetic_offer(),
            bookradar_store::synthetic_target(),
            unix(T0 + INTERVAL) - std::time::Duration::from_nanos(1),
        )
        .expect("before");
    assert!(eligible(&before, true, -1));
    let at_t1 = store
        .current(
            synthetic_offer(),
            bookradar_store::synthetic_target(),
            unix(T0 + INTERVAL),
        )
        .expect("t1");
    assert_context(
        &at_t1,
        1,
        TemporalAssessment::Stale,
        ConflictState::Clear,
        RecheckState::Clear,
        false,
    );
    let later = store
        .current(
            synthetic_offer(),
            bookradar_store::synthetic_target(),
            unix_nanos_after_t1(),
        )
        .expect("after");
    assert_eq!(
        later.candidates()[0].assessment().temporal(),
        TemporalAssessment::Stale
    );
    store
        .append_review(
            sample.reference(),
            1,
            &supported_at(unix(T0 + INTERVAL + 20_000), "later"),
        )
        .expect("later review");
    let still = store
        .current(
            synthetic_offer(),
            bookradar_store::synthetic_target(),
            unix(T0 + INTERVAL),
        )
        .expect("still stale");
    assert_eq!(
        still.candidates()[0].assessment().temporal(),
        TemporalAssessment::Stale
    );
}

#[test]
fn unknown_future_and_unorderable_history_block_positive_labels() {
    let dir = TempDir::new();
    let (_path, mut store) = create(&dir);
    let mut unknown = Sample::qualifying(1, T0);
    unknown.observed = TimestampAssertion::unknown();
    append(&mut store, &unknown).expect("unknown");
    let rendered = render_at(&mut store, T0 + 10);
    assert!(rendered.contains("Unverified -- observation order unknown"));
    assert!(!rendered.contains("Checked available"));

    let dir = TempDir::new();
    let (_path, mut store) = create(&dir);
    let mut offset = Sample::qualifying(1, T0);
    offset.observed =
        TimestampAssertion::with_unknown_offset(Some("synthetic:offset"), Some(unix(T0)))
            .expect("offset");
    append(&mut store, &offset).expect("offset");
    let view = current_at(&mut store, T0 + 10);
    assert_context(
        &view,
        1,
        TemporalAssessment::Unknown,
        ConflictState::Clear,
        RecheckState::Clear,
        false,
    );
    assert!(render_at(&mut store, T0 + 10).contains("Unverified -- observation offset unknown"));

    let dir = TempDir::new();
    let (_path, mut store) = create(&dir);
    append(&mut store, &Sample::qualifying(1, T0)).expect("older");
    append(&mut store, &Sample::qualifying(2, T0 + 10_000)).expect("future");
    let view = current_at(&mut store, T0 + 100);
    assert_eq!(candidate_ids(&view), vec![2]);
    assert_eq!(
        view.candidates()[0].assessment().temporal(),
        TemporalAssessment::Unknown
    );
    let rendered = render_at(&mut store, T0 + 100);
    assert!(rendered.contains("Unverified -- observation time in future"));
    assert!(!rendered.contains("Checked available"));
    assert!(!rendered.contains("Last observed available"));

    let dir = TempDir::new();
    let (_path, mut store) = create(&dir);
    append(&mut store, &Sample::qualifying(1, T0)).expect("known");
    let mut unordered = Sample::qualifying(2, T0);
    unordered.observed = TimestampAssertion::unknown();
    append(&mut store, &unordered).expect("unordered");
    let view = current_at(&mut store, T0 + 10);
    assert_context(
        &view,
        1,
        TemporalAssessment::WithinInterval,
        ConflictState::Unknown,
        RecheckState::Unknown,
        false,
    );
    assert!(render_at(&mut store, T0 + 10).contains("Unverified -- observation order unknown"));
}

#[test]
fn equal_time_conflict_survives_until_explicit_rejection() {
    for reverse in [false, true] {
        let dir = TempDir::new();
        let (_path, mut store) = create(&dir);
        let first = if reverse {
            unavailable(2, T0)
        } else {
            Sample::qualifying(1, T0)
        };
        let second = if reverse {
            Sample::qualifying(1, T0)
        } else {
            unavailable(2, T0)
        };
        append(&mut store, &first).expect("first");
        append(&mut store, &second).expect("second");
        assert_eq!(
            current_at(&mut store, T0 + 10).conflict(),
            ConflictState::Unresolved
        );
        assert!(render_at(&mut store, T0 + 10).contains("Unverified -- unresolved conflict"));
        assert!(!render_at(&mut store, T0 + 10).contains("Last observed"));
    }

    let dir = TempDir::new();
    let (_path, mut store) = create(&dir);
    append(&mut store, &Sample::qualifying(1, T0)).expect("available");
    append(&mut store, &unavailable(2, T0)).expect("conflict");
    append(&mut store, &Sample::qualifying(3, T0 + 3_600)).expect("later");
    assert_eq!(
        current_at(&mut store, T0 + 3_700).conflict(),
        ConflictState::Unresolved
    );
    assert!(matches!(
        current_at(&mut store, T0 + 3_700).candidates()[0]
            .assessment()
            .eligibility(),
        Eligibility::NotEligible
    ));
    let rejection = rejected_review(unix(T0 + 4_000));
    store
        .append_review(unavailable(2, T0).reference(), 1, &rejection)
        .expect("reject");
    assert_context(
        &current_at(&mut store, T0 + 4_100),
        3,
        TemporalAssessment::WithinInterval,
        ConflictState::Clear,
        RecheckState::Clear,
        true,
    );

    let dir = TempDir::new();
    let (_path, mut store) = create(&dir);
    append(&mut store, &Sample::qualifying(1, T0)).expect("available");
    append(&mut store, &unavailable(2, T0)).expect("unavailable");
    let mut reserved = Sample::qualifying(3, T0);
    reserved.availability = SourceAvailability::Reserved;
    reserved.statement = Some("synthetic:reserved".to_owned());
    append(&mut store, &reserved).expect("reserved");
    store
        .append_review(
            unavailable(2, T0).reference(),
            1,
            &rejected_review(unix(T0 + 20)),
        )
        .expect("reject one");
    assert_eq!(
        current_at(&mut store, T0 + 10).conflict(),
        ConflictState::Unresolved
    );
}

#[test]
fn older_import_does_not_replace_or_invent_a_conflict() {
    let dir = TempDir::new();
    let (_path, mut store) = create(&dir);
    append(&mut store, &Sample::qualifying(2, T0 + 3_600)).expect("newer");
    append(&mut store, &unavailable(1, T0)).expect("older import");
    let view = current_at(&mut store, T0 + 7_200);
    assert_eq!(candidate_ids(&view), vec![2]);
    assert_eq!(view.conflict(), ConflictState::Clear);
    assert!(matches!(
        view.candidates()[0].assessment().eligibility(),
        Eligibility::Eligible { .. }
    ));
}

#[test]
fn compatible_tie_keeps_both_candidates_and_displays_the_lowest_eligible_id() {
    let dir = TempDir::new();
    let (_path, mut store) = create(&dir);
    append(&mut store, &Sample::qualifying(2, T0)).expect("two");
    append(&mut store, &Sample::qualifying(1, T0)).expect("one");
    let view = current_at(&mut store, T0 + 10);
    assert_eq!(candidate_ids(&view), vec![1, 2]);
    assert_eq!(view.conflict(), ConflictState::Clear);
    let rendered = render_at(&mut store, T0 + 10);
    assert!(rendered.contains("latest_attempts=1,2"));
    assert!(rendered.contains("candidates=1,2"));
    assert!(rendered.contains("verification=eligible"));
    assert!(rendered.contains("Checked available -- unix:1793523600"));
}

#[test]
fn later_availability_after_ordinary_unavailability_can_qualify() {
    let dir = TempDir::new();
    let (_path, mut store) = create(&dir);
    append(&mut store, &unavailable(1, T0)).expect("negative");
    append(&mut store, &Sample::qualifying(2, T0 + 10_800)).expect("later");
    assert_context(
        &current_at(&mut store, T0 + 10_900),
        2,
        TemporalAssessment::WithinInterval,
        ConflictState::Clear,
        RecheckState::Clear,
        true,
    );
}

#[test]
fn unsupported_unavailable_claim_has_no_supported_label() {
    let dir = TempDir::new();
    let (_path, mut store) = create(&dir);
    let mut sample = unavailable(1, T0);
    sample.review = None;
    append(&mut store, &sample).expect("append");
    let rendered = render_at(&mut store, T0 + 10);
    assert!(rendered.contains("Unverified -- insufficient support"));
    assert!(!rendered.contains("Last observed unavailable"));
}

#[test]
fn disputed_review_remains_active_across_a_later_observation() {
    let dir = TempDir::new();
    let (_path, mut store) = create(&dir);
    let mut disputed = Sample::qualifying(1, T0);
    disputed.review.as_mut().expect("review").knowledge = KnowledgeAssessment::Disputed;
    append(&mut store, &disputed).expect("disputed");
    append(&mut store, &Sample::qualifying(2, T0 + 3_600)).expect("later");
    assert_eq!(
        current_at(&mut store, T0 + 3_700).conflict(),
        ConflictState::Unresolved
    );
    assert!(render_at(&mut store, T0 + 3_700).contains("Unverified -- unresolved conflict"));
}

fn permute(items: &mut [Fixture], start: usize, visit: &mut dyn FnMut(&[Fixture])) {
    if start == items.len() {
        visit(items);
        return;
    }
    for index in start..items.len() {
        items.swap(start, index);
        permute(items, start + 1, visit);
        items.swap(start, index);
    }
}

fn sample_change(id: u64, seconds: i64, change: impl FnOnce(&mut Sample)) -> Sample {
    let mut sample = Sample::qualifying(id, seconds);
    change(&mut sample);
    sample
}

fn unavailable(id: u64, seconds: i64) -> Sample {
    sample_change(id, seconds, |sample| {
        sample.availability = SourceAvailability::Unavailable;
        sample.statement = Some("synthetic:unavailable".to_owned());
    })
}

fn failure(id: u64, seconds: i64, access: AccessResult) -> Sample {
    sample_change(id, seconds, |sample| {
        sample.access = access;
        sample.listing = if access == AccessResult::PageGone {
            ListingDisposition::Removed
        } else {
            ListingDisposition::Uncertain
        };
        sample.availability = SourceAvailability::Unknown;
        sample.statement = None;
    })
}

fn candidate_ids(view: &bookradar_store::CurrentView) -> Vec<u64> {
    view.candidates()
        .iter()
        .map(|candidate| {
            candidate
                .record()
                .observation()
                .reference()
                .observation()
                .get()
        })
        .collect()
}

fn eligible(view: &bookradar_store::CurrentView, expect: bool, _marker: i32) -> bool {
    let is_eligible = matches!(
        view.candidates()[0].assessment().eligibility(),
        Eligibility::Eligible { .. }
    );
    assert_eq!(is_eligible, expect);
    true
}

fn unix_nanos_after_t1() -> std::time::SystemTime {
    unix(T0 + INTERVAL) + std::time::Duration::from_nanos(1)
}

fn rejected_review(at: std::time::SystemTime) -> ReviewDraft {
    ReviewDraft::try_new(
        KnowledgeAssessment::Rejected,
        Some(ReviewerApproval::new("synthetic:reviewer").expect("approval")),
        "synthetic:assessor",
        "synthetic:reject",
        at,
        at,
    )
    .expect("review")
}

fn supported_at(at: std::time::SystemTime, rationale: &str) -> ReviewDraft {
    ReviewDraft::try_new(
        KnowledgeAssessment::Supported,
        Some(ReviewerApproval::new("synthetic:reviewer").expect("approval")),
        "synthetic:assessor",
        rationale,
        at,
        at,
    )
    .expect("review")
}

impl Sample {
    fn reference(&self) -> bookradar_core::ObservationRef {
        common::record(self)
            .expect("record")
            .observation()
            .reference()
    }
}
