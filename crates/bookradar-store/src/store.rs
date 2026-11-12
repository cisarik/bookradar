//! Exclusive SQLite creation, checked encoding, and append-only writes.

use std::fs::OpenOptions;
use std::io::{ErrorKind, Read};
use std::path::Path;
use std::time::{Duration, SystemTime};

use bookradar_core::{
    AccessResult, ClaimCoverage, ClaimCoverageState, DomainError, EditionId, EvidenceMethod,
    EvidenceReference, EvidenceSupport, KnowledgeAssessment, LanguageEvidence, ListingDisposition,
    Observation, ObservationId, ObservationRef, OfferId, OfferRef, OffsetState, ReviewerApproval,
    SourceAvailability, SourceId, SourceStatement, SubjectIdentity, TimestampAssertion,
};
use rusqlite::{Connection, ErrorCode, OpenFlags, TransactionBehavior, params};

use crate::history::{self, CurrentView, HistoryEntry};

const APPLICATION_ID: i64 = 0x4252_4452;
const USER_VERSION: i64 = 1;
const SCHEMA_SQL: &str = include_str!("schema_v1.sql");

const EXPECTED_OBJECTS: &[(&str, &str)] = &[
    ("index", "observations_by_time"),
    ("table", "editions"),
    ("table", "observations"),
    ("table", "offers"),
    ("table", "reviews"),
    ("table", "sources"),
    ("trigger", "observations_no_delete"),
    ("trigger", "observations_no_update"),
    ("trigger", "reviews_no_delete"),
    ("trigger", "reviews_no_update"),
];

/// How an existing database may be opened.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OpenMode {
    ReadOnly,
    ReadWrite,
}

/// A storage failure. Library paths return this instead of panicking.
#[derive(Debug)]
pub enum StoreError {
    AlreadyExists,
    NotFound,
    UnsupportedFormat { reason: &'static str },
    DuplicateKey,
    OutOfStorageRange,
    ReviewRevisionConflict,
    InvalidReview { reason: &'static str },
    EmptyReference,
    ReadOnly,
    Io(std::io::Error),
    Sqlite(rusqlite::Error),
    Domain(DomainError),
    AppendOnly,
    Constraint,
    Busy,
}

impl StoreError {
    /// Stable category used by the synthetic CLI.
    pub fn category(&self) -> &'static str {
        match self {
            Self::AlreadyExists => "already-exists",
            Self::NotFound => "not-found",
            Self::UnsupportedFormat { .. } => "unsupported-schema",
            Self::DuplicateKey => "duplicate-key",
            Self::OutOfStorageRange => "storage-range",
            Self::ReviewRevisionConflict => "review-conflict",
            Self::InvalidReview { .. }
            | Self::EmptyReference
            | Self::ReadOnly
            | Self::Sqlite(_)
            | Self::Domain(_)
            | Self::AppendOnly
            | Self::Constraint
            | Self::Busy => "storage",
            Self::Io(_) => "filesystem",
        }
    }
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyExists => formatter.write_str("database path already exists"),
            Self::NotFound => formatter.write_str("database path does not exist"),
            Self::UnsupportedFormat { reason } => {
                write!(formatter, "unsupported database format ({reason})")
            }
            Self::DuplicateKey => formatter.write_str("observation key already exists"),
            Self::OutOfStorageRange => {
                formatter.write_str("value is outside the stored integer range")
            }
            Self::ReviewRevisionConflict => formatter.write_str("review revision does not match"),
            Self::InvalidReview { reason } => write!(formatter, "invalid review ({reason})"),
            Self::EmptyReference => formatter.write_str("empty required reference"),
            Self::ReadOnly => formatter.write_str("database is open read-only"),
            Self::Io(error) => write!(formatter, "filesystem error: {error}"),
            Self::Sqlite(error) => write!(formatter, "sqlite error: {error}"),
            Self::Domain(error) => write!(formatter, "domain error: {error}"),
            Self::AppendOnly => formatter.write_str("append-only table rejected a mutation"),
            Self::Constraint => formatter.write_str("database constraint rejected the write"),
            Self::Busy => formatter.write_str("database is busy"),
        }
    }
}

impl std::error::Error for StoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Sqlite(error) => Some(error),
            Self::Domain(error) => Some(error),
            _ => None,
        }
    }
}

/// One immutable observation plus the store-owned provenance around it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObservationRecord {
    observation: Observation,
    ingested_at: SystemTime,
    captured_at: TimestampAssertion,
    requested_url: Option<String>,
    resolved_url: Option<String>,
    redirect_reference: Option<String>,
    provenance_reference: String,
    uncertainty_rationale: Option<String>,
}

impl ObservationRecord {
    /// Checks store-owned text. Core constructors have already checked the observation.
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        observation: Observation,
        ingested_at: SystemTime,
        captured_at: TimestampAssertion,
        requested_url: Option<String>,
        resolved_url: Option<String>,
        redirect_reference: Option<String>,
        provenance_reference: impl Into<String>,
        uncertainty_rationale: Option<String>,
    ) -> Result<Self, StoreError> {
        let provenance_reference = provenance_reference.into();
        require_text(&provenance_reference)?;
        require_optional(requested_url.as_deref())?;
        require_optional(resolved_url.as_deref())?;
        require_optional(redirect_reference.as_deref())?;
        require_optional(uncertainty_rationale.as_deref())?;
        require_optional(captured_at.original())?;
        Ok(Self {
            observation,
            ingested_at,
            captured_at,
            requested_url,
            resolved_url,
            redirect_reference,
            provenance_reference,
            uncertainty_rationale,
        })
    }

    pub fn observation(&self) -> &Observation {
        &self.observation
    }

    pub fn ingested_at(&self) -> SystemTime {
        self.ingested_at
    }

    pub fn captured_at(&self) -> &TimestampAssertion {
        &self.captured_at
    }

    pub fn requested_url(&self) -> Option<&str> {
        self.requested_url.as_deref()
    }

    pub fn resolved_url(&self) -> Option<&str> {
        self.resolved_url.as_deref()
    }

    pub fn redirect_reference(&self) -> Option<&str> {
        self.redirect_reference.as_deref()
    }

    pub fn provenance_reference(&self) -> &str {
        &self.provenance_reference
    }

    pub fn uncertainty_rationale(&self) -> Option<&str> {
        self.uncertainty_rationale.as_deref()
    }
}

/// An attributed assessment revision supplied by the caller.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewDraft {
    knowledge: KnowledgeAssessment,
    approval: Option<ReviewerApproval>,
    assessor_reference: String,
    rationale: String,
    assessed_at: SystemTime,
    recorded_at: SystemTime,
}

impl ReviewDraft {
    pub fn try_new(
        knowledge: KnowledgeAssessment,
        approval: Option<ReviewerApproval>,
        assessor_reference: impl Into<String>,
        rationale: impl Into<String>,
        assessed_at: SystemTime,
        recorded_at: SystemTime,
    ) -> Result<Self, StoreError> {
        let assessor_reference = assessor_reference.into();
        let rationale = rationale.into();
        require_text(&assessor_reference)?;
        require_text(&rationale)?;
        Ok(Self {
            knowledge,
            approval,
            assessor_reference,
            rationale,
            assessed_at,
            recorded_at,
        })
    }

    pub fn knowledge(&self) -> KnowledgeAssessment {
        self.knowledge
    }

    pub fn approval(&self) -> Option<&ReviewerApproval> {
        self.approval.as_ref()
    }

    pub fn assessor_reference(&self) -> &str {
        &self.assessor_reference
    }

    pub fn rationale(&self) -> &str {
        &self.rationale
    }

    pub fn assessed_at(&self) -> SystemTime {
        self.assessed_at
    }

    pub fn recorded_at(&self) -> SystemTime {
        self.recorded_at
    }
}

/// One stored assessment revision. Revision numbers start at 1.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewRevision {
    revision: u64,
    knowledge: KnowledgeAssessment,
    approval: Option<ReviewerApproval>,
    assessor_reference: String,
    rationale: String,
    assessed_at: SystemTime,
    recorded_at: SystemTime,
}

