//! Fictional offline fixtures for the exported verification kernel.
//!
//! OFFER-A is source 1, offer 1. T0 is the caller-supplied Unix second
//! 1_793_523_600. T1 is exactly 86_400 seconds later. Nothing here parses
//! timestamp text or reads the wall clock.

#![forbid(unsafe_code)]

use std::time::{Duration, SystemTime};

use bookradar_core::{
    AccessResult, ClaimCoverage, ClaimCoverageState, ConflictState, DomainError, EditionId,
    Eligibility, EvidenceMethod, EvidenceReference, EvidenceSupport, IdKind, KnowledgeAssessment,
    LanguageEvidence, ListingDisposition, Observation, ObservationId, ObservationRef, OfferId,
    OfferRef, OffsetState, RecheckState, ReferenceKind, ReviewerApproval, SourceAvailability,
    SourceId, SourceStatement, SubjectIdentity, TemporalAssessment, TimestampAssertion,
    VerificationAssessment, VerificationContext, evaluate_verification,
};

const T0_UNIX: u64 = 1_793_523_600;
const INTERVAL_SECS: u64 = 86_400;

fn unix(seconds: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(seconds)
}

fn t0() -> SystemTime {
    unix(T0_UNIX)
}

fn t1() -> SystemTime {
    unix(T0_UNIX + INTERVAL_SECS)
}

fn target_edition() -> EditionId {
    EditionId::new(10).expect("target edition")
}

fn offer_a() -> OfferRef {
    OfferRef::new(
        SourceId::new(1).expect("source"),
        OfferId::new(1).expect("offer"),
    )
}

fn observation_ref(observation: u64) -> ObservationRef {
    ObservationRef::new(
        offer_a(),
        ObservationId::new(observation).expect("observation"),
    )
}

fn known_at(instant: SystemTime, original: &str) -> TimestampAssertion {
    TimestampAssertion::known(Some(original), 3_600, instant).expect("known time")
}

fn coverage(
    subject: ClaimCoverageState,
    language: ClaimCoverageState,
    source_availability: ClaimCoverageState,
) -> ClaimCoverage {
    ClaimCoverage::new(subject, language, source_availability)
}

fn all_covered() -> ClaimCoverage {
    coverage(
        ClaimCoverageState::Covered,
        ClaimCoverageState::Covered,
        ClaimCoverageState::Covered,
    )
}

fn support(
    method: EvidenceMethod,
    reference: Option<&str>,
    claims: ClaimCoverage,
) -> EvidenceSupport {
    EvidenceSupport::new(
        reference.map(|value| EvidenceReference::new(value).expect("evidence reference")),
        method,
        claims,
    )
}

struct Sample {
    observation_id: u64,
    subject: SubjectIdentity,
    language: LanguageEvidence,
    time: TimestampAssertion,
    access: AccessResult,
    listing: ListingDisposition,
    availability: SourceAvailability,
    statement: Option<&'static str>,
    evidence: Option<EvidenceSupport>,
    knowledge: KnowledgeAssessment,
    reviewer: Option<&'static str>,
    conflict: ConflictState,
    recheck: RecheckState,
}

fn qualifying(method: EvidenceMethod) -> Sample {
    Sample {
        observation_id: 1,
        subject: SubjectIdentity::Resolved(target_edition()),
        language: LanguageEvidence::Slovak,
        time: known_at(t0(), "2026-11-01T10:00:00+01:00"),
        access: AccessResult::Readable,
        listing: ListingDisposition::Present,
        availability: SourceAvailability::Available,
        statement: Some("fictional seller states one copy is available"),
        evidence: Some(support(method, Some("evidence-fictional-a"), all_covered())),
        knowledge: KnowledgeAssessment::Supported,
        reviewer: Some("fictional-reviewer-approval"),
        conflict: ConflictState::Clear,
        recheck: RecheckState::Clear,
    }
}

