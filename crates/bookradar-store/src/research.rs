//! Separate SQLite research ledger.
//!
//! Application id `0x42525249` keeps this file distinct from an observation
//! store. Candidate rows reuse the v1 observation tables inside this file only.

use std::fs::OpenOptions;
use std::io::ErrorKind;
use std::path::Path;
use std::time::SystemTime;

use bookradar_core::{
    ConflictState, EditionId, Eligibility, KnowledgeAssessment, Observation, ObservationId,
    ObservationRef, OfferId, OfferRef, RecheckState, SourceId,
};
use rusqlite::{
    Connection, OpenFlags, OptionalExtension, Transaction, TransactionBehavior, params,
};

use crate::history::{self, CurrentView};
use crate::research_contract::{
    CandidateDraft, Envelope, OfferTuple, RULE_VERSION, StructuralPosition, read_envelope,
};
use crate::store::{
    self, OpenMode, ReviewDraft, StoreError, configure_connection, ensure_expected_application_id,
    load_history, map_open, map_sqlite, set_delete_journal, validate_database, write_observation,
    write_review,
};

const APPLICATION_ID: i64 = 0x4252_5249;
const USER_VERSION: i64 = 1;
const SCHEMA_SQL: &str = concat!(
    include_str!("schema_v1.sql"),
    "\n",
    include_str!("schema_research_v1.sql")
);
const ASSESSOR: &str = "rule:research-admission-v1";

const EXPECTED_OBJECTS: &[(&str, &str)] = &[
    ("index", "observations_by_time"),
    ("index", "research_findings_by_identity"),
    ("table", "editions"),
    ("table", "observations"),
    ("table", "offers"),
    ("table", "research_findings"),
    ("table", "research_inputs"),
    ("table", "research_runs"),
    ("table", "reviews"),
    ("table", "sources"),
    ("trigger", "findings_no_delete"),
    ("trigger", "findings_no_update"),
    ("trigger", "inputs_no_delete"),
    ("trigger", "inputs_no_update"),
    ("trigger", "observations_no_delete"),
    ("trigger", "observations_no_update"),
    ("trigger", "reviews_no_delete"),
    ("trigger", "reviews_no_update"),
    ("trigger", "runs_no_delete"),
    ("trigger", "runs_no_update"),
];

#[derive(Debug)]
pub enum ResearchError {
    Store(StoreError),
    Oversized,
    InvalidArgument,
}

impl ResearchError {
    pub fn category(&self) -> &'static str {
        match self {
            Self::Store(error) => error.category(),
            Self::Oversized | Self::InvalidArgument => "usage",
        }
    }
}

impl std::fmt::Display for ResearchError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Store(error) => write!(formatter, "{error}"),
            Self::Oversized => formatter.write_str("input exceeds 1 MiB"),
            Self::InvalidArgument => formatter.write_str("invalid research argument"),
        }
    }
}

impl From<StoreError> for ResearchError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

pub struct ImportContext {
    ingested_at: SystemTime,
    abort_before_commit: bool,
}

impl ImportContext {
    pub fn new(ingested_at: SystemTime) -> Self {
        Self {
            ingested_at,
            abort_before_commit: false,
        }
    }

    /// Aborts after the import writes and before commit, so tests can observe rollback.
    pub fn with_abort_before_commit(mut self) -> Self {
        self.abort_before_commit = true;
        self
    }