impl ReviewRevision {
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn knowledge(&self) -> KnowledgeAssessment {
        self.knowledge
    }

    pub fn approval(&self) -> Option<&ReviewerApproval> {
        self.approval.as_ref()
    }

    pub fn assessor_reference(&self) -> &str {
        &self.assessor_reference
    }

    pub fn rationale(&self) -> &str {
        &self.rationale
    }

    pub fn assessed_at(&self) -> SystemTime {
        self.assessed_at
    }

    pub fn recorded_at(&self) -> SystemTime {
        self.recorded_at
    }
}

/// A local database connection. Each value owns one connection.
#[derive(Debug)]
pub struct Store {
    conn: Connection,
    writable: bool,
}

impl Store {
    /// Creates a version-1 database at a path that must not already exist.
    pub fn create(path: &Path) -> Result<Self, StoreError> {
        match OpenOptions::new().write(true).create_new(true).open(path) {
            Ok(file) => drop(file),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                return Err(StoreError::AlreadyExists);
            }
            Err(error) => return Err(StoreError::Io(error)),
        }
        let mut store = match Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE) {
            Ok(conn) => Store {
                conn,
                writable: true,
            },
            Err(error) => {
                let _ = std::fs::remove_file(path);
                return Err(map_sqlite(error));
            }
        };
        if let Err(error) = store.initialize() {
            drop(store);
            let _ = std::fs::remove_file(path);
            return Err(error);
        }
        Ok(store)
    }

    /// Opens an existing database. A missing path is not created.
    pub fn open(path: &Path, mode: OpenMode) -> Result<Self, StoreError> {
        if !path.exists() {
            return Err(StoreError::NotFound);
        }
        if path.is_dir() {
            return Err(StoreError::UnsupportedFormat {
                reason: "path-is-directory",
            });
        }
        ensure_expected_application_id(path, APPLICATION_ID)?;
        let flags = match mode {
            OpenMode::ReadOnly => OpenFlags::SQLITE_OPEN_READ_ONLY,
            OpenMode::ReadWrite => OpenFlags::SQLITE_OPEN_READ_WRITE,
        };
        let conn = Connection::open_with_flags(path, flags).map_err(map_open)?;
        let store = Store {
            conn,
            writable: matches!(mode, OpenMode::ReadWrite),
        };
        store.configure()?;
        store.validate()?;
        Ok(store)
    }

    /// Appends an observation and its optional initial review in one transaction.
    pub fn append(
        &mut self,
        record: &ObservationRecord,
        review: Option<&ReviewDraft>,
    ) -> Result<(), StoreError> {
        self.require_writable()?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_sqlite)?;
        write_observation(&tx, record, review)?;
        tx.commit().map_err(map_sqlite)
    }

    /// Appends the next review revision when `expected_revision` is current.
    ///
    /// The first revision uses `expected_revision == 0`.
    pub fn append_review(
        &mut self,
        observation: ObservationRef,
        expected_revision: u64,
        draft: &ReviewDraft,
    ) -> Result<u64, StoreError> {
        self.require_writable()?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_sqlite)?;
        let next = write_review(&tx, observation, expected_revision, draft)?;
        tx.commit().map_err(map_sqlite)?;
        Ok(next)
    }

    /// Returns every observation of one offer, ordered for display.
    pub fn history(&mut self, offer: OfferRef) -> Result<Vec<HistoryEntry>, StoreError> {
        let mut entries = self.load_offer(offer)?;
        history::sort_entries(&mut entries);
        Ok(entries)
    }

    /// Resolves the current candidate set and evaluates it through the kernel.
    pub fn current(
        &mut self,
        offer: OfferRef,
        target: EditionId,
        now: SystemTime,
    ) -> Result<CurrentView, StoreError> {
        let entries = self.history(offer)?;
        history::resolve(&entries, target, now)
    }

    fn initialize(&mut self) -> Result<(), StoreError> {
        self.configure()?;
        self.select_delete_journal()?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_sqlite)?;
        tx.execute_batch(SCHEMA_SQL).map_err(map_sqlite)?;
        tx.pragma_update(None, "application_id", APPLICATION_ID)
            .map_err(map_sqlite)?;
        tx.pragma_update(None, "user_version", USER_VERSION)
            .map_err(map_sqlite)?;
        tx.commit().map_err(map_sqlite)?;
        self.validate()
    }

    fn configure(&self) -> Result<(), StoreError> {
        configure_connection(&self.conn)
    }

    fn select_delete_journal(&self) -> Result<(), StoreError> {
        set_delete_journal(&self.conn)
    }

    fn validate(&self) -> Result<(), StoreError> {
        validate_database(
            &self.conn,
            self.writable,
            APPLICATION_ID,
            USER_VERSION,
            EXPECTED_OBJECTS,
        )
    }

    fn load_offer(&mut self, offer: OfferRef) -> Result<Vec<HistoryEntry>, StoreError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Deferred)
            .map_err(map_sqlite)?;
        let entries = load_history(&tx, offer)?;
        tx.commit().map_err(map_sqlite)?;
        Ok(entries)
    }

    fn require_writable(&self) -> Result<(), StoreError> {
        if self.writable {
            Ok(())
        } else {
            Err(StoreError::ReadOnly)
        }
    }
}

pub(crate) fn ensure_expected_application_id(path: &Path, expected: i64) -> Result<(), StoreError> {
    let mut file = std::fs::File::open(path).map_err(StoreError::Io)?;
    let mut header = [0u8; 72];
    let read = file.read(&mut header).map_err(StoreError::Io)?;
    if read < 72 || &header[..16] != b"SQLite format 3\0" {
        return Ok(());
    }
    let application_id = i64::from(u32::from_be_bytes([
        header[68], header[69], header[70], header[71],
    ]));
    if application_id != 0 && application_id != expected {
        return Err(StoreError::UnsupportedFormat {
            reason: "application-id",
        });
    }
    Ok(())
}

pub(crate) fn configure_connection(conn: &Connection) -> Result<(), StoreError> {
    conn.pragma_update(None, "foreign_keys", "ON")
        .map_err(map_sqlite)?;
    conn.busy_timeout(Duration::ZERO).map_err(map_sqlite)?;
    conn.pragma_update(None, "synchronous", "FULL")
        .map_err(map_sqlite)?;
    Ok(())
}

pub(crate) fn set_delete_journal(conn: &Connection) -> Result<(), StoreError> {
    let mode: String = conn
        .query_row("PRAGMA journal_mode = DELETE", [], |row| row.get(0))
        .map_err(map_sqlite)?;
    if mode.eq_ignore_ascii_case("delete") {
        Ok(())
    } else {
        Err(StoreError::UnsupportedFormat {
            reason: "journal-mode",
        })
    }
}