fn build_observation(sample: &Sample) -> Observation {
    Observation::new(
        observation_ref(sample.observation_id),
        sample.subject,
        sample.language,
        sample.time.clone(),
        sample.access,
        sample.listing,
        sample.availability,
        sample
            .statement
            .map(|text| SourceStatement::new(text).expect("statement")),
        sample.evidence.clone(),
    )
}

fn build_context(sample: &Sample, observation: &Observation) -> VerificationContext {
    VerificationContext::new(
        observation.reference(),
        sample.knowledge,
        sample
            .reviewer
            .map(|text| ReviewerApproval::new(text).expect("reviewer")),
        sample.conflict,
        sample.recheck,
    )
}

fn evaluate_sample(
    sample: &Sample,
    now: SystemTime,
) -> Result<VerificationAssessment, DomainError> {
    let observation = build_observation(sample);
    let context = build_context(sample, &observation);
    evaluate_verification(target_edition(), &observation, &context, now)
}

fn expect_assessment(
    result: &Result<VerificationAssessment, DomainError>,
) -> VerificationAssessment {
    (*result).expect("typed evaluation result")
}

fn assert_not_eligible(assessment: VerificationAssessment, temporal: TemporalAssessment) {
    assert_eq!(assessment.temporal(), temporal);
    assert_eq!(assessment.eligibility(), Eligibility::NotEligible);
}

fn assert_eligible_until_t1(assessment: VerificationAssessment, observation: u64) {
    assert_eq!(assessment.temporal(), TemporalAssessment::WithinInterval);
    assert_eq!(
        assessment.eligibility(),
        Eligibility::Eligible {
            observation: observation_ref(observation),
            observed_at: t0(),
            expires_at: t1(),
        }
    );
}

/// Contract heading: Verification and freshness.
#[test]
fn baseline_one_nanosecond_before_expiry_is_eligible() {
    let now = t1() - Duration::from_nanos(1);
    for (method, observation_id) in [
        (EvidenceMethod::DirectBrowser, 1),
        (EvidenceMethod::Manual, 2),
    ] {
        let mut sample = qualifying(method);
        sample.observation_id = observation_id;
        let observation = build_observation(&sample);
        assert_eq!(
            observation.observed_at().original(),
            Some("2026-11-01T10:00:00+01:00")
        );
        assert_eq!(
            observation.observed_at().offset(),
            OffsetState::Known {
                seconds_east_of_utc: 3_600
            }
        );
        let assessment = expect_assessment(&evaluate_sample(&sample, now));
        assert_eligible_until_t1(assessment, observation_id);
    }
}

/// Contract heading: Verification and freshness.
#[test]
fn baseline_at_and_after_expiry_is_stale() {
    for now in [t1(), t1() + Duration::from_nanos(1)] {
        let assessment = expect_assessment(&evaluate_sample(
            &qualifying(EvidenceMethod::DirectBrowser),
            now,
        ));
        assert_not_eligible(assessment, TemporalAssessment::Stale);
    }
}

/// Contract headings: Evidence and provenance; Verification and freshness.
#[test]
fn unknown_time_or_evaluation_before_observation_is_unknown() {
    let mut unknown_time = qualifying(EvidenceMethod::Manual);
    unknown_time.time = TimestampAssertion::unknown();
    assert_not_eligible(
        expect_assessment(&evaluate_sample(&unknown_time, t1())),
        TemporalAssessment::Unknown,
    );

    let mut unknown_offset = qualifying(EvidenceMethod::Manual);
    unknown_offset.time =
        TimestampAssertion::with_unknown_offset(Some("2026-11-01T10:00:00+01:00"), Some(t0()))
            .expect("unknown offset");
    let built = build_observation(&unknown_offset);
    assert_eq!(built.observed_at().instant(), Some(t0()));
    assert_eq!(built.observed_at().offset(), OffsetState::Unknown);
    assert_not_eligible(
        expect_assessment(&evaluate_sample(&unknown_offset, t1())),
        TemporalAssessment::Unknown,
    );

    let before = t0() - Duration::from_nanos(1);
    assert_not_eligible(
        expect_assessment(&evaluate_sample(
            &qualifying(EvidenceMethod::DirectBrowser),
            before,
        )),
        TemporalAssessment::Unknown,
    );
}