    pub fn ingested_at(&self) -> SystemTime {
        self.ingested_at
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportReport {
    pub run: &'static str,
    pub seen: Option<u64>,
    pub admitted: u64,
    pub replayed: u64,
    pub rejected: u64,
    pub quarantined: u64,
    pub conflicts: u64,
    pub projected: u64,
}

pub fn render_import(report: &ImportReport) -> String {
    let seen = match report.seen {
        Some(value) => value.to_string(),
        None => "unknown".to_owned(),
    };
    format!(
        "run={} seen={seen} admitted={} replayed={} rejected={} quarantined={} conflicts={} projected={}",
        report.run,
        report.admitted,
        report.replayed,
        report.rejected,
        report.quarantined,
        report.conflicts,
        report.projected
    )
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResearchOfferKey {
    producer_id: String,
    source_ref: String,
    listing_ref: String,
    listing_generation: String,
}

impl ResearchOfferKey {
    pub fn try_new(
        producer_id: impl Into<String>,
        source_ref: impl Into<String>,
        listing_ref: impl Into<String>,
        listing_generation: impl Into<String>,
    ) -> Result<Self, ResearchError> {
        let key = Self {
            producer_id: producer_id.into(),
            source_ref: source_ref.into(),
            listing_ref: listing_ref.into(),
            listing_generation: listing_generation.into(),
        };
        if !crate::research_contract::is_identifier(&key.producer_id)
            || !crate::research_contract::is_identifier(&key.source_ref)
            || !crate::research_contract::is_identifier(&key.listing_ref)
            || !crate::research_contract::is_identifier(&key.listing_generation)
        {
            return Err(ResearchError::InvalidArgument);
        }
        Ok(key)
    }
}

/// Identity of one stored research finding. Position may be zero.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateQuery {
    pub producer_id: String,
    pub run_id: String,
    pub finding_id: String,
    pub receipt_id: u64,
    pub position: u64,
}

/// One ledger row addressed by receipt, position, and finding identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateRecord {
    pub producer_id: String,
    pub run_id: String,
    pub finding_id: String,
    pub receipt_id: u64,
    pub position: u64,
    pub outcome: String,
    pub reason: String,
    pub ledger_knowledge: Option<String>,
    pub canonical_finding: Option<Vec<u8>>,
    pub source_ref: Option<String>,
    pub listing_ref: Option<String>,
    pub listing_generation: Option<String>,
    pub ledger_source_id: Option<u64>,
    pub ledger_offer_id: Option<u64>,
    pub ledger_observation_id: Option<u64>,
}

/// Research producer, run, and finding identifiers use one grammar.
pub fn is_research_identifier(text: &str) -> bool {
    crate::research_contract::is_identifier(text)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InputView {
    pub receipt_id: u64,
    pub raw_bytes: Vec<u8>,
    pub canonical_bytes: Option<Vec<u8>>,
    pub disposition: String,
    pub reported_outcome: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FindingView {
    pub receipt_id: u64,
    pub position: u64,
    pub outcome: String,
    pub reason: String,
    pub knowledge: Option<String>,
    pub canonical_finding: Option<Vec<u8>>,
    pub observation_id: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResearchSnapshot {
    pub envelopes: u64,
    pub runs: u64,
    pub findings: u64,
    pub observations: u64,
    pub reviews: u64,
    pub editions: u64,
    pub inputs: Vec<InputView>,
    pub findings_rows: Vec<FindingView>,
}

pub fn render_inspect(snapshot: &ResearchSnapshot) -> String {
    let mut text = format!(
        "envelopes={} runs={} findings={} observations={} reviews={}\n",
        snapshot.envelopes,
        snapshot.runs,
        snapshot.findings,
        snapshot.observations,
        snapshot.reviews
    );
    for row in &snapshot.findings_rows {
        text.push_str(&format!(
            "input={} finding={} outcome={} reason={}",
            row.receipt_id, row.position, row.outcome, row.reason
        ));
        if let Some(knowledge) = &row.knowledge {
            text.push_str(&format!(" knowledge={knowledge}"));
        }
        text.push('\n');
    }
    text
}

pub fn render_research_current(view: &CurrentView) -> String {
    let verification = if view.candidates().iter().any(|candidate| {
        matches!(
            candidate.assessment().eligibility(),
            Eligibility::Eligible { .. }
        )
    }) {
        "eligible"
    } else {
        "not-eligible"
    };
    format!(
        "candidates={} conflict={} recheck={} verification={verification}",
        view.candidates().len(),
        conflict_token(view.conflict()),
        recheck_token(view.recheck())
    )
}

pub struct ResearchStore {
    conn: Connection,
    writable: bool,
}

impl ResearchStore {
    pub fn create(path: &Path) -> Result<Self, StoreError> {
        match OpenOptions::new().write(true).create_new(true).open(path) {
            Ok(file) => drop(file),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                return Err(StoreError::AlreadyExists);
            }
            Err(error) => return Err(StoreError::Io(error)),
        }
        let conn = match Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE) {
            Ok(conn) => conn,
            Err(error) => {
                let _ = std::fs::remove_file(path);
                return Err(map_sqlite(error));
            }
        };
        let mut store = Self {
            conn,
            writable: true,
        };
        if let Err(error) = store.initialize() {
            drop(store);
            let _ = std::fs::remove_file(path);
            return Err(error);
        }
        Ok(store)
    }

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
        let store = Self {
            conn,
            writable: matches!(mode, OpenMode::ReadWrite),
        };
        configure_connection(&store.conn)?;
        validate_database(
            &store.conn,
            store.writable,
            APPLICATION_ID,
            USER_VERSION,
            EXPECTED_OBJECTS,
        )?;
        Ok(store)
    }

    pub fn import(
        &mut self,
        bytes: &[u8],
        context: ImportContext,
    ) -> Result<ImportReport, ResearchError> {
        if bytes.len() > crate::research_contract::MAX_ENVELOPE_BYTES {
            return Err(ResearchError::Oversized);
        }
        self.require_writable()?;
        let (ingested_s, ingested_ns) = store::encode_instant(context.ingested_at)?;
        let ingested_ns = i64::from(ingested_ns);
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_sqlite)?;
        ensure_ingestion_advances(&tx, ingested_s, ingested_ns)?;
        let envelope = read_envelope(bytes);
        let report = match envelope {
            Envelope::Oversized => return Err(ResearchError::Oversized),
            Envelope::Opaque { reason, canonical } => {
                if opaque_already_stored(&tx, bytes, canonical.as_deref())? {
                    opaque_report()
                } else {
                    insert_receipt(
                        &tx,
                        bytes,
                        canonical.as_deref(),
                        ingested_s,
                        ingested_ns,
                        "quarantined",
                        None,
                        reason,
                    )?;
                    opaque_report()
                }
            }
            Envelope::Ready {
                canonical,
                producer_id,
                run_id,
                context_canonical,
                reported_outcome,
                positions,
            } => {
                if let Some(receipt_id) = find_canonical_receipt(&tx, &canonical)? {
                    stored_replay_report(&tx, receipt_id)?
                } else {
                    import_ready(
                        &tx,
                        bytes,
                        &canonical,
                        &producer_id,
                        &run_id,
                        &context_canonical,
                        reported_outcome,
                        positions,
                        context.ingested_at,
                        ingested_s,
                        ingested_ns,
                    )?
                }
            }
        };
        if context.abort_before_commit {
            return Err(ResearchError::Store(StoreError::Constraint));
        }
        tx.commit().map_err(map_sqlite)?;
        Ok(report)
    }