pub(crate) fn validate_database(
    conn: &Connection,
    writable: bool,
    application_id_expected: i64,
    user_version_expected: i64,
    expected_objects: &[(&str, &str)],
) -> Result<(), StoreError> {
    let journal: String = conn
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .map_err(map_sqlite)?;
    if !journal.eq_ignore_ascii_case("delete") {
        return Err(StoreError::UnsupportedFormat {
            reason: "journal-mode",
        });
    }
    let synchronous: i64 = conn
        .query_row("PRAGMA synchronous", [], |row| row.get(0))
        .map_err(map_sqlite)?;
    if synchronous != 2 {
        return Err(StoreError::UnsupportedFormat {
            reason: "synchronous",
        });
    }
    let foreign_keys: i64 = conn
        .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
        .map_err(map_sqlite)?;
    if foreign_keys != 1 {
        return Err(StoreError::UnsupportedFormat {
            reason: "foreign-keys",
        });
    }
    let mut quick = conn.prepare("PRAGMA quick_check").map_err(map_sqlite)?;
    let quick_rows = quick
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(map_sqlite)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(map_sqlite)?;
    if quick_rows != ["ok"] {
        return Err(StoreError::UnsupportedFormat {
            reason: "quick-check",
        });
    }
    let application_id: i64 = conn
        .query_row("PRAGMA application_id", [], |row| row.get(0))
        .map_err(map_sqlite)?;
    if application_id != application_id_expected {
        return Err(StoreError::UnsupportedFormat {
            reason: "application-id",
        });
    }
    let user_version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(map_sqlite)?;
    if user_version != user_version_expected {
        return Err(StoreError::UnsupportedFormat {
            reason: "user-version",
        });
    }
    let mut foreign = conn
        .prepare("PRAGMA foreign_key_check")
        .map_err(map_sqlite)?;
    let foreign_count = foreign
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(map_sqlite)?
        .count();
    if foreign_count > 0 {
        return Err(StoreError::UnsupportedFormat {
            reason: "foreign-key",
        });
    }
    let mut statement = conn
        .prepare(
            "SELECT type, name FROM sqlite_master
             WHERE name NOT LIKE 'sqlite_%'
             ORDER BY type, name",
        )
        .map_err(map_sqlite)?;
    let found = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(map_sqlite)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(map_sqlite)?;
    let expected: Vec<(String, String)> = expected_objects
        .iter()
        .map(|(kind, name)| ((*kind).to_owned(), (*name).to_owned()))
        .collect();
    if found != expected {
        return Err(StoreError::UnsupportedFormat {
            reason: "schema-objects",
        });
    }
    if !writable {
        conn.pragma_update(None, "query_only", "ON")
            .map_err(map_sqlite)?;
    }
    Ok(())
}

pub(crate) fn load_history(
    conn: &Connection,
    offer: OfferRef,
) -> Result<Vec<HistoryEntry>, StoreError> {
    let source = fit_id(offer.source().get())?;
    let offer_id = fit_id(offer.offer().get())?;
    let mut observations = conn
        .prepare(
            "SELECT source_id, offer_id, id, subject, edition_id, language,
                    observed_original, observed_offset, observed_s, observed_ns,
                    captured_original, captured_offset, captured_s, captured_ns,
                    ingested_s, ingested_ns, access, listing, availability,
                    source_statement, evidence_method, evidence_reference,
                    subject_covered, language_covered, availability_covered,
                    requested_url, resolved_url, redirect_reference,
                    provenance_reference, uncertainty_rationale
             FROM observations
             WHERE source_id = ?1 AND offer_id = ?2
             ORDER BY id",
        )
        .map_err(map_sqlite)?;
    let records = observations
        .query_map(params![source, offer_id], decode_observation_row)
        .map_err(map_sqlite)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(map_stored_row)?;
    let mut reviews = conn
        .prepare(
            "SELECT observation_id, revision, assessed_s, assessed_ns, recorded_s,
                    recorded_ns, knowledge, reviewer_approval, assessor_reference, rationale
             FROM reviews
             WHERE source_id = ?1 AND offer_id = ?2
             ORDER BY observation_id, revision",
        )
        .map_err(map_sqlite)?;
    let review_rows = reviews
        .query_map(params![source, offer_id], decode_review_row)
        .map_err(map_sqlite)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(map_stored_row)?;
    drop(reviews);
    drop(observations);

    let mut entries: Vec<HistoryEntry> = records.into_iter().map(HistoryEntry::bare).collect();
    for (observation_id, revision) in review_rows {
        let Some(entry) = entries.iter_mut().find(|entry| {
            entry.record().observation().reference().observation().get() == observation_id
        }) else {
            return Err(StoreError::UnsupportedFormat {
                reason: "orphan-review",
            });
        };
        entry.push_review(revision);
    }
    Ok(entries)
}

pub(crate) fn write_observation(
    conn: &Connection,
    record: &ObservationRecord,
    review: Option<&ReviewDraft>,
) -> Result<(), StoreError> {
    record.revalidate()?;
    if let Some(draft) = review {
        draft.revalidate()?;
    }
    let encoded = EncodedObservation::encode(record)?;
    let encoded_review = review
        .map(|draft| EncodedReview::encode(draft, 1))
        .transpose()?;
    insert_registries(conn, record)?;
    insert_observation(conn, &encoded)?;
    if let Some(encoded_review) = encoded_review.as_ref() {
        insert_review(conn, record.observation().reference(), encoded_review)?;
    }
    Ok(())
}