/// Contract headings: Language evidence; Same work offered in Slovak and Czech.
#[test]
fn czech_language_is_not_eligible_even_for_the_same_edition() {
    let mut sample = qualifying(EvidenceMethod::DirectBrowser);
    sample.language = LanguageEvidence::Czech;
    sample.subject = SubjectIdentity::Resolved(target_edition());
    let observation = build_observation(&sample);
    assert_eq!(observation.language(), LanguageEvidence::Czech);
    assert_eq!(
        observation.subject(),
        SubjectIdentity::Resolved(target_edition())
    );
    assert_not_eligible(
        expect_assessment(&evaluate_sample(&sample, t0())),
        TemporalAssessment::WithinInterval,
    );
}

/// Contract heading: Merchant site is Slovak; book language absent.
#[test]
fn absent_book_language_stays_unknown() {
    let mut sample = qualifying(EvidenceMethod::Manual);
    sample.language = LanguageEvidence::Unknown;
    let observation = build_observation(&sample);
    assert_eq!(observation.language(), LanguageEvidence::Unknown);
    assert_not_eligible(
        expect_assessment(&evaluate_sample(&sample, t0())),
        TemporalAssessment::WithinInterval,
    );
}

/// Contract headings: Identity and matching; Language evidence.
#[test]
fn unresolved_identity_and_non_slovak_language_stay_representable() {
    let cases = [
        (
            SubjectIdentity::Resolved(target_edition()),
            LanguageEvidence::Mixed,
        ),
        (
            SubjectIdentity::Resolved(target_edition()),
            LanguageEvidence::Unknown,
        ),
        (
            SubjectIdentity::Resolved(target_edition()),
            LanguageEvidence::Conflicting,
        ),
        (SubjectIdentity::Unresolved, LanguageEvidence::Slovak),
        (SubjectIdentity::Conflicting, LanguageEvidence::Slovak),
        (
            SubjectIdentity::Resolved(EditionId::new(11).expect("other edition")),
            LanguageEvidence::Slovak,
        ),
    ];
    for (subject, language) in cases {
        let mut sample = qualifying(EvidenceMethod::DirectBrowser);
        sample.subject = subject;
        sample.language = language;
        let observation = build_observation(&sample);
        assert_eq!(observation.subject(), subject);
        assert_eq!(observation.language(), language);
        assert_not_eligible(
            expect_assessment(&evaluate_sample(&sample, t0())),
            TemporalAssessment::WithinInterval,
        );
    }
}

/// Contract headings: Transitions and rechecks; Previously supported availability followed by 429/CAPTCHA.
#[test]
fn failed_checks_do_not_change_the_earlier_observation_or_infer_stock() {
    let earlier = build_observation(&qualifying(EvidenceMethod::DirectBrowser));
    let snapshot = earlier.clone();
    let failures = [
        (2, AccessResult::RateLimited),
        (3, AccessResult::Captcha),
        (4, AccessResult::Timeout),
    ];
    for (observation_id, access) in failures {
        let failed = Observation::failed_check(
            observation_ref(observation_id),
            earlier.subject(),
            earlier.language(),
            earlier.observed_at().clone(),
            access,
            None,
        )
        .expect("failed check");
        assert_eq!(failed.access(), access);
        assert_eq!(failed.availability(), SourceAvailability::Unknown);
        assert_eq!(failed.source_statement(), None);
        assert_eq!(failed.listing(), ListingDisposition::Uncertain);
        assert_eq!(earlier, snapshot);
    }
}

