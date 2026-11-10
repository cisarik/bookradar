//! Checked domain values. Fields stay private; dimensions stay independent.

use std::num::NonZeroU64;
use std::time::SystemTime;

/// Eighteen hours covers stated civil offsets.
///
/// The bound is structural. It does not show that an offset was derived from
/// source text, and evaluation does not apply it to the normalized instant.
const MAX_ABSOLUTE_OFFSET_SECONDS: i32 = 18 * 60 * 60;

/// Which identifier rejected a zero value.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum IdKind {
    Source,
    Offer,
    Observation,
    Edition,
}

/// Which required text was empty.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ReferenceKind {
    Evidence,
    SourceStatement,
    ReviewerApproval,
    OriginalTimestamp,
}

/// A construction or evaluation failure. Library paths return this instead of panicking.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DomainError {
    InvalidId(IdKind),
    EmptyRequiredReference(ReferenceKind),
    OffsetOutOfBounds {
        seconds_east_of_utc: i32,
    },
    NotAnAccessFailure(AccessResult),
    ObservationMismatch {
        assessment: ObservationRef,
        observation: ObservationRef,
    },
}

impl std::fmt::Display for IdKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Source => "source",
            Self::Offer => "offer",
            Self::Observation => "observation",
            Self::Edition => "edition",
        })
    }
}

impl std::fmt::Display for ReferenceKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Evidence => "evidence",
            Self::SourceStatement => "source-statement",
            Self::ReviewerApproval => "reviewer-approval",
            Self::OriginalTimestamp => "original-timestamp",
        })
    }
}

impl std::fmt::Display for DomainError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidId(kind) => write!(formatter, "invalid zero {kind} identifier"),
            Self::EmptyRequiredReference(kind) => {
                write!(formatter, "empty required {kind} reference")
            }
            Self::OffsetOutOfBounds {
                seconds_east_of_utc,
            } => write!(
                formatter,
                "offset {seconds_east_of_utc} seconds is outside the structural bound"
            ),
            Self::NotAnAccessFailure(access) => {
                write!(formatter, "access result {access:?} is not a failed check")
            }
            Self::ObservationMismatch {
                assessment,
                observation,
            } => write!(
                formatter,
                "assessment {assessment:?} is not bound to observation {observation:?}"
            ),
        }
    }
}

impl std::error::Error for DomainError {}

fn nonzero_id(value: u64, kind: IdKind) -> Result<NonZeroU64, DomainError> {
    NonZeroU64::new(value).ok_or(DomainError::InvalidId(kind))
}

fn optional_text(value: Option<&str>, kind: ReferenceKind) -> Result<Option<String>, DomainError> {
    match value {
        None => Ok(None),
        Some(text) if !text.is_empty() => Ok(Some(text.to_owned())),
        Some(_) => Err(DomainError::EmptyRequiredReference(kind)),
    }
}

fn required_text(value: &str, kind: ReferenceKind) -> Result<String, DomainError> {
    optional_text(Some(value), kind)?.ok_or(DomainError::EmptyRequiredReference(kind))
}

macro_rules! id_newtype {
    ($name:ident, $kind:ident, $doc:expr) => {
        #[doc = $doc]
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
        pub struct $name(NonZeroU64);

        impl $name {
            /// Returns an error when `value` is zero.
            pub fn new(value: u64) -> Result<Self, DomainError> {
                nonzero_id(value, IdKind::$kind).map(Self)
            }

            /// Returns the caller-allocated key.
            pub fn get(self) -> u64 {
                self.0.get()
            }
        }
    };
}

id_newtype!(
    SourceId,
    Source,
    "Caller-allocated source key. This crate does not generate or resolve it."
);
id_newtype!(
    OfferId,
    Offer,
    "Caller-allocated offer key within one source. A URL or seller change is not an operation here."
);
id_newtype!(
    ObservationId,
    Observation,
    "Caller-allocated observation key. Later attempts use a new key rather than overwriting one."
);
id_newtype!(
    EditionId,
    Edition,
    "Caller-allocated edition key. This crate does not match titles, ISBNs, or copies."
);

/// One offer generation at one source.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct OfferRef {
    source: SourceId,
    offer: OfferId,
}

impl OfferRef {
    pub fn new(source: SourceId, offer: OfferId) -> Self {
        Self { source, offer }
    }

    pub fn source(self) -> SourceId {
        self.source
    }

    pub fn offer(self) -> OfferId {
        self.offer
    }
}

/// Binds an assessment to one observation of one offer.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct ObservationRef {
    offer: OfferRef,
    observation: ObservationId,
}

impl ObservationRef {
    pub fn new(offer: OfferRef, observation: ObservationId) -> Self {
        Self { offer, observation }
    }

    pub fn offer(self) -> OfferRef {
        self.offer
    }

    pub fn observation(self) -> ObservationId {
        self.observation
    }
}

/// Whether the caller resolved this observation to an edition.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum SubjectIdentity {
    Resolved(EditionId),
    Unresolved,
    Conflicting,
}