pub(crate) fn write_review(
    conn: &Connection,
    observation: ObservationRef,
    expected_revision: u64,
    draft: &ReviewDraft,
) -> Result<u64, StoreError> {
    draft.revalidate()?;
    let expected = i64::try_from(expected_revision).map_err(|_| StoreError::OutOfStorageRange)?;
    let (source, offer, id) = offer_key(observation)?;
    let current: Option<i64> = conn
        .query_row(
            "SELECT MAX(revision) FROM reviews
             WHERE source_id = ?1 AND offer_id = ?2 AND observation_id = ?3",
            params![source, offer, id],
            |row| row.get(0),
        )
        .map_err(map_sqlite)?;
    let current = current.unwrap_or(0);
    if current != expected {
        return Err(StoreError::ReviewRevisionConflict);
    }
    if current > 0 {
        let (previous_seconds, previous_nanos): (i64, i64) = conn
            .query_row(
                "SELECT assessed_s, assessed_ns FROM reviews
                 WHERE source_id = ?1 AND offer_id = ?2 AND observation_id = ?3
                   AND revision = ?4",
                params![source, offer, id, current],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(map_sqlite)?;
        let (seconds, nanos) = encode_instant(draft.assessed_at())?;
        if (seconds, i64::from(nanos)) < (previous_seconds, previous_nanos) {
            return Err(StoreError::InvalidReview {
                reason: "assessment-time-regressed",
            });
        }
    }
    let next = current
        .checked_add(1)
        .ok_or(StoreError::OutOfStorageRange)?;
    let mut encoded = EncodedReview::encode(draft, next)?;
    encoded.revision = next;
    insert_review(conn, observation, &encoded)?;
    u64::try_from(next).map_err(|_| StoreError::OutOfStorageRange)
}

impl ObservationRecord {
    fn revalidate(&self) -> Result<(), StoreError> {
        require_text(&self.provenance_reference)?;
        require_optional(self.requested_url.as_deref())?;
        require_optional(self.resolved_url.as_deref())?;
        require_optional(self.redirect_reference.as_deref())?;
        require_optional(self.uncertainty_rationale.as_deref())?;
        require_optional(self.captured_at.original())?;
        require_optional(self.observation.observed_at().original())?;
        Ok(())
    }
}

impl ReviewDraft {
    fn revalidate(&self) -> Result<(), StoreError> {
        require_text(&self.assessor_reference)?;
        require_text(&self.rationale)
    }
}

struct EncodedTime {
    original: Option<String>,
    offset: Option<i32>,
    seconds: Option<i64>,
    nanos: Option<i64>,
}

struct EncodedObservation {
    source_id: i64,
    offer_id: i64,
    id: i64,
    subject: &'static str,
    edition_id: Option<i64>,
    language: &'static str,
    observed: EncodedTime,
    captured: EncodedTime,
    ingested_s: i64,
    ingested_ns: i64,
    access: &'static str,
    listing: &'static str,
    availability: &'static str,
    source_statement: Option<String>,
    evidence_method: Option<&'static str>,
    evidence_reference: Option<String>,
    subject_covered: Option<i64>,
    language_covered: Option<i64>,
    availability_covered: Option<i64>,
    requested_url: Option<String>,
    resolved_url: Option<String>,
    redirect_reference: Option<String>,
    provenance_reference: String,
    uncertainty_rationale: Option<String>,
}

impl EncodedObservation {
    fn encode(record: &ObservationRecord) -> Result<Self, StoreError> {
        let observation = record.observation();
        let reference = observation.reference();
        let (subject, edition_id) = encode_subject(observation.subject())?;
        let evidence = encode_evidence(observation.evidence())?;
        let (ingested_s, ingested_ns) = encode_instant(record.ingested_at())?;
        Ok(Self {
            source_id: fit_id(reference.offer().source().get())?,
            offer_id: fit_id(reference.offer().offer().get())?,
            id: fit_id(reference.observation().get())?,
            subject,
            edition_id,
            language: language_token(observation.language()),
            observed: encode_assertion(observation.observed_at())?,
            captured: encode_assertion(record.captured_at())?,
            ingested_s,
            ingested_ns: i64::from(ingested_ns),
            access: access_token(observation.access()),
            listing: listing_token(observation.listing()),
            availability: availability_token(observation.availability()),
            source_statement: observation
                .source_statement()
                .map(|statement| statement.as_str().to_owned()),
            evidence_method: evidence.method,
            evidence_reference: evidence.reference,
            subject_covered: evidence.subject,
            language_covered: evidence.language,
            availability_covered: evidence.availability,
            requested_url: record.requested_url.clone(),
            resolved_url: record.resolved_url.clone(),
            redirect_reference: record.redirect_reference.clone(),
            provenance_reference: record.provenance_reference.clone(),
            uncertainty_rationale: record.uncertainty_rationale.clone(),
        })
    }
}

struct EncodedReview {
    revision: i64,
    assessed_s: i64,
    assessed_ns: i64,
    recorded_s: i64,
    recorded_ns: i64,
    knowledge: &'static str,
    approval: Option<String>,
    assessor_reference: String,
    rationale: String,
}

impl EncodedReview {
    fn encode(draft: &ReviewDraft, revision: i64) -> Result<Self, StoreError> {
        let (assessed_s, assessed_ns) = encode_instant(draft.assessed_at())?;
        let (recorded_s, recorded_ns) = encode_instant(draft.recorded_at())?;
        Ok(Self {
            revision,
            assessed_s,
            assessed_ns: i64::from(assessed_ns),
            recorded_s,
            recorded_ns: i64::from(recorded_ns),
            knowledge: knowledge_token(draft.knowledge()),
            approval: draft
                .approval()
                .map(|approval| approval.as_str().to_owned()),
            assessor_reference: draft.assessor_reference.clone(),
            rationale: draft.rationale.clone(),
        })
    }
}

fn insert_registries(conn: &Connection, record: &ObservationRecord) -> Result<(), StoreError> {
    let reference = record.observation().reference();
    let source = fit_id(reference.offer().source().get())?;
    let offer = fit_id(reference.offer().offer().get())?;
    conn.execute(
        "INSERT INTO sources(id) VALUES (?1) ON CONFLICT(id) DO NOTHING",
        params![source],
    )
    .map_err(map_sqlite)?;
    if let SubjectIdentity::Resolved(edition) = record.observation().subject() {
        let edition = fit_id(edition.get())?;
        conn.execute(
            "INSERT INTO editions(id) VALUES (?1) ON CONFLICT(id) DO NOTHING",
            params![edition],
        )
        .map_err(map_sqlite)?;
    }
    conn.execute(
        "INSERT INTO offers(source_id, id) VALUES (?1, ?2)
         ON CONFLICT(source_id, id) DO NOTHING",
        params![source, offer],
    )
    .map_err(map_sqlite)?;
    Ok(())
}

fn insert_observation(conn: &Connection, row: &EncodedObservation) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO observations (
            source_id, offer_id, id, subject, edition_id, language,
            observed_original, observed_offset, observed_s, observed_ns,
            captured_original, captured_offset, captured_s, captured_ns,
            ingested_s, ingested_ns, access, listing, availability, source_statement,
            evidence_method, evidence_reference, subject_covered, language_covered,
            availability_covered, requested_url, resolved_url, redirect_reference,
            provenance_reference, uncertainty_rationale
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6,
            ?7, ?8, ?9, ?10,
            ?11, ?12, ?13, ?14,
            ?15, ?16, ?17, ?18, ?19, ?20,
            ?21, ?22, ?23, ?24,
            ?25, ?26, ?27, ?28,
            ?29, ?30
        )",
        params![
            row.source_id,
            row.offer_id,
            row.id,
            row.subject,
            row.edition_id,
            row.language,
            row.observed.original,
            row.observed.offset,
            row.observed.seconds,
            row.observed.nanos,
            row.captured.original,
            row.captured.offset,
            row.captured.seconds,
            row.captured.nanos,
            row.ingested_s,
            row.ingested_ns,
            row.access,
            row.listing,
            row.availability,
            row.source_statement,
            row.evidence_method,
            row.evidence_reference,
            row.subject_covered,
            row.language_covered,
            row.availability_covered,
            row.requested_url,
            row.resolved_url,
            row.redirect_reference,
            row.provenance_reference,
            row.uncertainty_rationale,
        ],
    )
    .map_err(map_sqlite)?;
    Ok(())
}

fn insert_review(
    conn: &Connection,
    observation: ObservationRef,
    review: &EncodedReview,
) -> Result<(), StoreError> {
    let (source, offer, id) = offer_key(observation)?;
    conn.execute(
        "INSERT INTO reviews (
            source_id, offer_id, observation_id, revision,
            assessed_s, assessed_ns, recorded_s, recorded_ns,
            knowledge, reviewer_approval, assessor_reference, rationale
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            source,
            offer,
            id,
            review.revision,
            review.assessed_s,
            review.assessed_ns,
            review.recorded_s,
            review.recorded_ns,
            review.knowledge,
            review.approval,
            review.assessor_reference,
            review.rationale,
        ],
    )
    .map_err(map_sqlite)?;
    Ok(())
}

fn decode_observation_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ObservationRecord> {
    let source_id: i64 = row.get(0)?;
    let offer_id: i64 = row.get(1)?;
    let id: i64 = row.get(2)?;
    let subject: String = row.get(3)?;
    let edition_id: Option<i64> = row.get(4)?;
    let language: String = row.get(5)?;
    let observed = read_encoded_time(row, 6)?;
    let captured = read_encoded_time(row, 10)?;
    let ingested_s: i64 = row.get(14)?;
    let ingested_ns: i64 = row.get(15)?;
    let access: String = row.get(16)?;
    let listing: String = row.get(17)?;
    let availability: String = row.get(18)?;
    let source_statement: Option<String> = row.get(19)?;
    let evidence_method: Option<String> = row.get(20)?;
    let evidence_reference: Option<String> = row.get(21)?;
    let subject_covered: Option<i64> = row.get(22)?;
    let language_covered: Option<i64> = row.get(23)?;
    let availability_covered: Option<i64> = row.get(24)?;
    let requested_url: Option<String> = row.get(25)?;
    let resolved_url: Option<String> = row.get(26)?;
    let redirect_reference: Option<String> = row.get(27)?;
    let provenance_reference: String = row.get(28)?;
    let uncertainty_rationale: Option<String> = row.get(29)?;
    decode_observation(
        source_id,
        offer_id,
        id,
        &subject,
        edition_id,
        &language,
        observed,
        captured,
        ingested_s,
        ingested_ns,
        &access,
        &listing,
        &availability,
        source_statement,
        evidence_method,
        evidence_reference,
        subject_covered,
        language_covered,
        availability_covered,
        requested_url,
        resolved_url,
        redirect_reference,
        provenance_reference,
        uncertainty_rationale,
    )
    .map_err(stored_row_error)
}

fn decode_review_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<(u64, ReviewRevision)> {
    let observation_id: i64 = row.get(0)?;
    let revision: i64 = row.get(1)?;
    let assessed_s: i64 = row.get(2)?;
    let assessed_ns: i64 = row.get(3)?;
    let recorded_s: i64 = row.get(4)?;
    let recorded_ns: i64 = row.get(5)?;
    let knowledge: String = row.get(6)?;
    let approval: Option<String> = row.get(7)?;
    let assessor_reference: String = row.get(8)?;
    let rationale: String = row.get(9)?;
    decode_review(
        observation_id,
        revision,
        assessed_s,
        assessed_ns,
        recorded_s,
        recorded_ns,
        &knowledge,
        approval,
        assessor_reference,
        rationale,
    )
    .map_err(stored_row_error)
}