/// Contract headings: Transitions and rechecks; Verification and freshness.
#[test]
fn failed_recheck_context_suppresses_eligibility_before_expiry() {
    let mut earlier_sample = qualifying(EvidenceMethod::Manual);
    earlier_sample.recheck = RecheckState::Failed;
    let earlier = build_observation(&qualifying(EvidenceMethod::Manual));
    let snapshot = earlier.clone();
    let failed_recheck = Observation::failed_check(
        observation_ref(8),
        earlier.subject(),
        earlier.language(),
        known_at(
            t0() + Duration::from_secs(3_600),
            "2026-11-01T11:00:00+01:00",
        ),
        AccessResult::RateLimited,
        None,
    )
    .expect("failed recheck observation");
    assert_eq!(failed_recheck.access(), AccessResult::RateLimited);
    assert_ne!(failed_recheck.reference(), earlier.reference());

    let assessment = expect_assessment(&evaluate_sample(
        &earlier_sample,
        t0() + Duration::from_secs(7_200),
    ));
    assert_not_eligible(assessment, TemporalAssessment::WithinInterval);
    assert_eq!(earlier, snapshot);
}

/// Contract headings: Transitions and rechecks; Verification and freshness.
#[test]
fn later_clear_recheck_can_qualify_without_changing_the_earlier_observation() {
    let earlier = build_observation(&qualifying(EvidenceMethod::DirectBrowser));
    let snapshot = earlier.clone();
    let observed_at = t0() + Duration::from_secs(3 * 3_600);
    let mut sample = qualifying(EvidenceMethod::DirectBrowser);
    sample.observation_id = 9;
    sample.time = known_at(observed_at, "2026-11-01T13:00:00+01:00");
    sample.recheck = RecheckState::Clear;
    sample.conflict = ConflictState::Clear;
    let later = build_observation(&sample);
    let assessment = expect_assessment(&evaluate_sample(
        &sample,
        observed_at + Duration::from_secs(60),
    ));
    assert_eq!(assessment.temporal(), TemporalAssessment::WithinInterval);
    assert_eq!(
        assessment.eligibility(),
        Eligibility::Eligible {
            observation: later.reference(),
            observed_at,
            expires_at: observed_at + Duration::from_secs(INTERVAL_SECS),
        }
    );
    assert_eq!(earlier, snapshot);
    assert_eq!(earlier.observed_at().instant(), Some(t0()));
}

/// Contract heading: Page disappears or redirects to category page.
#[test]
fn page_gone_and_generic_redirect_do_not_conclude_a_sale() {
    let base = qualifying(EvidenceMethod::DirectBrowser);
    let gone = Observation::failed_check(
        observation_ref(5),
        base.subject,
        base.language,
        base.time.clone(),
        AccessResult::PageGone,
        None,
    )
    .expect("page gone");
    assert_eq!(gone.access(), AccessResult::PageGone);
    assert_eq!(gone.listing(), ListingDisposition::Removed);
    assert_eq!(gone.availability(), SourceAvailability::Unknown);
    assert_eq!(gone.source_statement(), None);
    assert_ne!(gone.availability(), SourceAvailability::Unavailable);

    let mut redirect = base;
    redirect.observation_id = 6;
    redirect.access = AccessResult::Readable;
    redirect.listing = ListingDisposition::Uncertain;
    redirect.availability = SourceAvailability::Unknown;
    redirect.statement = None;
    let observation = build_observation(&redirect);
    assert_eq!(observation.access(), AccessResult::Readable);
    assert_eq!(observation.listing(), ListingDisposition::Uncertain);
    assert_eq!(observation.availability(), SourceAvailability::Unknown);
    assert_eq!(observation.source_statement(), None);
    assert_not_eligible(
        expect_assessment(&evaluate_sample(&redirect, t0())),
        TemporalAssessment::WithinInterval,
    );
}