    pub fn inspect(&mut self) -> Result<ResearchSnapshot, StoreError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Deferred)
            .map_err(map_sqlite)?;
        let snapshot = read_snapshot(&tx)?;
        tx.commit().map_err(map_sqlite)?;
        Ok(snapshot)
    }

    /// Reads one finding without writing. `None` means the identity does not match.
    pub fn candidate(
        &mut self,
        query: &CandidateQuery,
    ) -> Result<Option<CandidateRecord>, StoreError> {
        let receipt = i64_id(query.receipt_id)?;
        let position = i64_id(query.position)?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Deferred)
            .map_err(map_sqlite)?;
        let row = tx
            .query_row(
                "SELECT f.producer_id, f.run_id, f.finding_id, f.outcome, f.reason,
                        f.canonical_finding, f.source_ref, f.listing_ref, f.listing_generation,
                        f.source_id, f.offer_id, f.observation_id,
                        CASE
                            WHEN f.observation_id IS NULL THEN f.knowledge
                            ELSE (
                                SELECT r.knowledge FROM reviews r
                                WHERE r.source_id = f.source_id AND r.offer_id = f.offer_id
                                  AND r.observation_id = f.observation_id
                                ORDER BY r.revision DESC LIMIT 1
                            )
                        END
                 FROM research_findings f
                 WHERE f.receipt_id = ?1 AND f.position = ?2
                   AND f.producer_id = ?3 AND f.run_id = ?4 AND f.finding_id = ?5",
                params![
                    receipt,
                    position,
                    query.producer_id,
                    query.run_id,
                    query.finding_id
                ],
                decode_candidate,
            )
            .optional()
            .map_err(map_sqlite)?;
        tx.commit().map_err(map_sqlite)?;
        Ok(row.map(|mut record| {
            record.receipt_id = query.receipt_id;
            record.position = query.position;
            record
        }))
    }

    pub fn current(
        &mut self,
        key: &ResearchOfferKey,
        target: EditionId,
        now: SystemTime,
    ) -> Result<CurrentView, StoreError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Deferred)
            .map_err(map_sqlite)?;
        let located = locate_offer(
            &tx,
            &key.producer_id,
            &key.source_ref,
            &key.listing_ref,
            &key.listing_generation,
        )?;
        let view = match located {
            None => history::resolve(&[], target, now)?,
            Some((source_id, offer_id)) => {
                let source = SourceId::new(u64_id(source_id)?).map_err(|_| {
                    StoreError::UnsupportedFormat {
                        reason: "source-id",
                    }
                })?;
                let offer = OfferId::new(u64_id(offer_id)?)
                    .map_err(|_| StoreError::UnsupportedFormat { reason: "offer-id" })?;
                let mut entries = load_history(&tx, OfferRef::new(source, offer))?;
                history::sort_entries(&mut entries);
                history::resolve(&entries, target, now)?
            }
        };
        tx.commit().map_err(map_sqlite)?;
        Ok(view)
    }

    fn initialize(&mut self) -> Result<(), StoreError> {
        configure_connection(&self.conn)?;
        set_delete_journal(&self.conn)?;
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
        validate_database(
            &self.conn,
            self.writable,
            APPLICATION_ID,
            USER_VERSION,
            EXPECTED_OBJECTS,
        )
    }

    fn require_writable(&self) -> Result<(), StoreError> {
        if self.writable {
            Ok(())
        } else {
            Err(StoreError::ReadOnly)
        }
    }
}

struct Planned {
    outcome: &'static str,
    reason: &'static str,
    finding_id: Option<String>,
    canonical: Option<Vec<u8>>,
    draft: Option<CandidateDraft>,
    tuple: Option<OfferTuple>,
    original: Option<(i64, i64)>,
    dispute_observation: Option<(i64, i64, i64)>,
}

#[allow(clippy::too_many_arguments)]
fn import_ready(
    tx: &Transaction<'_>,
    raw: &[u8],
    canonical: &[u8],
    producer_id: &str,
    run_id: &str,
    context_canonical: &[u8],
    reported_outcome: &str,
    positions: Vec<StructuralPosition>,
    ingested_at: SystemTime,
    ingested_s: i64,
    ingested_ns: i64,
) -> Result<ImportReport, ResearchError> {
    let context_conflict = run_context_conflicts(tx, producer_id, run_id, context_canonical)?;
    let mut planned = plan_positions(tx, producer_id, run_id, positions, context_conflict)?;
    let projected = count_drafts(&planned);
    let report = report_from_planned(&planned, projected);
    let receipt_id = insert_receipt(
        tx,
        raw,
        Some(canonical),
        ingested_s,
        ingested_ns,
        report.run,
        Some(reported_outcome),
        "none",
    )?;
    if !context_conflict {
        ensure_run(tx, producer_id, run_id, context_canonical, receipt_id)?;
    }
    let mut touched = Vec::new();
    for (position, row) in planned.iter_mut().enumerate() {
        let position = i64::try_from(position).map_err(|_| StoreError::OutOfStorageRange)?;
        if let Some(draft) = row.draft.take() {
            let tuple = row.tuple.clone().ok_or(StoreError::Constraint)?;
            let (source_id, offer_id, observation_id) = project_candidate(
                tx,
                producer_id,
                &tuple,
                draft,
                receipt_id,
                position,
                ingested_at,
            )?;
            touched.push((source_id, offer_id));
            insert_finding(
                tx,
                receipt_id,
                position,
                producer_id,
                run_id,
                row,
                Some((source_id, offer_id, observation_id)),
                Some("unreviewed"),
            )?;
        } else {
            insert_finding(
                tx,
                receipt_id,
                position,
                producer_id,
                run_id,
                row,
                None,
                knowledge_for(row),
            )?;
        }
    }
    if context_conflict {
        dispute_run(tx, producer_id, run_id, ingested_at, "run-context-conflict")?;
    }
    for row in &planned {
        if let Some((source, offer, observation)) = row.dispute_observation {
            dispute_one(
                tx,
                source,
                offer,
                observation,
                ingested_at,
                "identity-conflict",
            )?;
        }
    }
    for (source, offer) in touched {
        dispute_equal_time(tx, source, offer, ingested_at)?;
    }
    Ok(report)
}