#[allow(clippy::too_many_arguments)]
fn decode_observation(
    source_id: i64,
    offer_id: i64,
    id: i64,
    subject: &str,
    edition_id: Option<i64>,
    language: &str,
    observed: EncodedTime,
    captured: EncodedTime,
    ingested_s: i64,
    ingested_ns: i64,
    access: &str,
    listing: &str,
    availability: &str,
    source_statement: Option<String>,
    evidence_method: Option<String>,
    evidence_reference: Option<String>,
    subject_covered: Option<i64>,
    language_covered: Option<i64>,
    availability_covered: Option<i64>,
    requested_url: Option<String>,
    resolved_url: Option<String>,
    redirect_reference: Option<String>,
    provenance_reference: String,
    uncertainty_rationale: Option<String>,
) -> Result<ObservationRecord, StoreError> {
    let observation_ref = ObservationRef::new(
        OfferRef::new(id_new(source_id, "source")?, id_new_offer(offer_id)?),
        ObservationId::new(widen_id(id)?).map_err(|_| StoreError::UnsupportedFormat {
            reason: "observation-id",
        })?,
    );
    let observation = Observation::new(
        observation_ref,
        decode_subject(subject, edition_id)?,
        decode_language(language)?,
        decode_assertion(observed)?,
        decode_access(access)?,
        decode_listing(listing)?,
        decode_availability(availability)?,
        decode_statement(source_statement)?,
        decode_evidence(
            evidence_method.as_deref(),
            evidence_reference,
            subject_covered,
            language_covered,
            availability_covered,
        )?,
    );
    ObservationRecord::try_new(
        observation,
        decode_instant(ingested_s, ingested_ns)?,
        decode_assertion(captured)?,
        nonempty_optional(requested_url, "requested-url")?,
        nonempty_optional(resolved_url, "resolved-url")?,
        nonempty_optional(redirect_reference, "redirect-reference")?,
        nonempty_required(provenance_reference, "provenance")?,
        nonempty_optional(uncertainty_rationale, "uncertainty-rationale")?,
    )
}

#[allow(clippy::too_many_arguments)]
fn decode_review(
    observation_id: i64,
    revision: i64,
    assessed_s: i64,
    assessed_ns: i64,
    recorded_s: i64,
    recorded_ns: i64,
    knowledge: &str,
    approval: Option<String>,
    assessor_reference: String,
    rationale: String,
) -> Result<(u64, ReviewRevision), StoreError> {
    if revision <= 0 {
        return Err(StoreError::UnsupportedFormat {
            reason: "review-revision",
        });
    }
    let approval = match approval {
        None => None,
        Some(text) if text.is_empty() => {
            return Err(StoreError::UnsupportedFormat {
                reason: "reviewer-approval",
            });
        }
        Some(text) => {
            Some(
                ReviewerApproval::new(&text).map_err(|_| StoreError::UnsupportedFormat {
                    reason: "reviewer-approval",
                })?,
            )
        }
    };
    if assessor_reference.is_empty() || rationale.is_empty() {
        return Err(StoreError::UnsupportedFormat {
            reason: "review-text",
        });
    }
    Ok((
        widen_id(observation_id)?,
        ReviewRevision {
            revision: u64::try_from(revision).map_err(|_| StoreError::UnsupportedFormat {
                reason: "review-revision",
            })?,
            knowledge: decode_knowledge(knowledge)?,
            approval,
            assessor_reference,
            rationale,
            assessed_at: decode_instant(assessed_s, assessed_ns)?,
            recorded_at: decode_instant(recorded_s, recorded_ns)?,
        },
    ))
}

fn read_encoded_time(row: &rusqlite::Row<'_>, start: usize) -> rusqlite::Result<EncodedTime> {
    Ok(EncodedTime {
        original: row.get(start)?,
        offset: row.get(start + 1)?,
        seconds: row.get(start + 2)?,
        nanos: row.get(start + 3)?,
    })
}

pub(crate) fn encode_instant(instant: SystemTime) -> Result<(i64, u32), StoreError> {
    match instant.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(duration) => {
            let seconds =
                i64::try_from(duration.as_secs()).map_err(|_| StoreError::OutOfStorageRange)?;
            Ok((seconds, duration.subsec_nanos()))
        }
        Err(error) => {
            let duration = error.duration();
            let seconds = duration.as_secs();
            let nanos = duration.subsec_nanos();
            if nanos == 0 {
                let signed = i64::try_from(seconds).map_err(|_| StoreError::OutOfStorageRange)?;
                let signed = signed.checked_neg().ok_or(StoreError::OutOfStorageRange)?;
                Ok((signed, 0))
            } else {
                let whole = seconds
                    .checked_add(1)
                    .ok_or(StoreError::OutOfStorageRange)?;
                let signed = i64::try_from(whole).map_err(|_| StoreError::OutOfStorageRange)?;
                let signed = signed.checked_neg().ok_or(StoreError::OutOfStorageRange)?;
                Ok((signed, 1_000_000_000 - nanos))
            }
        }
    }
}

pub(crate) fn decode_instant(seconds: i64, nanos: i64) -> Result<SystemTime, StoreError> {
    if !(0..=999_999_999).contains(&nanos) {
        return Err(StoreError::UnsupportedFormat {
            reason: "nanoseconds",
        });
    }
    let nanos = u32::try_from(nanos).map_err(|_| StoreError::UnsupportedFormat {
        reason: "nanoseconds",
    })?;
    if seconds >= 0 {
        let seconds = u64::try_from(seconds)
            .map_err(|_| StoreError::UnsupportedFormat { reason: "instant" })?;
        SystemTime::UNIX_EPOCH
            .checked_add(Duration::new(seconds, nanos))
            .ok_or(StoreError::UnsupportedFormat { reason: "instant" })
    } else {
        let absolute = u64::try_from(
            seconds
                .checked_neg()
                .ok_or(StoreError::UnsupportedFormat { reason: "instant" })?,
        )
        .map_err(|_| StoreError::UnsupportedFormat { reason: "instant" })?;
        SystemTime::UNIX_EPOCH
            .checked_sub(Duration::new(absolute, 0))
            .ok_or(StoreError::UnsupportedFormat { reason: "instant" })?
            .checked_add(Duration::new(0, nanos))
            .ok_or(StoreError::UnsupportedFormat { reason: "instant" })
    }
}

fn encode_assertion(assertion: &TimestampAssertion) -> Result<EncodedTime, StoreError> {
    require_optional(assertion.original())?;
    let offset = match assertion.offset() {
        OffsetState::Known {
            seconds_east_of_utc,
        } => Some(seconds_east_of_utc),
        OffsetState::Unknown => None,
    };
    let (seconds, nanos) = match assertion.instant() {
        None => (None, None),
        Some(instant) => {
            let (seconds, nanos) = encode_instant(instant)?;
            (Some(seconds), Some(i64::from(nanos)))
        }
    };
    if offset.is_some() && seconds.is_none() {
        return Err(StoreError::UnsupportedFormat {
            reason: "offset-without-instant",
        });
    }
    Ok(EncodedTime {
        original: assertion.original().map(str::to_owned),
        offset,
        seconds,
        nanos,
    })
}

fn decode_assertion(encoded: EncodedTime) -> Result<TimestampAssertion, StoreError> {
    let original = nonempty_optional(encoded.original, "timestamp-original")?;
    let instant = match (encoded.seconds, encoded.nanos) {
        (None, None) => None,
        (Some(seconds), Some(nanos)) => Some(decode_instant(seconds, nanos)?),
        _ => {
            return Err(StoreError::UnsupportedFormat {
                reason: "partial-instant",
            });
        }
    };
    match encoded.offset {
        None => {
            TimestampAssertion::with_unknown_offset(original.as_deref(), instant).map_err(|_| {
                StoreError::UnsupportedFormat {
                    reason: "timestamp",
                }
            })
        }
        Some(seconds_east) => {
            let instant = instant.ok_or(StoreError::UnsupportedFormat {
                reason: "offset-without-instant",
            })?;
            TimestampAssertion::known(original.as_deref(), seconds_east, instant).map_err(|_| {
                StoreError::UnsupportedFormat {
                    reason: "timestamp",
                }
            })
        }
    }
}