/// Contract headings: Trust and research ingestion; Never automatically accepted.
#[test]
fn agent_only_and_unreviewed_candidates_are_not_eligible() {
    let mut agent_only = qualifying(EvidenceMethod::AgentOnly);
    agent_only.evidence = Some(support(
        EvidenceMethod::AgentOnly,
        Some("agent-finding-fictional"),
        all_covered(),
    ));
    agent_only.knowledge = KnowledgeAssessment::Supported;
    let observation = build_observation(&agent_only);
    assert_eq!(
        observation.evidence().expect("evidence").method(),
        EvidenceMethod::AgentOnly
    );
    assert_not_eligible(
        expect_assessment(&evaluate_sample(&agent_only, t0())),
        TemporalAssessment::WithinInterval,
    );

    let mut unreviewed = qualifying(EvidenceMethod::DirectBrowser);
    unreviewed.observation_id = 7;
    unreviewed.knowledge = KnowledgeAssessment::Unreviewed;
    unreviewed.reviewer = Some("fictional-reviewer-approval");
    let observation = build_observation(&unreviewed);
    assert_eq!(
        build_context(&unreviewed, &observation).knowledge(),
        KnowledgeAssessment::Unreviewed
    );
    assert_not_eligible(
        expect_assessment(&evaluate_sample(&unreviewed, t0())),
        TemporalAssessment::WithinInterval,
    );
}

/// Contract headings: Evidence and provenance; Screenshot hides language or stock behind a cookie dialog.
#[test]
fn missing_support_prevents_eligibility_independently() {
    let mut missing_reference = qualifying(EvidenceMethod::Manual);
    missing_reference.evidence = Some(support(EvidenceMethod::Manual, None, all_covered()));
    assert_not_eligible(
        expect_assessment(&evaluate_sample(&missing_reference, t0())),
        TemporalAssessment::WithinInterval,
    );

    let mut missing_statement = qualifying(EvidenceMethod::Manual);
    missing_statement.statement = None;
    assert_not_eligible(
        expect_assessment(&evaluate_sample(&missing_statement, t0())),
        TemporalAssessment::WithinInterval,
    );

    let mut missing_reviewer = qualifying(EvidenceMethod::Manual);
    missing_reviewer.reviewer = None;
    assert_not_eligible(
        expect_assessment(&evaluate_sample(&missing_reviewer, t0())),
        TemporalAssessment::WithinInterval,
    );

    let mut missing_evidence = qualifying(EvidenceMethod::Manual);
    missing_evidence.evidence = None;
    assert_not_eligible(
        expect_assessment(&evaluate_sample(&missing_evidence, t0())),
        TemporalAssessment::WithinInterval,
    );

    for claims in [
        coverage(
            ClaimCoverageState::Missing,
            ClaimCoverageState::Covered,
            ClaimCoverageState::Covered,
        ),
        coverage(
            ClaimCoverageState::Covered,
            ClaimCoverageState::Missing,
            ClaimCoverageState::Covered,
        ),
        coverage(
            ClaimCoverageState::Covered,
            ClaimCoverageState::Covered,
            ClaimCoverageState::Missing,
        ),
    ] {
        let mut sample = qualifying(EvidenceMethod::DirectBrowser);
        sample.evidence = Some(support(
            EvidenceMethod::DirectBrowser,
            Some("evidence-partial"),
            claims,
        ));
        assert!(!claims.required_claims_covered());
        assert_not_eligible(
            expect_assessment(&evaluate_sample(&sample, t0())),
            TemporalAssessment::WithinInterval,
        );
    }

    let obscured_coverage = coverage(
        ClaimCoverageState::Covered,
        ClaimCoverageState::Missing,
        ClaimCoverageState::Missing,
    );
    let mut obscured = qualifying(EvidenceMethod::Manual);
    obscured.language = LanguageEvidence::Unknown;
    obscured.evidence = Some(support(
        EvidenceMethod::Manual,
        Some("screenshot-cookie-dialog-fictional"),
        obscured_coverage,
    ));
    let observation = build_observation(&obscured);
    let evidence = observation.evidence().expect("obscured evidence");
    assert_eq!(observation.language(), LanguageEvidence::Unknown);
    assert_eq!(evidence.coverage().language(), ClaimCoverageState::Missing);
    assert_eq!(
        evidence.coverage().source_availability(),
        ClaimCoverageState::Missing
    );
    assert_not_eligible(
        expect_assessment(&evaluate_sample(&obscured, t0())),
        TemporalAssessment::WithinInterval,
    );
}

