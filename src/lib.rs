//! Stateless verification policy for one BookRadar book-offer observation.
//!
//! The caller supplies the observation and the current assessment context.
//! This crate does not inspect evidence, reduce history, or persist data.

#![forbid(unsafe_code)]

mod model;
mod verification;

pub use model::{
    AccessResult, ClaimCoverage, ClaimCoverageState, ConflictState, DomainError, EditionId,
    EvidenceMethod, EvidenceReference, EvidenceSupport, IdKind, KnowledgeAssessment,
    LanguageEvidence, ListingDisposition, Observation, ObservationId, ObservationRef, OfferId,
    OfferRef, OffsetState, RecheckState, ReferenceKind, ReviewerApproval, SourceAvailability,
    SourceId, SourceStatement, SubjectIdentity, TemporalAssessment, TimestampAssertion,
    VerificationContext,
};
pub use verification::{Eligibility, VerificationAssessment, evaluate_verification};