fn encode_subject(subject: SubjectIdentity) -> Result<(&'static str, Option<i64>), StoreError> {
    match subject {
        SubjectIdentity::Resolved(edition) => Ok(("resolved", Some(fit_id(edition.get())?))),
        SubjectIdentity::Unresolved => Ok(("unresolved", None)),
        SubjectIdentity::Conflicting => Ok(("conflicting", None)),
    }
}

fn decode_subject(subject: &str, edition_id: Option<i64>) -> Result<SubjectIdentity, StoreError> {
    match (subject, edition_id) {
        ("resolved", Some(id)) => Ok(SubjectIdentity::Resolved(
            EditionId::new(widen_id(id)?).map_err(|_| StoreError::UnsupportedFormat {
                reason: "edition-id",
            })?,
        )),
        ("unresolved", None) => Ok(SubjectIdentity::Unresolved),
        ("conflicting", None) => Ok(SubjectIdentity::Conflicting),
        _ => Err(StoreError::UnsupportedFormat { reason: "subject" }),
    }
}

struct EncodedEvidence {
    method: Option<&'static str>,
    reference: Option<String>,
    subject: Option<i64>,
    language: Option<i64>,
    availability: Option<i64>,
}

fn encode_evidence(evidence: Option<&EvidenceSupport>) -> Result<EncodedEvidence, StoreError> {
    let Some(evidence) = evidence else {
        return Ok(EncodedEvidence {
            method: None,
            reference: None,
            subject: None,
            language: None,
            availability: None,
        });
    };
    Ok(EncodedEvidence {
        method: Some(method_token(evidence.method())),
        reference: evidence
            .reference()
            .map(|reference| reference.as_str().to_owned()),
        subject: Some(coverage_bit(evidence.coverage().subject())),
        language: Some(coverage_bit(evidence.coverage().language())),
        availability: Some(coverage_bit(evidence.coverage().source_availability())),
    })
}

fn decode_evidence(
    method: Option<&str>,
    reference: Option<String>,
    subject: Option<i64>,
    language: Option<i64>,
    availability: Option<i64>,
) -> Result<Option<EvidenceSupport>, StoreError> {
    if method.is_none()
        && reference.is_none()
        && subject.is_none()
        && language.is_none()
        && availability.is_none()
    {
        return Ok(None);
    }
    let Some(method) = method else {
        return Err(StoreError::UnsupportedFormat { reason: "evidence" });
    };
    let reference = match reference {
        None => None,
        Some(text) if text.is_empty() => {
            return Err(StoreError::UnsupportedFormat {
                reason: "evidence-reference",
            });
        }
        Some(text) => {
            Some(
                EvidenceReference::new(&text).map_err(|_| StoreError::UnsupportedFormat {
                    reason: "evidence-reference",
                })?,
            )
        }
    };
    Ok(Some(EvidenceSupport::new(
        reference,
        decode_method(method)?,
        ClaimCoverage::new(
            decode_coverage(subject)?,
            decode_coverage(language)?,
            decode_coverage(availability)?,
        ),
    )))
}

fn decode_statement(statement: Option<String>) -> Result<Option<SourceStatement>, StoreError> {
    match statement {
        None => Ok(None),
        Some(text) if text.is_empty() => Err(StoreError::UnsupportedFormat {
            reason: "source-statement",
        }),
        Some(text) => {
            SourceStatement::new(&text)
                .map(Some)
                .map_err(|_| StoreError::UnsupportedFormat {
                    reason: "source-statement",
                })
        }
    }
}

fn offer_key(observation: ObservationRef) -> Result<(i64, i64, i64), StoreError> {
    Ok((
        fit_id(observation.offer().source().get())?,
        fit_id(observation.offer().offer().get())?,
        fit_id(observation.observation().get())?,
    ))
}

fn id_new(value: i64, reason: &'static str) -> Result<SourceId, StoreError> {
    SourceId::new(widen_id(value)?).map_err(|_| StoreError::UnsupportedFormat { reason })
}

fn id_new_offer(value: i64) -> Result<OfferId, StoreError> {
    OfferId::new(widen_id(value)?).map_err(|_| StoreError::UnsupportedFormat { reason: "offer-id" })
}

fn fit_id(value: u64) -> Result<i64, StoreError> {
    i64::try_from(value).map_err(|_| StoreError::OutOfStorageRange)
}

fn widen_id(value: i64) -> Result<u64, StoreError> {
    u64::try_from(value)
        .ok()
        .filter(|id| *id > 0)
        .ok_or(StoreError::UnsupportedFormat {
            reason: "identifier",
        })
}

fn require_text(value: &str) -> Result<(), StoreError> {
    if value.is_empty() || value.contains('\0') {
        Err(StoreError::EmptyReference)
    } else {
        Ok(())
    }
}

fn require_optional(value: Option<&str>) -> Result<(), StoreError> {
    match value {
        None => Ok(()),
        Some(text) => require_text(text),
    }
}

fn nonempty_optional(
    value: Option<String>,
    reason: &'static str,
) -> Result<Option<String>, StoreError> {
    match value {
        None => Ok(None),
        Some(text) if text.is_empty() || text.contains('\0') => {
            Err(StoreError::UnsupportedFormat { reason })
        }
        Some(text) => Ok(Some(text)),
    }
}

fn nonempty_required(value: String, reason: &'static str) -> Result<String, StoreError> {
    match nonempty_optional(Some(value), reason)? {
        Some(text) => Ok(text),
        None => Err(StoreError::UnsupportedFormat { reason }),
    }
}

fn coverage_bit(state: ClaimCoverageState) -> i64 {
    match state {
        ClaimCoverageState::Covered => 1,
        ClaimCoverageState::Missing => 0,
    }
}

fn decode_coverage(value: Option<i64>) -> Result<ClaimCoverageState, StoreError> {
    match value {
        Some(1) => Ok(ClaimCoverageState::Covered),
        Some(0) => Ok(ClaimCoverageState::Missing),
        _ => Err(StoreError::UnsupportedFormat { reason: "coverage" }),
    }
}

pub(crate) fn access_token(access: AccessResult) -> &'static str {
    match access {
        AccessResult::Readable => "readable",
        AccessResult::RateLimited => "rate-limited",
        AccessResult::Captcha => "captcha",
        AccessResult::AuthenticationBlocked => "authentication-blocked",
        AccessResult::Timeout => "timeout",
        AccessResult::TransportError => "transport-error",
        AccessResult::PageGone => "page-gone",
    }
}

fn decode_access(value: &str) -> Result<AccessResult, StoreError> {
    Ok(match value {
        "readable" => AccessResult::Readable,
        "rate-limited" => AccessResult::RateLimited,
        "captcha" => AccessResult::Captcha,
        "authentication-blocked" => AccessResult::AuthenticationBlocked,
        "timeout" => AccessResult::Timeout,
        "transport-error" => AccessResult::TransportError,
        "page-gone" => AccessResult::PageGone,
        _ => {
            return Err(StoreError::UnsupportedFormat { reason: "access" });
        }
    })
}

pub(crate) fn listing_token(listing: ListingDisposition) -> &'static str {
    match listing {
        ListingDisposition::Present => "present",
        ListingDisposition::Removed => "removed",
        ListingDisposition::Uncertain => "uncertain",
    }
}

fn decode_listing(value: &str) -> Result<ListingDisposition, StoreError> {
    Ok(match value {
        "present" => ListingDisposition::Present,
        "removed" => ListingDisposition::Removed,
        "uncertain" => ListingDisposition::Uncertain,
        _ => {
            return Err(StoreError::UnsupportedFormat { reason: "listing" });
        }
    })
}