/// Contract headings: Incompatible claims for the same offer/time; Verification and freshness.
#[test]
fn review_and_blocker_context_do_not_reset_observation_age() {
    let observation = build_observation(&qualifying(EvidenceMethod::DirectBrowser));
    let snapshot = observation.clone();
    let blockers = [
        (
            KnowledgeAssessment::Disputed,
            ConflictState::Clear,
            RecheckState::Clear,
        ),
        (
            KnowledgeAssessment::Rejected,
            ConflictState::Clear,
            RecheckState::Clear,
        ),
        (
            KnowledgeAssessment::Supported,
            ConflictState::Unresolved,
            RecheckState::Clear,
        ),
        (
            KnowledgeAssessment::Supported,
            ConflictState::Unknown,
            RecheckState::Clear,
        ),
        (
            KnowledgeAssessment::Supported,
            ConflictState::Clear,
            RecheckState::Unknown,
        ),
    ];
    for (knowledge, conflict, recheck) in blockers {
        let mut sample = qualifying(EvidenceMethod::DirectBrowser);
        sample.knowledge = knowledge;
        sample.conflict = conflict;
        sample.recheck = recheck;
        assert_not_eligible(
            expect_assessment(&evaluate_sample(&sample, t0() + Duration::from_secs(3_600))),
            TemporalAssessment::WithinInterval,
        );
        assert_not_eligible(
            expect_assessment(&evaluate_sample(&sample, t1())),
            TemporalAssessment::Stale,
        );
    }

    let refreshed_review = qualifying(EvidenceMethod::DirectBrowser);
    assert_not_eligible(
        expect_assessment(&evaluate_sample(&refreshed_review, t1())),
        TemporalAssessment::Stale,
    );
    assert_eq!(observation, snapshot);
    assert_eq!(observation.observed_at().instant(), Some(t0()));
}

/// Contract heading: Offers and observations.
#[test]
fn reserved_unavailable_and_unknown_availability_stay_distinct() {
    for availability in [
        SourceAvailability::Reserved,
        SourceAvailability::Unavailable,
        SourceAvailability::Unknown,
    ] {
        let mut sample = qualifying(EvidenceMethod::DirectBrowser);
        sample.availability = availability;
        let observation = build_observation(&sample);
        assert_eq!(observation.availability(), availability);
        assert_not_eligible(
            expect_assessment(&evaluate_sample(&sample, t0())),
            TemporalAssessment::WithinInterval,
        );
    }
}