fn plan_positions(
    tx: &Transaction<'_>,
    producer_id: &str,
    run_id: &str,
    positions: Vec<StructuralPosition>,
    context_conflict: bool,
) -> Result<Vec<Planned>, StoreError> {
    let mut planned = Vec::with_capacity(positions.len());
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for position in positions {
        match position {
            StructuralPosition::Rejected { reason } => planned.push(Planned {
                outcome: "rejected",
                reason,
                finding_id: None,
                canonical: None,
                draft: None,
                tuple: None,
                original: None,
                dispute_observation: None,
            }),
            StructuralPosition::Quarantined {
                reason,
                finding_id,
                canonical,
            } => {
                let slot = planned.len();
                planned.push(Planned {
                    outcome: "quarantined",
                    reason,
                    finding_id: finding_id.clone(),
                    canonical,
                    draft: None,
                    tuple: None,
                    original: None,
                    dispute_observation: None,
                });
                if finding_id.is_some() {
                    push_group(&mut groups, &planned, slot);
                }
            }
            StructuralPosition::Ready {
                finding_id,
                canonical,
                draft,
                tuple,
            } => {
                let slot = planned.len();
                planned.push(Planned {
                    outcome: "admitted",
                    reason: "none",
                    finding_id: Some(finding_id),
                    canonical: Some(canonical),
                    draft: draft.map(|draft| *draft),
                    tuple,
                    original: None,
                    dispute_observation: None,
                });
                push_group(&mut groups, &planned, slot);
            }
        }
    }
    for group in groups {
        resolve_group(tx, producer_id, run_id, &mut planned, &group)?;
    }
    if context_conflict {
        for row in &mut planned {
            if row.outcome != "rejected" {
                row.outcome = "quarantined";
                row.reason = "run-context-conflict";
                row.draft = None;
                row.dispute_observation = None;
            }
        }
    }
    Ok(planned)
}

fn push_group(groups: &mut Vec<Vec<usize>>, planned: &[Planned], slot: usize) {
    let Some(finding_id) = planned[slot].finding_id.as_deref() else {
        return;
    };
    if let Some(group) = groups
        .iter_mut()
        .find(|group| planned[group[0]].finding_id.as_deref() == Some(finding_id))
    {
        group.push(slot);
    } else {
        groups.push(vec![slot]);
    }
}

fn resolve_group(
    tx: &Transaction<'_>,
    producer_id: &str,
    run_id: &str,
    planned: &mut [Planned],
    group: &[usize],
) -> Result<(), StoreError> {
    let finding_id = planned[group[0]]
        .finding_id
        .clone()
        .ok_or(StoreError::Constraint)?;
    let mut distinct: Vec<&[u8]> = Vec::new();
    for slot in group {
        if let Some(canonical) = planned[*slot].canonical.as_deref()
            && !distinct.contains(&canonical)
        {
            distinct.push(canonical);
        }
    }
    let stored = stored_variants(tx, producer_id, run_id, &finding_id)?;
    if distinct.len() > 1 {
        let dispute = stored.iter().find_map(|variant| variant.observation);
        let original = stored
            .first()
            .map(|variant| (variant.receipt_id, variant.position));
        for slot in group {
            mark_conflict(&mut planned[*slot], original, dispute);
        }
        return Ok(());
    }
    let canonical = distinct.first().copied();
    if let Some(canonical) = canonical
        && let Some(variant) = stored.iter().find(|variant| variant.canonical == canonical)
    {
        let original = (variant.receipt_id, variant.position);
        for slot in group {
            planned[*slot].outcome = "replayed";
            planned[*slot].reason = "none";
            planned[*slot].draft = None;
            planned[*slot].original = Some(original);
            planned[*slot].dispute_observation = None;
        }
        return Ok(());
    }
    if !stored.is_empty() {
        let dispute = stored.iter().find_map(|variant| variant.observation);
        let original = stored
            .first()
            .map(|variant| (variant.receipt_id, variant.position));
        for slot in group {
            mark_conflict(&mut planned[*slot], original, dispute);
        }
        return Ok(());
    }
    let first = group[0];
    let original_position = i64::try_from(first).map_err(|_| StoreError::OutOfStorageRange)?;
    for slot in group.iter().skip(1) {
        planned[*slot].outcome = "replayed";
        planned[*slot].reason = "none";
        planned[*slot].draft = None;
        planned[*slot].original = Some((-1, original_position));
        planned[*slot].dispute_observation = None;
    }
    Ok(())
}

fn mark_conflict(
    row: &mut Planned,
    original: Option<(i64, i64)>,
    dispute: Option<(i64, i64, i64)>,
) {
    row.outcome = "quarantined";
    row.reason = "identity-conflict";
    row.draft = None;
    row.original = original;
    row.dispute_observation = dispute;
}

struct Variant {
    canonical: Vec<u8>,
    receipt_id: i64,
    position: i64,
    observation: Option<(i64, i64, i64)>,
}

fn stored_variants(
    tx: &Transaction<'_>,
    producer_id: &str,
    run_id: &str,
    finding_id: &str,
) -> Result<Vec<Variant>, StoreError> {
    let mut statement = tx
        .prepare(
            "SELECT canonical_finding, receipt_id, position, source_id, offer_id, observation_id
             FROM research_findings
             WHERE producer_id = ?1 AND run_id = ?2 AND finding_id = ?3
               AND canonical_finding IS NOT NULL
             ORDER BY receipt_id, position",
        )
        .map_err(map_sqlite)?;
    let rows = statement
        .query_map(params![producer_id, run_id, finding_id], |row| {
            let canonical: Vec<u8> = row.get(0)?;
            let receipt_id: i64 = row.get(1)?;
            let position: i64 = row.get(2)?;
            let source_id: Option<i64> = row.get(3)?;
            let offer_id: Option<i64> = row.get(4)?;
            let observation_id: Option<i64> = row.get(5)?;
            let observation = match (source_id, offer_id, observation_id) {
                (Some(source), Some(offer), Some(observation)) => {
                    Some((source, offer, observation))
                }
                _ => None,
            };
            Ok(Variant {
                canonical,
                receipt_id,
                position,
                observation,
            })
        })
        .map_err(map_sqlite)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(map_sqlite)?;
    Ok(rows)
}

