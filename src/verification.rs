//! Pure evaluation of one observation against one explicit context.

use std::time::{Duration, SystemTime};

use crate::model::{
    AccessResult, ConflictState, DomainError, EditionId, EvidenceMethod, KnowledgeAssessment,
    LanguageEvidence, ListingDisposition, Observation, ObservationRef, OffsetState, RecheckState,
    SourceAvailability, SubjectIdentity, TemporalAssessment, TimestampAssertion,
    VerificationContext,
};

/// Exact verification interval: 24 hours of 86,400 seconds.
const VERIFICATION_INTERVAL: Duration = Duration::from_secs(86_400);

/// Policy outcome for supplied inputs. This is not proof of a real listing or a purchase.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Eligibility {
    Eligible {
        observation: ObservationRef,
        observed_at: SystemTime,
        expires_at: SystemTime,
    },
    NotEligible,
}

/// Temporal assessment kept separate from eligibility.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VerificationAssessment {
    temporal: TemporalAssessment,
    eligibility: Eligibility,
}

impl VerificationAssessment {
    pub fn temporal(self) -> TemporalAssessment {
        self.temporal
    }

    pub fn eligibility(self) -> Eligibility {
        self.eligibility
    }
}

#[derive(Clone, Copy)]
enum Freshness {
    WithinInterval {
        observed_at: SystemTime,
        expires_at: SystemTime,
    },
    Stale,
    Unknown,
}

impl Freshness {
    fn temporal(self) -> TemporalAssessment {
        match self {
            Self::WithinInterval { .. } => TemporalAssessment::WithinInterval,
            Self::Stale => TemporalAssessment::Stale,
            Self::Unknown => TemporalAssessment::Unknown,
        }
    }
}

/// Evaluates one supplied observation. The function does not read the clock, the
/// network, or any evidence bytes.
pub fn evaluate_verification(
    target: EditionId,
    observation: &Observation,
    context: &VerificationContext,
    now: SystemTime,
) -> Result<VerificationAssessment, DomainError> {
    if context.observation() != observation.reference() {
        return Err(DomainError::ObservationMismatch {
            assessment: context.observation(),
            observation: observation.reference(),
        });
    }

    let freshness = assess_freshness(observation.observed_at(), now);
    let temporal = freshness.temporal();
    let eligibility = match freshness {
        Freshness::WithinInterval {
            observed_at,
            expires_at,
        } if policy_matches(target, observation, context) => Eligibility::Eligible {
            observation: observation.reference(),
            observed_at,
            expires_at,
        },
        _ => Eligibility::NotEligible,
    };
    Ok(VerificationAssessment {
        temporal,
        eligibility,
    })
}

fn assess_freshness(assertion: &TimestampAssertion, now: SystemTime) -> Freshness {
    if !matches!(assertion.offset(), OffsetState::Known { .. }) {
        return Freshness::Unknown;
    }
    let Some(observed_at) = assertion.instant() else {
        return Freshness::Unknown;
    };
    if observed_at > now {
        return Freshness::Unknown;
    }
    let Some(expires_at) = observed_at.checked_add(VERIFICATION_INTERVAL) else {
        return Freshness::Unknown;
    };
    let Ok(age) = now.duration_since(observed_at) else {
        return Freshness::Unknown;
    };
    if age < VERIFICATION_INTERVAL {
        Freshness::WithinInterval {
            observed_at,
            expires_at,
        }
    } else {
        Freshness::Stale
    }
}

fn policy_matches(
    target: EditionId,
    observation: &Observation,
    context: &VerificationContext,
) -> bool {
    let SubjectIdentity::Resolved(edition) = observation.subject() else {
        return false;
    };
    if edition != target {
        return false;
    }
    if observation.language() != LanguageEvidence::Slovak {
        return false;
    }
    if observation.access() != AccessResult::Readable
        || observation.listing() != ListingDisposition::Present
    {
        return false;
    }
    if observation.availability() != SourceAvailability::Available
        || observation.source_statement().is_none()
    {
        return false;
    }
    let Some(evidence) = observation.evidence() else {
        return false;
    };
    if evidence.reference().is_none() || !evidence.coverage().required_claims_covered() {
        return false;
    }
    match evidence.method() {
        EvidenceMethod::DirectBrowser | EvidenceMethod::Manual => {}
        EvidenceMethod::AgentOnly => return false,
    }
    if context.knowledge() != KnowledgeAssessment::Supported
        || context.reviewer_approval().is_none()
    {
        return false;
    }
    context.conflict() == ConflictState::Clear && context.recheck() == RecheckState::Clear
}