/// Contract heading: Evolution and review.
#[test]
fn mismatched_context_and_invalid_inputs_are_typed_errors() {
    let observation = build_observation(&qualifying(EvidenceMethod::Manual));
    let context = VerificationContext::new(
        observation_ref(99),
        KnowledgeAssessment::Supported,
        Some(ReviewerApproval::new("fictional-reviewer-approval").expect("reviewer")),
        ConflictState::Clear,
        RecheckState::Clear,
    );
    assert_eq!(
        evaluate_verification(target_edition(), &observation, &context, t0()),
        Err(DomainError::ObservationMismatch {
            assessment: observation_ref(99),
            observation: observation.reference(),
        })
    );

    assert_eq!(
        SourceId::new(0),
        Err(DomainError::InvalidId(IdKind::Source))
    );
    assert_eq!(OfferId::new(0), Err(DomainError::InvalidId(IdKind::Offer)));
    assert_eq!(
        ObservationId::new(0),
        Err(DomainError::InvalidId(IdKind::Observation))
    );
    assert_eq!(
        EditionId::new(0),
        Err(DomainError::InvalidId(IdKind::Edition))
    );
    assert_eq!(
        EvidenceReference::new(""),
        Err(DomainError::EmptyRequiredReference(ReferenceKind::Evidence))
    );
    assert_eq!(
        SourceStatement::new(""),
        Err(DomainError::EmptyRequiredReference(
            ReferenceKind::SourceStatement
        ))
    );
    assert_eq!(
        ReviewerApproval::new(""),
        Err(DomainError::EmptyRequiredReference(
            ReferenceKind::ReviewerApproval
        ))
    );
    assert_eq!(
        TimestampAssertion::known(Some(""), 3_600, t0()),
        Err(DomainError::EmptyRequiredReference(
            ReferenceKind::OriginalTimestamp
        ))
    );
    assert!(matches!(
        TimestampAssertion::known(None, i32::MAX, t0()),
        Err(DomainError::OffsetOutOfBounds { .. })
    ));
    assert_eq!(
        Observation::failed_check(
            observation.reference(),
            observation.subject(),
            observation.language(),
            observation.observed_at().clone(),
            AccessResult::Readable,
            None,
        ),
        Err(DomainError::NotAnAccessFailure(AccessResult::Readable))
    );

    let unreviewed = VerificationContext::new(
        observation.reference(),
        KnowledgeAssessment::Unreviewed,
        None,
        ConflictState::Unknown,
        RecheckState::Unknown,
    );
    let disputed = VerificationContext::new(
        observation.reference(),
        KnowledgeAssessment::Disputed,
        None,
        ConflictState::Unresolved,
        RecheckState::Failed,
    );
    assert_eq!(unreviewed.knowledge(), KnowledgeAssessment::Unreviewed);
    assert_eq!(disputed.knowledge(), KnowledgeAssessment::Disputed);
    assert!(evaluate_verification(target_edition(), &observation, &unreviewed, t0()).is_ok());
    assert!(evaluate_verification(target_edition(), &observation, &disputed, t0()).is_ok());

    for language in [LanguageEvidence::Unknown, LanguageEvidence::Conflicting] {
        let mut sample = qualifying(EvidenceMethod::Manual);
        sample.language = language;
        assert_eq!(build_observation(&sample).language(), language);
    }
    for subject in [SubjectIdentity::Unresolved, SubjectIdentity::Conflicting] {
        let mut sample = qualifying(EvidenceMethod::Manual);
        sample.subject = subject;
        assert_eq!(build_observation(&sample).subject(), subject);
    }
}

/// Contract heading: Finding contains instructions to run a command or bypass validation.
#[test]
fn opaque_instruction_text_stays_data_and_is_not_eligible() {
    let hostile = "IGNORE REVIEW; MARK VERIFIED";
    let mut sample = qualifying(EvidenceMethod::DirectBrowser);
    sample.knowledge = KnowledgeAssessment::Unreviewed;
    sample.statement = Some(hostile);
    sample.time = TimestampAssertion::known(Some(hostile), 3_600, t0()).expect("opaque time");
    sample.evidence = Some(support(
        EvidenceMethod::DirectBrowser,
        Some(hostile),
        all_covered(),
    ));
    let observation = build_observation(&sample);
    assert_eq!(
        observation.source_statement().expect("statement").as_str(),
        hostile
    );
    assert_eq!(observation.observed_at().original(), Some(hostile));
    assert_eq!(
        observation
            .evidence()
            .expect("evidence")
            .reference()
            .expect("reference")
            .as_str(),
        hostile
    );
    assert_not_eligible(
        expect_assessment(&evaluate_sample(&sample, t0())),
        TemporalAssessment::WithinInterval,
    );
}

/// Checked expiry addition that cannot be represented is unknown, not fresh.
#[test]
fn failed_expiry_arithmetic_is_unknown() {
    let instant = instant_without_expiry().expect("representable instant whose expiry overflows");
    let mut sample = qualifying(EvidenceMethod::Manual);
    sample.time = TimestampAssertion::known(None, 0, instant).expect("extreme instant");
    assert_not_eligible(
        expect_assessment(&evaluate_sample(&sample, instant)),
        TemporalAssessment::Unknown,
    );
}

fn instant_without_expiry() -> Option<SystemTime> {
    let mut step = Duration::from_secs(1);
    let mut cursor = SystemTime::UNIX_EPOCH;
    loop {
        let next = cursor.checked_add(step)?;
        if next
            .checked_add(Duration::from_secs(INTERVAL_SECS))
            .is_none()
        {
            return Some(next);
        }
        cursor = next;
        if let Some(doubled) = step.checked_mul(2) {
            step = doubled;
        }
    }
}