/// Book-language evidence. A site locale is not a value of this type.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum LanguageEvidence {
    Slovak,
    Czech,
    Mixed,
    Unknown,
    Conflicting,
}

/// Source-reported availability. These states are not inferred from access failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum SourceAvailability {
    Available,
    Reserved,
    Unavailable,
    Unknown,
}

/// What the check was able to do. This is not a stock conclusion.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum AccessResult {
    Readable,
    RateLimited,
    Captcha,
    AuthenticationBlocked,
    Timeout,
    TransportError,
    PageGone,
}

/// Whether the listing itself appeared present.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ListingDisposition {
    Present,
    Removed,
    Uncertain,
}

/// Caller-supplied knowledge state. Unknown and disputed values are valid inputs.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum KnowledgeAssessment {
    Unreviewed,
    Supported,
    Disputed,
    Rejected,
}

/// Freshness of a usable instant. This is not eligibility.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum TemporalAssessment {
    WithinInterval,
    Stale,
    Unknown,
}

/// Explicit conflict input. The library does not detect conflicts.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ConflictState {
    Clear,
    Unresolved,
    Unknown,
}

/// Explicit recheck input. The library does not choose which observation is current.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum RecheckState {
    Clear,
    Failed,
    Unknown,
}

/// How the evidence was obtained. Agent-only evidence cannot qualify.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum EvidenceMethod {
    DirectBrowser,
    Manual,
    AgentOnly,
}

/// Whether one required claim is covered. Absence is not a covered claim.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ClaimCoverageState {
    Covered,
    Missing,
}

/// Coverage of subject, language, and source-availability claims.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ClaimCoverage {
    subject: ClaimCoverageState,
    language: ClaimCoverageState,
    source_availability: ClaimCoverageState,
}

impl ClaimCoverage {
    pub fn new(
        subject: ClaimCoverageState,
        language: ClaimCoverageState,
        source_availability: ClaimCoverageState,
    ) -> Self {
        Self {
            subject,
            language,
            source_availability,
        }
    }

    pub fn subject(self) -> ClaimCoverageState {
        self.subject
    }

    pub fn language(self) -> ClaimCoverageState {
        self.language
    }

    pub fn source_availability(self) -> ClaimCoverageState {
        self.source_availability
    }

    pub fn required_claims_covered(self) -> bool {
        self.subject == ClaimCoverageState::Covered
            && self.language == ClaimCoverageState::Covered
            && self.source_availability == ClaimCoverageState::Covered
    }
}

/// Opaque evidence reference. The text is data, not an instruction.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct EvidenceReference(String);