fn count_drafts(planned: &[Planned]) -> u64 {
    planned.iter().filter(|row| row.draft.is_some()).count() as u64
}

fn report_from_planned(planned: &[Planned], projected: u64) -> ImportReport {
    let admitted = count_outcome(planned, "admitted");
    let replayed = count_outcome(planned, "replayed");
    let rejected = count_outcome(planned, "rejected");
    let quarantined = count_outcome(planned, "quarantined");
    let conflicts = planned
        .iter()
        .filter(|row| matches!(row.reason, "identity-conflict" | "run-context-conflict"))
        .count() as u64;
    ImportReport {
        run: run_name(admitted, replayed, rejected, quarantined),
        seen: Some(planned.len() as u64),
        admitted,
        replayed,
        rejected,
        quarantined,
        conflicts,
        projected,
    }
}

fn count_outcome(planned: &[Planned], outcome: &str) -> u64 {
    planned.iter().filter(|row| row.outcome == outcome).count() as u64
}

fn run_name(admitted: u64, replayed: u64, rejected: u64, quarantined: u64) -> &'static str {
    if rejected + quarantined == 0 {
        "complete"
    } else if admitted + replayed > 0 {
        "partial"
    } else {
        "quarantined"
    }
}

fn knowledge_for(row: &Planned) -> Option<&'static str> {
    if row.outcome == "admitted" {
        Some("unreviewed")
    } else {
        None
    }
}

#[allow(clippy::too_many_arguments)]
fn insert_finding(
    tx: &Transaction<'_>,
    receipt_id: i64,
    position: i64,
    producer_id: &str,
    run_id: &str,
    row: &Planned,
    observation: Option<(i64, i64, i64)>,
    knowledge: Option<&str>,
) -> Result<(), StoreError> {
    let (original_receipt, original_position) = match row.original {
        Some((-1, position)) => (Some(receipt_id), Some(position)),
        Some((receipt, position)) => (Some(receipt), Some(position)),
        None => (None, None),
    };
    let (source_ref, listing_ref, listing_generation) = match &row.tuple {
        Some(tuple) => (
            Some(tuple.source_ref.as_str()),
            Some(tuple.listing_ref.as_str()),
            Some(tuple.listing_generation.as_str()),
        ),
        None => (None, None, None),
    };
    let (source_id, offer_id, observation_id) = match observation {
        Some((source, offer, observation)) => (Some(source), Some(offer), Some(observation)),
        None => (None, None, None),
    };
    tx.execute(
        "INSERT INTO research_findings (
            receipt_id, position, producer_id, run_id, finding_id, canonical_finding,
            outcome, reason, knowledge, original_receipt_id, original_position,
            source_ref, listing_ref, listing_generation, source_id, offer_id, observation_id
         ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17
         )",
        params![
            receipt_id,
            position,
            producer_id,
            run_id,
            row.finding_id,
            row.canonical,
            row.outcome,
            row.reason,
            knowledge,
            original_receipt,
            original_position,
            source_ref,
            listing_ref,
            listing_generation,
            source_id,
            offer_id,
            observation_id,
        ],
    )
    .map_err(map_sqlite)?;
    Ok(())
}

fn project_candidate(
    tx: &Transaction<'_>,
    producer_id: &str,
    tuple: &OfferTuple,
    draft: CandidateDraft,
    receipt_id: i64,
    position: i64,
    ingested_at: SystemTime,
) -> Result<(i64, i64, i64), StoreError> {
    let (source_id, offer_id) = match locate_offer(
        tx,
        producer_id,
        &tuple.source_ref,
        &tuple.listing_ref,
        &tuple.listing_generation,
    )? {
        Some(ids) => ids,
        None => allocate_offer(tx)?,
    };
    let observation_id = next_observation_id(tx, source_id, offer_id)?;
    let source = SourceId::new(u64_id(source_id)?).map_err(StoreError::Domain)?;
    let offer = OfferId::new(u64_id(offer_id)?).map_err(StoreError::Domain)?;
    let observation = ObservationId::new(u64_id(observation_id)?).map_err(StoreError::Domain)?;
    let record = store::ObservationRecord::try_new(
        Observation::new(
            ObservationRef::new(OfferRef::new(source, offer), observation),
            draft.subject,
            draft.language,
            draft.observed_at,
            draft.access,
            draft.listing,
            draft.availability,
            draft.source_statement,
            Some(draft.evidence),
        ),
        ingested_at,
        draft.captured_at,
        draft.requested_url,
        draft.resolved_url,
        draft.redirect_reference,
        format!("research:{receipt_id}/{position}"),
        draft.uncertainty_rationale,
    )?;
    let review = ReviewDraft::try_new(
        KnowledgeAssessment::Unreviewed,
        None,
        ASSESSOR,
        RULE_VERSION,
        ingested_at,
        ingested_at,
    )?;
    write_observation(tx, &record, Some(&review))?;
    Ok((source_id, offer_id, observation_id))
}

fn allocate_offer(tx: &Transaction<'_>) -> Result<(i64, i64), StoreError> {
    let max_source: i64 = tx
        .query_row("SELECT COALESCE(MAX(id), 0) FROM sources", [], |row| {
            row.get(0)
        })
        .map_err(map_sqlite)?;
    let source_id = max_source
        .checked_add(1)
        .filter(|id| *id > 0)
        .ok_or(StoreError::OutOfStorageRange)?;
    Ok((source_id, 1))
}

fn next_observation_id(
    tx: &Transaction<'_>,
    source_id: i64,
    offer_id: i64,
) -> Result<i64, StoreError> {
    let current: i64 = tx
        .query_row(
            "SELECT COALESCE(MAX(id), 0) FROM observations WHERE source_id = ?1 AND offer_id = ?2",
            params![source_id, offer_id],
            |row| row.get(0),
        )
        .map_err(map_sqlite)?;
    current
        .checked_add(1)
        .filter(|id| *id > 0)
        .ok_or(StoreError::OutOfStorageRange)
}