pub(crate) fn availability_token(availability: SourceAvailability) -> &'static str {
    match availability {
        SourceAvailability::Available => "available",
        SourceAvailability::Reserved => "reserved",
        SourceAvailability::Unavailable => "unavailable",
        SourceAvailability::Unknown => "unknown",
    }
}

fn decode_availability(value: &str) -> Result<SourceAvailability, StoreError> {
    Ok(match value {
        "available" => SourceAvailability::Available,
        "reserved" => SourceAvailability::Reserved,
        "unavailable" => SourceAvailability::Unavailable,
        "unknown" => SourceAvailability::Unknown,
        _ => {
            return Err(StoreError::UnsupportedFormat {
                reason: "availability",
            });
        }
    })
}

pub(crate) fn language_token(language: LanguageEvidence) -> &'static str {
    match language {
        LanguageEvidence::Slovak => "slovak",
        LanguageEvidence::Czech => "czech",
        LanguageEvidence::Mixed => "mixed",
        LanguageEvidence::Unknown => "unknown",
        LanguageEvidence::Conflicting => "conflicting",
    }
}

fn decode_language(value: &str) -> Result<LanguageEvidence, StoreError> {
    Ok(match value {
        "slovak" => LanguageEvidence::Slovak,
        "czech" => LanguageEvidence::Czech,
        "mixed" => LanguageEvidence::Mixed,
        "unknown" => LanguageEvidence::Unknown,
        "conflicting" => LanguageEvidence::Conflicting,
        _ => {
            return Err(StoreError::UnsupportedFormat { reason: "language" });
        }
    })
}

fn method_token(method: EvidenceMethod) -> &'static str {
    match method {
        EvidenceMethod::DirectBrowser => "direct-browser",
        EvidenceMethod::Manual => "manual",
        EvidenceMethod::AgentOnly => "agent-only",
    }
}

fn decode_method(value: &str) -> Result<EvidenceMethod, StoreError> {
    Ok(match value {
        "direct-browser" => EvidenceMethod::DirectBrowser,
        "manual" => EvidenceMethod::Manual,
        "agent-only" => EvidenceMethod::AgentOnly,
        _ => {
            return Err(StoreError::UnsupportedFormat {
                reason: "evidence-method",
            });
        }
    })
}

pub(crate) fn knowledge_token(knowledge: KnowledgeAssessment) -> &'static str {
    match knowledge {
        KnowledgeAssessment::Unreviewed => "unreviewed",
        KnowledgeAssessment::Supported => "supported",
        KnowledgeAssessment::Disputed => "disputed",
        KnowledgeAssessment::Rejected => "rejected",
    }
}

fn decode_knowledge(value: &str) -> Result<KnowledgeAssessment, StoreError> {
    Ok(match value {
        "unreviewed" => KnowledgeAssessment::Unreviewed,
        "supported" => KnowledgeAssessment::Supported,
        "disputed" => KnowledgeAssessment::Disputed,
        "rejected" => KnowledgeAssessment::Rejected,
        _ => {
            return Err(StoreError::UnsupportedFormat {
                reason: "knowledge",
            });
        }
    })
}

pub(crate) fn map_sqlite(error: rusqlite::Error) -> StoreError {
    match error {
        rusqlite::Error::SqliteFailure(sqlite, message) => map_failure(sqlite, message),
        other => StoreError::Sqlite(other),
    }
}

pub(crate) fn map_open(error: rusqlite::Error) -> StoreError {
    match error {
        rusqlite::Error::SqliteFailure(sqlite, _message)
            if sqlite.code == ErrorCode::NotADatabase
                || sqlite.code == ErrorCode::DatabaseCorrupt =>
        {
            StoreError::UnsupportedFormat {
                reason: "corrupt-database",
            }
        }
        rusqlite::Error::SqliteFailure(sqlite, message) if sqlite.code == ErrorCode::CannotOpen => {
            let _ = message;
            StoreError::NotFound
        }
        other => map_sqlite(other),
    }
}

fn map_failure(sqlite: rusqlite::ffi::Error, message: Option<String>) -> StoreError {
    if message
        .as_ref()
        .is_some_and(|text| text.contains("append-only"))
    {
        return StoreError::AppendOnly;
    }
    if sqlite.code == ErrorCode::DatabaseBusy || sqlite.code == ErrorCode::DatabaseLocked {
        return StoreError::Busy;
    }
    if sqlite.code == ErrorCode::NotADatabase || sqlite.code == ErrorCode::DatabaseCorrupt {
        return StoreError::UnsupportedFormat {
            reason: "corrupt-database",
        };
    }
    if sqlite.code == ErrorCode::ReadOnly {
        return StoreError::ReadOnly;
    }
    if sqlite.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_PRIMARYKEY
        || sqlite.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE
    {
        return StoreError::DuplicateKey;
    }
    if sqlite.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_TRIGGER {
        return StoreError::AppendOnly;
    }
    if sqlite.code == ErrorCode::ConstraintViolation {
        return StoreError::Constraint;
    }
    StoreError::Sqlite(rusqlite::Error::SqliteFailure(sqlite, message))
}

fn map_stored_row(error: rusqlite::Error) -> StoreError {
    match error {
        rusqlite::Error::ToSqlConversionFailure(inner)
        | rusqlite::Error::FromSqlConversionFailure(_, _, inner) => {
            if let Some(store) = inner.downcast_ref::<StoreError>() {
                return clone_store_error(store);
            }
            StoreError::UnsupportedFormat {
                reason: "malformed-row",
            }
        }
        other => map_sqlite(other),
    }
}

fn stored_row_error(error: StoreError) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(error))
}