impl EvidenceReference {
    pub fn new(value: &str) -> Result<Self, DomainError> {
        required_text(value, ReferenceKind::Evidence).map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Explicit nonempty source statement. This crate does not judge whether it is true.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct SourceStatement(String);

impl SourceStatement {
    pub fn new(value: &str) -> Result<Self, DomainError> {
        required_text(value, ReferenceKind::SourceStatement).map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Attributed reviewer-approval reference. Presence is not authentication.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ReviewerApproval(String);

impl ReviewerApproval {
    pub fn new(value: &str) -> Result<Self, DomainError> {
        required_text(value, ReferenceKind::ReviewerApproval).map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Known or unknown offset supplied by the caller.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum OffsetState {
    Known { seconds_east_of_utc: i32 },
    Unknown,
}

/// Original representation, offset knowledge, and an optional normalized instant.
///
/// This type has no timestamp parser and no timezone conversion.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TimestampAssertion {
    original: Option<String>,
    offset: OffsetState,
    instant: Option<SystemTime>,
}

impl TimestampAssertion {
    /// No original text, no known offset, and no instant.
    pub fn unknown() -> Self {
        Self {
            original: None,
            offset: OffsetState::Unknown,
            instant: None,
        }
    }

    /// Retains an instant when the caller has one, while the offset stays unknown.
    pub fn with_unknown_offset(
        original: Option<&str>,
        instant: Option<SystemTime>,
    ) -> Result<Self, DomainError> {
        Ok(Self {
            original: optional_text(original, ReferenceKind::OriginalTimestamp)?,
            offset: OffsetState::Unknown,
            instant,
        })
    }

    /// Stores a caller-normalized instant without checking it against `original`.
    ///
    /// `seconds_east_of_utc` must lie within ±18 hours. The offset is retained
    /// and is not added to `instant`.
    pub fn known(
        original: Option<&str>,
        seconds_east_of_utc: i32,
        instant: SystemTime,
    ) -> Result<Self, DomainError> {
        if !(-MAX_ABSOLUTE_OFFSET_SECONDS..=MAX_ABSOLUTE_OFFSET_SECONDS)
            .contains(&seconds_east_of_utc)
        {
            return Err(DomainError::OffsetOutOfBounds {
                seconds_east_of_utc,
            });
        }
        Ok(Self {
            original: optional_text(original, ReferenceKind::OriginalTimestamp)?,
            offset: OffsetState::Known {
                seconds_east_of_utc,
            },
            instant: Some(instant),
        })
    }

    pub fn original(&self) -> Option<&str> {
        self.original.as_deref()
    }

    pub fn offset(&self) -> OffsetState {
        self.offset
    }

    pub fn instant(&self) -> Option<SystemTime> {
        self.instant
    }
}

/// Evidence attached to an observation. Missing support is an absent value, not a success.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceSupport {
    reference: Option<EvidenceReference>,
    method: EvidenceMethod,
    coverage: ClaimCoverage,
}

impl EvidenceSupport {
    pub fn new(
        reference: Option<EvidenceReference>,
        method: EvidenceMethod,
        coverage: ClaimCoverage,
    ) -> Self {
        Self {
            reference,
            method,
            coverage,
        }
    }

    pub fn reference(&self) -> Option<&EvidenceReference> {
        self.reference.as_ref()
    }

    pub fn method(&self) -> EvidenceMethod {
        self.method
    }

    pub fn coverage(&self) -> ClaimCoverage {
        self.coverage
    }
}

/// One immutable observation. Later checks are new values.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Observation {
    reference: ObservationRef,
    subject: SubjectIdentity,
    language: LanguageEvidence,
    observed_at: TimestampAssertion,
    access: AccessResult,
    listing: ListingDisposition,
    availability: SourceAvailability,
    source_statement: Option<SourceStatement>,
    evidence: Option<EvidenceSupport>,
}

impl Observation {
    /// Assembles already checked parts. Dimension combinations stay representable.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        reference: ObservationRef,
        subject: SubjectIdentity,
        language: LanguageEvidence,
        observed_at: TimestampAssertion,
        access: AccessResult,
        listing: ListingDisposition,
        availability: SourceAvailability,
        source_statement: Option<SourceStatement>,
        evidence: Option<EvidenceSupport>,
    ) -> Self {
        Self {
            reference,
            subject,
            language,
            observed_at,
            access,
            listing,
            availability,
            source_statement,
            evidence,
        }
    }

    /// Records a failed access attempt as a new observation.
    ///
    /// Availability is unknown, there is no source statement, and the listing is
    /// uncertain. `PageGone` instead records a removed listing. `Readable` is rejected.
    pub fn failed_check(
        reference: ObservationRef,
        subject: SubjectIdentity,
        language: LanguageEvidence,
        observed_at: TimestampAssertion,
        access: AccessResult,
        evidence: Option<EvidenceSupport>,
    ) -> Result<Self, DomainError> {
        let listing = match access {
            AccessResult::Readable => return Err(DomainError::NotAnAccessFailure(access)),
            AccessResult::PageGone => ListingDisposition::Removed,
            AccessResult::RateLimited
            | AccessResult::Captcha
            | AccessResult::AuthenticationBlocked
            | AccessResult::Timeout
            | AccessResult::TransportError => ListingDisposition::Uncertain,
        };
        Ok(Self::new(
            reference,
            subject,
            language,
            observed_at,
            access,
            listing,
            SourceAvailability::Unknown,
            None,
            evidence,
        ))
    }

    pub fn reference(&self) -> ObservationRef {
        self.reference
    }

    pub fn subject(&self) -> SubjectIdentity {
        self.subject
    }

    pub fn language(&self) -> LanguageEvidence {
        self.language
    }

    pub fn observed_at(&self) -> &TimestampAssertion {
        &self.observed_at
    }

    pub fn access(&self) -> AccessResult {
        self.access
    }

    pub fn listing(&self) -> ListingDisposition {
        self.listing
    }

    pub fn availability(&self) -> SourceAvailability {
        self.availability
    }

    pub fn source_statement(&self) -> Option<&SourceStatement> {
        self.source_statement.as_ref()
    }

    pub fn evidence(&self) -> Option<&EvidenceSupport> {
        self.evidence.as_ref()
    }
}

/// Explicit assessment context. There is no default that clears a blocker.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerificationContext {
    observation: ObservationRef,
    knowledge: KnowledgeAssessment,
    reviewer_approval: Option<ReviewerApproval>,
    conflict: ConflictState,
    recheck: RecheckState,
}

impl VerificationContext {
    pub fn new(
        observation: ObservationRef,
        knowledge: KnowledgeAssessment,
        reviewer_approval: Option<ReviewerApproval>,
        conflict: ConflictState,
        recheck: RecheckState,
    ) -> Self {
        Self {
            observation,
            knowledge,
            reviewer_approval,
            conflict,
            recheck,
        }
    }

    pub fn observation(&self) -> ObservationRef {
        self.observation
    }

    pub fn knowledge(&self) -> KnowledgeAssessment {
        self.knowledge
    }

    pub fn reviewer_approval(&self) -> Option<&ReviewerApproval> {
        self.reviewer_approval.as_ref()
    }

    pub fn conflict(&self) -> ConflictState {
        self.conflict
    }

    pub fn recheck(&self) -> RecheckState {
        self.recheck
    }
}