fn locate_offer(
    tx: &Transaction<'_>,
    producer_id: &str,
    source_ref: &str,
    listing_ref: &str,
    listing_generation: &str,
) -> Result<Option<(i64, i64)>, StoreError> {
    tx.query_row(
        "SELECT source_id, offer_id FROM research_findings
         WHERE producer_id = ?1 AND source_ref = ?2 AND listing_ref = ?3
           AND listing_generation = ?4 AND source_id IS NOT NULL AND offer_id IS NOT NULL
         LIMIT 1",
        params![producer_id, source_ref, listing_ref, listing_generation],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .optional()
    .map_err(map_sqlite)
}

fn dispute_run(
    tx: &Transaction<'_>,
    producer_id: &str,
    run_id: &str,
    ingested_at: SystemTime,
    reason: &str,
) -> Result<(), StoreError> {
    let mut statement = tx
        .prepare(
            "SELECT DISTINCT source_id, offer_id, observation_id FROM research_findings
             WHERE producer_id = ?1 AND run_id = ?2 AND observation_id IS NOT NULL",
        )
        .map_err(map_sqlite)?;
    let rows = statement
        .query_map(params![producer_id, run_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .map_err(map_sqlite)?
        .collect::<Result<Vec<(i64, i64, i64)>, _>>()
        .map_err(map_sqlite)?;
    drop(statement);
    for (source, offer, observation) in rows {
        dispute_one(tx, source, offer, observation, ingested_at, reason)?;
    }
    Ok(())
}

fn dispute_equal_time(
    tx: &Transaction<'_>,
    source_id: i64,
    offer_id: i64,
    ingested_at: SystemTime,
) -> Result<(), StoreError> {
    let source = SourceId::new(u64_id(source_id)?).map_err(StoreError::Domain)?;
    let offer = OfferId::new(u64_id(offer_id)?).map_err(StoreError::Domain)?;
    let entries = load_history(tx, OfferRef::new(source, offer))?;
    for (left_index, left) in entries.iter().enumerate() {
        for right in entries.iter().skip(left_index + 1) {
            let same_instant = left.record().observation().observed_at().instant()
                == right.record().observation().observed_at().instant()
                && left
                    .record()
                    .observation()
                    .observed_at()
                    .instant()
                    .is_some();
            if same_instant
                && history::observations_incompatible(
                    left.record().observation(),
                    right.record().observation(),
                )
            {
                dispute_one(
                    tx,
                    source_id,
                    offer_id,
                    i64_id(left.record().observation().reference().observation().get())?,
                    ingested_at,
                    "equal-time-contradiction",
                )?;
                dispute_one(
                    tx,
                    source_id,
                    offer_id,
                    i64_id(right.record().observation().reference().observation().get())?,
                    ingested_at,
                    "equal-time-contradiction",
                )?;
            }
        }
    }
    Ok(())
}

fn dispute_one(
    tx: &Transaction<'_>,
    source_id: i64,
    offer_id: i64,
    observation_id: i64,
    ingested_at: SystemTime,
    reason: &str,
) -> Result<(), StoreError> {
    let current: Option<(i64, String)> = tx
        .query_row(
            "SELECT revision, knowledge FROM reviews
             WHERE source_id = ?1 AND offer_id = ?2 AND observation_id = ?3
             ORDER BY revision DESC LIMIT 1",
            params![source_id, offer_id, observation_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(map_sqlite)?;
    let Some((revision, knowledge)) = current else {
        return Ok(());
    };
    if knowledge == "disputed" {
        return Ok(());
    }
    let source = SourceId::new(u64_id(source_id)?).map_err(StoreError::Domain)?;
    let offer = OfferId::new(u64_id(offer_id)?).map_err(StoreError::Domain)?;
    let observation = ObservationId::new(u64_id(observation_id)?).map_err(StoreError::Domain)?;
    let draft = ReviewDraft::try_new(
        KnowledgeAssessment::Disputed,
        None,
        ASSESSOR,
        reason,
        ingested_at,
        ingested_at,
    )?;
    let expected = u64::try_from(revision).map_err(|_| StoreError::OutOfStorageRange)?;
    write_review(
        tx,
        ObservationRef::new(OfferRef::new(source, offer), observation),
        expected,
        &draft,
    )?;
    Ok(())
}

fn run_context_conflicts(
    tx: &Transaction<'_>,
    producer_id: &str,
    run_id: &str,
    context_canonical: &[u8],
) -> Result<bool, StoreError> {
    let stored: Option<Vec<u8>> = tx
        .query_row(
            "SELECT canonical_context FROM research_runs WHERE producer_id = ?1 AND run_id = ?2",
            params![producer_id, run_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(map_sqlite)?;
    Ok(stored.is_some_and(|stored| stored != context_canonical))
}

fn ensure_run(
    tx: &Transaction<'_>,
    producer_id: &str,
    run_id: &str,
    context_canonical: &[u8],
    receipt_id: i64,
) -> Result<(), StoreError> {
    tx.execute(
        "INSERT INTO research_runs (producer_id, run_id, canonical_context, first_receipt_id)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(producer_id, run_id) DO NOTHING",
        params![producer_id, run_id, context_canonical, receipt_id],
    )
    .map_err(map_sqlite)?;
    Ok(())
}

fn ensure_ingestion_advances(
    tx: &Transaction<'_>,
    seconds: i64,
    nanos: i64,
) -> Result<(), StoreError> {
    let previous: Option<(i64, i64)> = tx
        .query_row(
            "SELECT ingested_s, ingested_ns FROM research_inputs
             ORDER BY ingested_s DESC, ingested_ns DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(map_sqlite)?;
    if previous.is_some_and(|previous| (seconds, nanos) < previous) {
        return Err(StoreError::InvalidReview {
            reason: "ingestion-time-regressed",
        });
    }
    Ok(())
}

fn find_canonical_receipt(
    tx: &Transaction<'_>,
    canonical: &[u8],
) -> Result<Option<i64>, StoreError> {
    tx.query_row(
        "SELECT id FROM research_inputs WHERE canonical_bytes = ?1 LIMIT 1",
        params![canonical],
        |row| row.get(0),
    )
    .optional()
    .map_err(map_sqlite)
}

fn opaque_already_stored(
    tx: &Transaction<'_>,
    raw: &[u8],
    canonical: Option<&[u8]>,
) -> Result<bool, StoreError> {
    let found = if let Some(canonical) = canonical {
        find_canonical_receipt(tx, canonical)?
    } else {
        tx.query_row(
            "SELECT id FROM research_inputs
             WHERE canonical_bytes IS NULL AND raw_bytes = ?1 LIMIT 1",
            params![raw],
            |row| row.get(0),
        )
        .optional()
        .map_err(map_sqlite)?
    };
    Ok(found.is_some())
}

fn stored_replay_report(tx: &Transaction<'_>, receipt_id: i64) -> Result<ImportReport, StoreError> {
    let mut statement = tx
        .prepare(
            "SELECT outcome, reason FROM research_findings
             WHERE receipt_id = ?1 ORDER BY position",
        )
        .map_err(map_sqlite)?;
    let rows = statement
        .query_map(params![receipt_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(map_sqlite)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(map_sqlite)?;
    let mut replayed = 0;
    let mut rejected = 0;
    let mut quarantined = 0;
    let mut conflicts = 0;
    for (outcome, reason) in &rows {
        match outcome.as_str() {
            "admitted" => replayed += 1,
            "replayed" => replayed += 1,
            "rejected" => rejected += 1,
            "quarantined" => quarantined += 1,
            _ => {
                return Err(StoreError::UnsupportedFormat {
                    reason: "finding-outcome",
                });
            }
        }
        if matches!(
            reason.as_str(),
            "identity-conflict" | "run-context-conflict"
        ) {
            conflicts += 1;
        }
    }
    Ok(ImportReport {
        run: run_name(0, replayed, rejected, quarantined),
        seen: Some(rows.len() as u64),
        admitted: 0,
        replayed,
        rejected,
        quarantined,
        conflicts,
        projected: 0,
    })
}

fn opaque_report() -> ImportReport {
    ImportReport {
        run: "quarantined",
        seen: None,
        admitted: 0,
        replayed: 0,
        rejected: 0,
        quarantined: 0,
        conflicts: 0,
        projected: 0,
    }
}

#[allow(clippy::too_many_arguments)]
fn insert_receipt(
    tx: &Transaction<'_>,
    raw: &[u8],
    canonical: Option<&[u8]>,
    ingested_s: i64,
    ingested_ns: i64,
    disposition: &str,
    reported_outcome: Option<&str>,
    issues: &str,
) -> Result<i64, StoreError> {
    let max_id: i64 = tx
        .query_row(
            "SELECT COALESCE(MAX(id), 0) FROM research_inputs",
            [],
            |row| row.get(0),
        )
        .map_err(map_sqlite)?;
    let id = max_id
        .checked_add(1)
        .filter(|id| *id > 0)
        .ok_or(StoreError::OutOfStorageRange)?;
    tx.execute(
        "INSERT INTO research_inputs (
            id, raw_bytes, canonical_bytes, ingested_s, ingested_ns, disposition,
            reported_outcome, rule_version, issues
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            id,
            raw,
            canonical,
            ingested_s,
            ingested_ns,
            disposition,
            reported_outcome,
            RULE_VERSION,
            issues
        ],
    )
    .map_err(map_sqlite)?;
    Ok(id)
}

fn read_snapshot(tx: &Transaction<'_>) -> Result<ResearchSnapshot, StoreError> {
    let envelopes = count(tx, "SELECT COUNT(*) FROM research_inputs")?;
    let runs = count(tx, "SELECT COUNT(*) FROM research_runs")?;
    let findings = count(tx, "SELECT COUNT(*) FROM research_findings")?;
    let observations = count(tx, "SELECT COUNT(*) FROM observations")?;
    let reviews = count(tx, "SELECT COUNT(*) FROM reviews")?;
    let editions = count(tx, "SELECT COUNT(*) FROM editions")?;
    let mut inputs_statement = tx
        .prepare(
            "SELECT id, raw_bytes, canonical_bytes, disposition, reported_outcome
             FROM research_inputs ORDER BY id",
        )
        .map_err(map_sqlite)?;
    let inputs = inputs_statement
        .query_map([], |row| {
            Ok(InputView {
                receipt_id: u64_id(row.get(0)?).map_err(store_row)?,
                raw_bytes: row.get(1)?,
                canonical_bytes: row.get(2)?,
                disposition: row.get(3)?,
                reported_outcome: row.get(4)?,
            })
        })
        .map_err(map_sqlite)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(map_sqlite)?;
    let mut findings_statement = tx
        .prepare(
            "SELECT f.receipt_id, f.position, f.outcome, f.reason, f.canonical_finding,
                    f.observation_id,
                    CASE
                        WHEN f.observation_id IS NULL THEN f.knowledge
                        ELSE (
                            SELECT r.knowledge FROM reviews r
                            WHERE r.source_id = f.source_id AND r.offer_id = f.offer_id
                              AND r.observation_id = f.observation_id
                            ORDER BY r.revision DESC LIMIT 1
                        )
                    END
             FROM research_findings f
             ORDER BY f.receipt_id, f.position",
        )
        .map_err(map_sqlite)?;
    let findings_rows = findings_statement
        .query_map([], |row| {
            let observation: Option<i64> = row.get(5)?;
            Ok(FindingView {
                receipt_id: u64_id(row.get(0)?).map_err(store_row)?,
                position: u64_id(row.get(1)?).map_err(store_row)?,
                outcome: row.get(2)?,
                reason: row.get(3)?,
                canonical_finding: row.get(4)?,
                observation_id: observation.map(u64_id).transpose().map_err(store_row)?,
                knowledge: row.get(6)?,
            })
        })
        .map_err(map_sqlite)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(map_sqlite)?;
    Ok(ResearchSnapshot {
        envelopes,
        runs,
        findings,
        observations,
        reviews,
        editions,
        inputs,
        findings_rows,
    })
}

fn count(tx: &Transaction<'_>, sql: &str) -> Result<u64, StoreError> {
    let value: i64 = tx
        .query_row(sql, [], |row| row.get(0))
        .map_err(map_sqlite)?;
    u64_id(value)
}

fn decode_candidate(row: &rusqlite::Row<'_>) -> rusqlite::Result<CandidateRecord> {
    let producer_id: String = row.get(0)?;
    let run_id: String = row.get(1)?;
    let finding_id: String = row.get(2)?;
    let outcome: String = row.get(3)?;
    let reason: String = row.get(4)?;
    let canonical_finding: Option<Vec<u8>> = row.get(5)?;
    let source_ref: Option<String> = row.get(6)?;
    let listing_ref: Option<String> = row.get(7)?;
    let listing_generation: Option<String> = row.get(8)?;
    let ledger_source_id: Option<i64> = row.get(9)?;
    let ledger_offer_id: Option<i64> = row.get(10)?;
    let ledger_observation_id: Option<i64> = row.get(11)?;
    let ledger_knowledge: Option<String> = row.get(12)?;
    Ok(CandidateRecord {
        producer_id,
        run_id,
        finding_id,
        receipt_id: 0,
        position: 0,
        outcome,
        reason,
        ledger_knowledge,
        canonical_finding,
        source_ref,
        listing_ref,
        listing_generation,
        ledger_source_id: optional_stored_id(ledger_source_id).map_err(store_row)?,
        ledger_offer_id: optional_stored_id(ledger_offer_id).map_err(store_row)?,
        ledger_observation_id: optional_stored_id(ledger_observation_id).map_err(store_row)?,
    })
}

fn optional_stored_id(value: Option<i64>) -> Result<Option<u64>, StoreError> {
    match value {
        None => Ok(None),
        Some(value) => {
            let id = u64_id(value)?;
            if id == 0 {
                Err(StoreError::UnsupportedFormat {
                    reason: "stored-id",
                })
            } else {
                Ok(Some(id))
            }
        }
    }
}

fn u64_id(value: i64) -> Result<u64, StoreError> {
    u64::try_from(value).map_err(|_| StoreError::UnsupportedFormat {
        reason: "stored-id",
    })
}

fn i64_id(value: u64) -> Result<i64, StoreError> {
    i64::try_from(value).map_err(|_| StoreError::OutOfStorageRange)
}

fn store_row(error: StoreError) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(error))
}

fn conflict_token(state: ConflictState) -> &'static str {
    match state {
        ConflictState::Clear => "clear",
        ConflictState::Unresolved => "unresolved",
        ConflictState::Unknown => "unknown",
    }
}

fn recheck_token(state: RecheckState) -> &'static str {
    match state {
        RecheckState::Clear => "clear",
        RecheckState::Failed => "failed",
        RecheckState::Unknown => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Temp {
        dir: std::path::PathBuf,
    }

    impl Temp {
        fn new() -> Self {
            static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "bookradar-research-unit-{}-{n}",
                std::process::id()
            ));
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

    fn at(seconds: i64) -> SystemTime {
        SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(seconds as u64)
    }

    #[test]
    fn append_only_enforcement_rejects_research_and_observation_mutation() {
        let temp = Temp::new();
        let path = temp.path("ledger.sqlite");
        let mut store = ResearchStore::create(&path).expect("create");
        let bytes = include_bytes!("../tests/fixtures/research/partial.json");
        store
            .import(bytes, ImportContext::new(at(1_793_545_201)))
            .expect("import");
        let before = store.inspect().expect("inspect");
        let update = store.conn.execute(
            "UPDATE research_findings SET reason = 'rewritten' WHERE position = 0",
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
        let after = store.inspect().expect("inspect");
        assert_eq!(after.findings, before.findings);
        assert_eq!(after.observations, before.observations);
    }

    #[test]
    fn unsupported_ledger_version_fails_closed() {
        let temp = Temp::new();
        let path = temp.path("ledger.sqlite");
        let store = ResearchStore::create(&path).expect("create");
        store
            .conn
            .execute("PRAGMA user_version = 2", [])
            .expect("version");
        drop(store);
        assert!(matches!(
            ResearchStore::open(&path, OpenMode::ReadOnly),
            Err(StoreError::UnsupportedFormat {
                reason: "user-version"
            })
        ));
    }

    #[test]
    fn busy_store_rejects_import() {
        let temp = Temp::new();
        let path = temp.path("ledger.sqlite");
        let holder = ResearchStore::create(&path).expect("create");
        holder.conn.execute_batch("BEGIN IMMEDIATE").expect("lock");
        let mut other = ResearchStore::open(&path, OpenMode::ReadWrite).expect("open");
        let error = other
            .import(b"{}", ImportContext::new(at(10)))
            .expect_err("busy");
        assert!(matches!(error, ResearchError::Store(StoreError::Busy)));
        drop(holder);
        drop(other);
        let mut reopened = ResearchStore::open(&path, OpenMode::ReadOnly).expect("reopen");
        assert_eq!(reopened.inspect().expect("inspect").envelopes, 0);
    }
}
