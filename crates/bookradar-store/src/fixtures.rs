//! Fixed synthetic observations used by the proof CLI.

use std::time::{Duration, SystemTime};

use bookradar_core::{
    AccessResult, ClaimCoverage, ClaimCoverageState, EditionId, EvidenceMethod, EvidenceReference,
    EvidenceSupport, KnowledgeAssessment, LanguageEvidence, Observation, ObservationId,
    ObservationRef, OfferId, OfferRef, ReviewerApproval, SourceId, SourceStatement,
    SubjectIdentity, TimestampAssertion,
};

use crate::store::{ObservationRecord, ReviewDraft, Store, StoreError};

/// The four CLI fixtures. Names and identities are fixed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Fixture {
    Available,
    Blocked,
    Recovered,
    Older,
}

impl Fixture {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "available" => Some(Self::Available),
            "blocked" => Some(Self::Blocked),
            "recovered" => Some(Self::Recovered),
            "older" => Some(Self::Older),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Blocked => "blocked",
            Self::Recovered => "recovered",
            Self::Older => "older",
        }
    }

    pub fn observation_id(self) -> u64 {
        match self {
            Self::Available => 1,
            Self::Blocked => 2,
            Self::Recovered => 3,
            Self::Older => 4,
        }
    }

    pub fn observed_unix(self) -> i64 {
        match self {
            Self::Available => 1_793_523_600,
            Self::Blocked => 1_793_527_200,
            Self::Recovered => 1_793_534_400,
            Self::Older => 1_793_520_000,
        }
    }

    pub fn ingested_unix(self) -> i64 {
        match self {
            Self::Available => 1_793_545_201,
            Self::Blocked => 1_793_545_202,
            Self::Recovered => 1_793_545_203,
            Self::Older => 1_793_545_204,
        }
    }

    pub fn record(self) -> ObservationRecord {
        let observed = known_timestamp(self.observed_unix(), self.original());
        let captured = observed.clone();
        let ingested = unix_seconds(self.ingested_unix());
        let reference = ObservationRef::new(
            synthetic_offer(),
            ObservationId::new(self.observation_id()).expect("fixture id"),
        );
        let evidence = Some(manual_evidence(self.name()));
        let observation = if self == Self::Blocked {
            Observation::failed_check(
                reference,
                SubjectIdentity::Resolved(synthetic_target()),
                LanguageEvidence::Slovak,
                observed,
                AccessResult::RateLimited,
                evidence,
            )
            .expect("blocked fixture")
        } else {
            Observation::new(
                reference,
                SubjectIdentity::Resolved(synthetic_target()),
                LanguageEvidence::Slovak,
                observed,
                AccessResult::Readable,
                bookradar_core::ListingDisposition::Present,
                self.availability(),
                Some(SourceStatement::new(self.statement()).expect("statement")),
                evidence,
            )
        };
        ObservationRecord::try_new(
            observation,
            ingested,
            captured,
            None,
            None,
            None,
            format!("synthetic:provenance:{}", self.name()),
            None,
        )
        .expect("fixture record")
    }

    pub fn review(self) -> ReviewDraft {
        let ingested = unix_seconds(self.ingested_unix());
        ReviewDraft::try_new(
            KnowledgeAssessment::Supported,
            Some(ReviewerApproval::new("synthetic:reviewer").expect("reviewer")),
            "synthetic:assessor",
            format!("synthetic:review:{}", self.name()),
            ingested,
            ingested,
        )
        .expect("fixture review")
    }

    fn original(self) -> &'static str {
        match self {
            Self::Available => "2026-11-01T10:00:00+01:00",
            Self::Blocked => "2026-11-01T11:00:00+01:00",
            Self::Recovered => "2026-11-01T13:00:00+01:00",
            Self::Older => "2026-11-01T09:00:00+01:00",
        }
    }

    fn statement(self) -> &'static str {
        match self {
            Self::Available => "synthetic:available",
            Self::Blocked => "synthetic:blocked",
            Self::Recovered => "synthetic:recovered",
            Self::Older => "synthetic:unavailable",
        }
    }

    fn availability(self) -> bookradar_core::SourceAvailability {
        match self {
            Self::Available | Self::Recovered => bookradar_core::SourceAvailability::Available,
            Self::Blocked => bookradar_core::SourceAvailability::Unknown,
            Self::Older => bookradar_core::SourceAvailability::Unavailable,
        }
    }
}

/// Appends one fixed fixture and its supported initial review.
pub fn append_fixture(store: &mut Store, fixture: Fixture) -> Result<(), StoreError> {
    let record = fixture.record();
    let review = fixture.review();
    store.append(&record, Some(&review))
}

pub fn synthetic_offer() -> OfferRef {
    OfferRef::new(
        SourceId::new(1).expect("source"),
        OfferId::new(1).expect("offer"),
    )
}

pub fn synthetic_target() -> EditionId {
    EditionId::new(10).expect("edition")
}

fn unix_seconds(seconds: i64) -> SystemTime {
    if seconds >= 0 {
        SystemTime::UNIX_EPOCH + Duration::from_secs(seconds as u64)
    } else {
        SystemTime::UNIX_EPOCH - Duration::from_secs(seconds.unsigned_abs())
    }
}

fn known_timestamp(seconds: i64, original: &str) -> TimestampAssertion {
    TimestampAssertion::known(Some(original), 3_600, unix_seconds(seconds)).expect("fixture time")
}

fn manual_evidence(name: &str) -> EvidenceSupport {
    let covered = ClaimCoverageState::Covered;
    EvidenceSupport::new(
        Some(EvidenceReference::new(&format!("synthetic:evidence:{name}")).expect("evidence")),
        EvidenceMethod::Manual,
        ClaimCoverage::new(covered, covered, covered),
    )
}