fn clone_store_error(error: &StoreError) -> StoreError {
    match error {
        StoreError::AlreadyExists => StoreError::AlreadyExists,
        StoreError::NotFound => StoreError::NotFound,
        StoreError::UnsupportedFormat { reason } => StoreError::UnsupportedFormat { reason },
        StoreError::DuplicateKey => StoreError::DuplicateKey,
        StoreError::OutOfStorageRange => StoreError::OutOfStorageRange,
        StoreError::ReviewRevisionConflict => StoreError::ReviewRevisionConflict,
        StoreError::InvalidReview { reason } => StoreError::InvalidReview { reason },
        StoreError::EmptyReference => StoreError::EmptyReference,
        StoreError::ReadOnly => StoreError::ReadOnly,
        StoreError::AppendOnly => StoreError::AppendOnly,
        StoreError::Constraint => StoreError::Constraint,
        StoreError::Busy => StoreError::Busy,
        StoreError::Domain(error) => StoreError::Domain(*error),
        StoreError::Io(_) | StoreError::Sqlite(_) => StoreError::UnsupportedFormat {
            reason: "malformed-row",
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Fixture, append_fixture};
    use bookradar_core::{EditionId, Observation, ObservationId, ObservationRef, SubjectIdentity};

    struct Temp {
        dir: std::path::PathBuf,
    }

    impl Temp {
        fn new() -> Self {
            static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let dir = std::env::temp_dir()
                .join(format!("bookradar-store-unit-{}-{n}", std::process::id()));
            std::fs::create_dir(&dir).expect("temp");
            Self { dir }
        }

        fn path(&self, name: &str) -> std::path::PathBuf {
            self.dir.join(name)
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn count(store: &Store, sql: &str) -> i64 {
        store
            .conn
            .query_row(sql, [], |row| row.get(0))
            .expect("count")
    }

    #[test]
    fn pre_epoch_fraction_round_trips() {
        let instant = SystemTime::UNIX_EPOCH - Duration::from_millis(500);
        let (seconds, nanos) = encode_instant(instant).expect("encode");
        assert_eq!((seconds, nanos), (-1, 500_000_000));
        assert_eq!(
            decode_instant(seconds, i64::from(nanos)).expect("decode"),
            instant
        );
    }

    #[test]
    fn bundled_sqlite_version_is_present() {
        let version = crate::bundled_sqlite_version();
        assert!(!version.is_empty());
        eprintln!("bundled-sqlite {version}");
    }

    #[test]
    fn create_refuses_existing_path_and_open_does_not_create() {
        let temp = Temp::new();
        let path = temp.path("store.sqlite");
        std::fs::write(&path, b"").expect("empty");
        assert!(matches!(
            Store::create(&path),
            Err(StoreError::AlreadyExists)
        ));
        assert_eq!(std::fs::read(&path).expect("bytes"), b"");
        let missing = temp.path("missing.sqlite");
        assert!(matches!(
            Store::open(&missing, OpenMode::ReadWrite),
            Err(StoreError::NotFound)
        ));
        assert!(!missing.exists());
    }

    #[test]
    fn fresh_store_is_version_one_and_append_only() {
        let temp = Temp::new();
        let path = temp.path("store.sqlite");
        let mut store = Store::create(&path).expect("create");
        assert_eq!(count(&store, "PRAGMA user_version"), 1);
        assert_eq!(count(&store, "PRAGMA application_id"), APPLICATION_ID);
        assert_eq!(count(&store, "PRAGMA synchronous"), 2);
        append_fixture(&mut store, Fixture::Available).expect("append");
        let update = store.conn.execute(
            "UPDATE observations SET access = 'timeout' WHERE id = 1",
            [],
        );
        assert!(matches!(
            update.map_err(map_sqlite),
            Err(StoreError::AppendOnly)
        ));
        assert!(matches!(
            map_sqlite(
                store
                    .conn
                    .execute("DELETE FROM observations", [])
                    .expect_err("delete")
            ),
            StoreError::AppendOnly
        ));
        assert!(matches!(
            map_sqlite(
                store
                    .conn
                    .execute("DELETE FROM reviews", [])
                    .expect_err("delete review")
            ),
            StoreError::AppendOnly
        ));
        drop(store);
        let mut reopened = Store::open(&path, OpenMode::ReadOnly).expect("reopen");
        assert_eq!(
            reopened
                .history(crate::synthetic_offer())
                .expect("history")
                .len(),
            1
        );
        assert!(matches!(
            append_fixture(&mut reopened, Fixture::Blocked),
            Err(StoreError::ReadOnly)
        ));
    }

    #[test]
    fn unsupported_identity_version_corruption_and_foreign_keys_fail_closed() {
        let temp = Temp::new();
        let path = temp.path("store.sqlite");
        let store = Store::create(&path).expect("create");
        store
            .conn
            .execute("PRAGMA application_id = 1", [])
            .expect("application id");
        drop(store);
        assert!(matches!(
            Store::open(&path, OpenMode::ReadWrite),
            Err(StoreError::UnsupportedFormat {
                reason: "application-id"
            })
        ));

        let path = temp.path("version.sqlite");
        let store = Store::create(&path).expect("create");
        store
            .conn
            .execute("PRAGMA user_version = 0", [])
            .expect("version");
        drop(store);
        assert!(matches!(
            Store::open(&path, OpenMode::ReadOnly),
            Err(StoreError::UnsupportedFormat {
                reason: "user-version"
            })
        ));

        let path = temp.path("version2.sqlite");
        let store = Store::create(&path).expect("create");
        store
            .conn
            .execute("PRAGMA user_version = 2", [])
            .expect("version");
        drop(store);
        assert!(matches!(
            Store::open(&path, OpenMode::ReadWrite),
            Err(StoreError::UnsupportedFormat {
                reason: "user-version"
            })
        ));

        let path = temp.path("wal.sqlite");
        let store = Store::create(&path).expect("create");
        drop(store);
        let conn = Connection::open(&path).expect("raw");
        let mode: String = conn
            .query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))
            .expect("wal");
        assert_eq!(mode, "wal");
        drop(conn);
        assert!(matches!(
            Store::open(&path, OpenMode::ReadWrite),
            Err(StoreError::UnsupportedFormat {
                reason: "journal-mode"
            })
        ));

        let corrupt = temp.path("corrupt.sqlite");
        std::fs::write(&corrupt, b"not a database").expect("corrupt");
        assert!(matches!(
            Store::open(&corrupt, OpenMode::ReadOnly),
            Err(StoreError::UnsupportedFormat {
                reason: "corrupt-database"
            })
        ));

        let path = temp.path("fk.sqlite");
        let store = Store::create(&path).expect("create");
        store
            .conn
            .execute_batch("PRAGMA foreign_keys = OFF")
            .expect("fk off");
        store
            .conn
            .execute(
                "INSERT INTO observations (
                    source_id, offer_id, id, subject, language, ingested_s, ingested_ns,
                    access, listing, availability, provenance_reference
                 ) VALUES (4, 4, 4, 'unresolved', 'unknown', 1, 0, 'readable', 'present', 'unknown', 'synthetic:orphan')",
                [],
            )
            .expect("orphan");
        drop(store);
        assert!(matches!(
            Store::open(&path, OpenMode::ReadWrite),
            Err(StoreError::UnsupportedFormat {
                reason: "foreign-key"
            })
        ));
    }

    #[test]
    fn duplicate_append_rolls_back_new_registry_rows() {
        let temp = Temp::new();
        let mut store = Store::create(&temp.path("store.sqlite")).expect("create");
        append_fixture(&mut store, Fixture::Available).expect("append");
        let duplicate = duplicate_with_new_edition();
        let review = Fixture::Available.review();
        assert!(matches!(
            store.append(&duplicate, Some(&review)),
            Err(StoreError::DuplicateKey)
        ));
        assert_eq!(count(&store, "SELECT COUNT(*) FROM editions"), 1);
        assert_eq!(count(&store, "SELECT COUNT(*) FROM observations"), 1);
        let tx = store.conn.unchecked_transaction().expect("tx");
        tx.execute("INSERT INTO sources(id) VALUES (99)", [])
            .expect("insert");
        drop(tx);
        assert_eq!(
            count(&store, "SELECT COUNT(*) FROM sources WHERE id = 99"),
            0
        );
    }

    #[test]
    fn oversized_identifier_is_not_truncated() {
        let temp = Temp::new();
        let mut store = Store::create(&temp.path("store.sqlite")).expect("create");
        let record = oversized_record();
        assert!(matches!(
            store.append(&record, None),
            Err(StoreError::OutOfStorageRange)
        ));
        assert_eq!(count(&store, "SELECT COUNT(*) FROM observations"), 0);
        assert_eq!(count(&store, "SELECT COUNT(*) FROM sources"), 0);
    }

    fn duplicate_with_new_edition() -> ObservationRecord {
        let base = Fixture::Available.record();
        let observation = Observation::new(
            base.observation().reference(),
            SubjectIdentity::Resolved(EditionId::new(11).expect("edition")),
            base.observation().language(),
            base.observation().observed_at().clone(),
            base.observation().access(),
            base.observation().listing(),
            base.observation().availability(),
            base.observation().source_statement().cloned(),
            base.observation().evidence().cloned(),
        );
        ObservationRecord::try_new(
            observation,
            base.ingested_at(),
            base.captured_at().clone(),
            None,
            None,
            None,
            base.provenance_reference().to_owned(),
            None,
        )
        .expect("record")
    }

    fn oversized_record() -> ObservationRecord {
        let base = Fixture::Available.record();
        let observation = Observation::new(
            ObservationRef::new(
                base.observation().reference().offer(),
                ObservationId::new(1 << 63).expect("id"),
            ),
            base.observation().subject(),
            base.observation().language(),
            base.observation().observed_at().clone(),
            base.observation().access(),
            base.observation().listing(),
            base.observation().availability(),
            base.observation().source_statement().cloned(),
            base.observation().evidence().cloned(),
        );
        ObservationRecord::try_new(
            observation,
            base.ingested_at(),
            base.captured_at().clone(),
            None,
            None,
            None,
            "synthetic:oversized",
            None,
        )
        .expect("record")
    }
}
