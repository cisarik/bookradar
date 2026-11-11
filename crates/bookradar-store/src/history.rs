//! Deterministic history ordering and current-candidate selection.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::time::SystemTime;

use bookradar_core::{
    AccessResult, ConflictState, EditionId, Eligibility, KnowledgeAssessment, LanguageEvidence,
    ListingDisposition, Observation, ObservationId, OffsetState, RecheckState, SourceAvailability,
    SubjectIdentity, TemporalAssessment, TimestampAssertion, VerificationAssessment,
    VerificationContext, evaluate_verification,
};

use crate::store::{
    self, ObservationRecord, ReviewRevision, StoreError, access_token, availability_token,
    knowledge_token, listing_token,
};

/// One stored observation and every review revision, oldest revision first.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryEntry {
    record: ObservationRecord,
    reviews: Vec<ReviewRevision>,
}

impl HistoryEntry {
    pub(crate) fn bare(record: ObservationRecord) -> Self {
        Self {
            record,
            reviews: Vec::new(),
        }
    }

    pub(crate) fn push_review(&mut self, review: ReviewRevision) {
        self.reviews.push(review);
    }

    pub fn record(&self) -> &ObservationRecord {
        &self.record
    }

    pub fn reviews(&self) -> &[ReviewRevision] {
        &self.reviews
    }
}

/// One kernel evaluation of a current candidate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateAssessment {
    record: ObservationRecord,
    context: VerificationContext,
    assessment: VerificationAssessment,
}

impl CandidateAssessment {
    pub fn record(&self) -> &ObservationRecord {
        &self.record
    }

    pub fn context(&self) -> &VerificationContext {
        &self.context
    }

    pub fn assessment(&self) -> VerificationAssessment {
        self.assessment
    }
}

/// A supported historical stock claim that can label a negative result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StockClaim {
    observation: ObservationId,
    availability: SourceAvailability,
    observed_at: SystemTime,
}

impl StockClaim {
    pub fn observation(&self) -> ObservationId {
        self.observation
    }

    pub fn availability(&self) -> SourceAvailability {
        self.availability
    }

    pub fn observed_at(&self) -> SystemTime {
        self.observed_at
    }
}

/// The current reading of one offer at one injected instant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CurrentView {
    observation_count: usize,
    last_ordered_attempts: Vec<ObservationRecord>,
    unorderable: Vec<ObservationRecord>,
    candidates: Vec<CandidateAssessment>,
    historical_claims: Vec<StockClaim>,
    conflict: ConflictState,
    recheck: RecheckState,
}

impl CurrentView {
    fn empty() -> Self {
        Self {
            observation_count: 0,
            last_ordered_attempts: Vec::new(),
            unorderable: Vec::new(),
            candidates: Vec::new(),
            historical_claims: Vec::new(),
            conflict: ConflictState::Clear,
            recheck: RecheckState::Clear,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.observation_count == 0
    }

    pub fn last_ordered_attempts(&self) -> &[ObservationRecord] {
        &self.last_ordered_attempts
    }

    pub fn unorderable(&self) -> &[ObservationRecord] {
        &self.unorderable
    }

    pub fn candidates(&self) -> &[CandidateAssessment] {
        &self.candidates
    }

    pub fn historical_claims(&self) -> &[StockClaim] {
        &self.historical_claims
    }

    pub fn conflict(&self) -> ConflictState {
        self.conflict
    }

    pub fn recheck(&self) -> RecheckState {
        self.recheck
    }
}

pub(crate) fn sort_entries(entries: &mut [HistoryEntry]) {
    entries.sort_by(history_order);
}

pub(crate) fn resolve(
    entries: &[HistoryEntry],
    target: EditionId,
    now: SystemTime,
) -> Result<CurrentView, StoreError> {
    if entries.is_empty() {
        return Ok(CurrentView::empty());
    }
    let (ordered, unorderable) = partition(entries);
    let (latest, candidate_entries) = select_sets(&ordered);
    let definite_conflict = has_pairwise(&ordered) || has_intrinsic_or_disputed(entries);
    let definite_failure = failed_recheck(&ordered, &candidate_entries);
    let (conflict, recheck) =
        context_states(definite_conflict, definite_failure, !unorderable.is_empty());
    let mut candidates = Vec::with_capacity(candidate_entries.len());
    for entry in candidate_entries {
        candidates.push(evaluate_candidate(entry, target, conflict, recheck, now)?);
    }
    candidates.sort_by_key(|candidate| observation_id_of(candidate.record()));
    let mut historical_claims = Vec::new();
    for entry in entries {
        if let Some(claim) = stock_claim(entry, target, now) {
            historical_claims.push(claim);
        }
    }
    historical_claims.sort_by(|left, right| {
        left.observed_at
            .cmp(&right.observed_at)
            .then(left.observation.get().cmp(&right.observation.get()))
    });
    Ok(CurrentView {
        observation_count: entries.len(),
        last_ordered_attempts: records_sorted(latest),
        unorderable: records_sorted(unorderable),
        candidates,
        historical_claims,
        conflict,
        recheck,
    })
}

/// Renders the synthetic CLI `current` transcript for one view.
pub fn render_current(
    view: &CurrentView,
    offer: bookradar_core::OfferRef,
    target: EditionId,
    now: SystemTime,
) -> String {
    if view.is_empty() {
        return format!(
            "synthetic=true offer={}/{} target={} status=empty",
            offer.source().get(),
            offer.offer().get(),
            target.get()
        );
    }
    let mut lines = machine_lines(view);
    lines.extend(human_lines(view, target, now));
    lines.join("\n")
}

/// Renders the synthetic CLI `history` transcript.
pub fn render_history(offer: bookradar_core::OfferRef, entries: &[HistoryEntry]) -> String {
    let mut lines = Vec::with_capacity(entries.len() + 1);
    lines.push(format!(
        "synthetic=true offer={}/{} count={}",
        offer.source().get(),
        offer.offer().get(),
        entries.len()
    ));
    for entry in entries {
        lines.push(render_history_row(entry));
    }
    lines.join("\n")
}

fn history_order(left: &HistoryEntry, right: &HistoryEntry) -> Ordering {
    match (observed_instant(left), observed_instant(right)) {
        (Some(left_time), Some(right_time)) => left_time
            .cmp(&right_time)
            .then(entry_id(left).cmp(&entry_id(right))),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => entry_id(left).cmp(&entry_id(right)),
    }
}

fn partition(entries: &[HistoryEntry]) -> (Vec<&HistoryEntry>, Vec<&HistoryEntry>) {
    let mut ordered = Vec::new();
    let mut unorderable = Vec::new();
    for entry in entries {
        if observed_instant(entry).is_some() {
            ordered.push(entry);
        } else {
            unorderable.push(entry);
        }
    }
    ordered.sort_by(|left, right| history_order(left, right));
    unorderable.sort_by_key(|entry| entry_id(entry));
    (ordered, unorderable)
}

fn select_sets<'a>(ordered: &[&'a HistoryEntry]) -> (Vec<&'a HistoryEntry>, Vec<&'a HistoryEntry>) {
    let Some(latest_time) = max_instant(ordered) else {
        return (Vec::new(), Vec::new());
    };
    let latest = at_instant(ordered, latest_time);
    let readable: Vec<&HistoryEntry> = ordered
        .iter()
        .copied()
        .filter(|entry| entry.record().observation().access() == AccessResult::Readable)
        .collect();
    let candidates = match max_instant(&readable) {
        Some(time) => readable
            .into_iter()
            .filter(|entry| observed_instant(entry) == Some(time))
            .collect(),
        None => latest.clone(),
    };
    (latest, candidates)
}

fn context_states(
    definite_conflict: bool,
    definite_failure: bool,
    unorderable: bool,
) -> (ConflictState, RecheckState) {
    let conflict = if definite_conflict {
        ConflictState::Unresolved
    } else if unorderable {
        ConflictState::Unknown
    } else {
        ConflictState::Clear
    };
    let recheck = if definite_failure {
        RecheckState::Failed
    } else if unorderable {
        RecheckState::Unknown
    } else {
        RecheckState::Clear
    };
    (conflict, recheck)
}

fn has_pairwise(ordered: &[&HistoryEntry]) -> bool {
    let mut groups: BTreeMap<SystemTime, Vec<&HistoryEntry>> = BTreeMap::new();
    for entry in ordered {
        if let Some(instant) = observed_instant(entry) {
            groups.entry(instant).or_default().push(*entry);
        }
    }
    groups.values().any(|group| group_conflicts(group))
}

fn group_conflicts(group: &[&HistoryEntry]) -> bool {
    group.iter().enumerate().any(|(index, left)| {
        group[index + 1..].iter().any(|right| {
            !is_rejected(left)
                && !is_rejected(right)
                && incompatible(left.record().observation(), right.record().observation())
        })
    })
}

fn incompatible(left: &Observation, right: &Observation) -> bool {
    editions_conflict(left, right)
        || languages_conflict(left, right)
        || availability_conflict(left, right)
        || listings_conflict(left, right)
}

fn editions_conflict(left: &Observation, right: &Observation) -> bool {
    match (resolved_edition(left), resolved_edition(right)) {
        (Some(left_edition), Some(right_edition)) => left_edition != right_edition,
        _ => false,
    }
}

fn languages_conflict(left: &Observation, right: &Observation) -> bool {
    match (
        explicit_language(left.language()),
        explicit_language(right.language()),
    ) {
        (Some(left_language), Some(right_language)) => left_language != right_language,
        _ => false,
    }
}

fn availability_conflict(left: &Observation, right: &Observation) -> bool {
    match (
        explicit_availability(left.availability()),
        explicit_availability(right.availability()),
    ) {
        (Some(left_availability), Some(right_availability)) => {
            left_availability != right_availability
        }
        _ => false,
    }
}

fn listings_conflict(left: &Observation, right: &Observation) -> bool {
    match (
        definite_listing(left.listing()),
        definite_listing(right.listing()),
    ) {
        (Some(left_listing), Some(right_listing)) => left_listing != right_listing,
        _ => false,
    }
}

fn has_intrinsic_or_disputed(entries: &[HistoryEntry]) -> bool {
    entries.iter().any(|entry| {
        (!is_rejected(entry) && intrinsic_conflict(entry.record().observation()))
            || latest_knowledge(entry).0 == KnowledgeAssessment::Disputed
    })
}

fn failed_recheck(ordered: &[&HistoryEntry], candidates: &[&HistoryEntry]) -> bool {
    let Some(candidate_time) = candidates.first().and_then(|entry| observed_instant(entry)) else {
        return false;
    };
    ordered.iter().any(|entry| {
        entry.record().observation().access() != AccessResult::Readable
            && observed_instant(entry).is_some_and(|instant| instant >= candidate_time)
    })
}

fn evaluate_candidate(
    entry: &HistoryEntry,
    target: EditionId,
    conflict: ConflictState,
    recheck: RecheckState,
    now: SystemTime,
) -> Result<CandidateAssessment, StoreError> {
    let (knowledge, approval) = latest_knowledge(entry);
    let context = VerificationContext::new(
        entry.record().observation().reference(),
        knowledge,
        approval,
        conflict,
        recheck,
    );
    let assessment = evaluate_verification(target, entry.record().observation(), &context, now)
        .map_err(StoreError::Domain)?;
    Ok(CandidateAssessment {
        record: entry.record().clone(),
        context,
        assessment,
    })
}

fn stock_claim(entry: &HistoryEntry, target: EditionId, now: SystemTime) -> Option<StockClaim> {
    let observation = entry.record().observation();
    if observation.access() != AccessResult::Readable {
        return None;
    }
    if observation.source_statement().is_none()
        || latest_knowledge(entry).0 != KnowledgeAssessment::Supported
    {
        return None;
    }
    if resolved_edition(observation) != Some(target)
        || observation.language() != LanguageEvidence::Slovak
    {
        return None;
    }
    let availability = explicit_availability(observation.availability())?;
    let observed_at = usable_instant(observation.observed_at(), now)?;
    Some(StockClaim {
        observation: observation.reference().observation(),
        availability,
        observed_at,
    })
}

fn usable_instant(assertion: &TimestampAssertion, now: SystemTime) -> Option<SystemTime> {
    if !matches!(assertion.offset(), OffsetState::Known { .. }) {
        return None;
    }
    let instant = assertion.instant()?;
    if instant > now {
        return None;
    }
    Some(instant)
}

fn machine_lines(view: &CurrentView) -> Vec<String> {
    if view.unorderable.is_empty()
        && view.last_ordered_attempts.len() == 1
        && view.candidates.len() == 1
    {
        return singleton_lines(view);
    }
    expanded_lines(view)
}

fn singleton_lines(view: &CurrentView) -> Vec<String> {
    let latest = &view.last_ordered_attempts[0];
    let candidate = &view.candidates[0];
    let observation = latest.observation();
    vec![
        format!(
            "synthetic=true latest_attempt={} access={} listing={} availability={}",
            observation.reference().observation().get(),
            access_token(observation.access()),
            listing_token(observation.listing()),
            availability_token(observation.availability())
        ),
        format!(
            "candidate={} temporal={} conflict={} recheck={} verification={}",
            observation_id_of(candidate.record()),
            temporal_token(candidate.assessment.temporal()),
            conflict_token(view.conflict),
            recheck_token(view.recheck),
            verification_token(view)
        ),
    ]
}

fn expanded_lines(view: &CurrentView) -> Vec<String> {
    let mut lines = Vec::new();
    if view.unorderable.is_empty() {
        lines.push(format!(
            "synthetic=true latest_attempts={}",
            join_ids(view.last_ordered_attempts.iter().map(observation_id_of))
        ));
        let mut attempts = view.last_ordered_attempts.clone();
        attempts.sort_by_key(observation_id_of);
        for attempt in &attempts {
            lines.push(render_attempt(attempt));
        }
    } else {
        lines.push("synthetic=true latest_attempts=unknown".to_owned());
        lines.push(format!(
            "last_ordered_attempts={}",
            join_ids(view.last_ordered_attempts.iter().map(observation_id_of))
        ));
        lines.push(format!(
            "unorderable={}",
            join_ids(view.unorderable.iter().map(observation_id_of))
        ));
        let mut attempts = view.last_ordered_attempts.clone();
        attempts.extend(view.unorderable.iter().cloned());
        attempts.sort_by_key(observation_id_of);
        for attempt in &attempts {
            lines.push(render_attempt(attempt));
        }
    }
    lines.push(format!(
        "candidates={} conflict={} recheck={} verification={}",
        join_ids(
            view.candidates
                .iter()
                .map(|candidate| observation_id_of(candidate.record()))
        ),
        conflict_token(view.conflict),
        recheck_token(view.recheck),
        verification_token(view)
    ));
    for candidate in &view.candidates {
        lines.push(format!(
            "candidate={} temporal={}",
            observation_id_of(candidate.record()),
            temporal_token(candidate.assessment.temporal())
        ));
    }
    lines
}

fn human_lines(view: &CurrentView, target: EditionId, now: SystemTime) -> Vec<String> {
    if let Some(line) = checked_available(view) {
        return vec![line];
    }
    let kind = principal_kind(view, target, now);
    let mut lines = vec![principal_text(view, target, kind)];
    match kind {
        Principal::Conflict
        | Principal::Unorderable
        | Principal::Future
        | Principal::OffsetUnknown
        | Principal::TimeUnknown => {
            if let Some(line) = dated_access_line(view, now) {
                lines.push(line);
            }
        }
        Principal::Removed | Principal::Recheck => {
            if let Some(claim) = view.historical_claims.last() {
                lines.push(stock_text(claim));
            }
        }
        Principal::Unavailable
        | Principal::Reserved
        | Principal::Available
        | Principal::UnknownAvailability
        | Principal::Insufficient => {}
    }
    lines
}

#[derive(Clone, Copy)]
enum Principal {
    Conflict,
    Unorderable,
    Future,
    OffsetUnknown,
    TimeUnknown,
    Removed,
    Recheck,
    Unavailable,
    Reserved,
    Available,
    UnknownAvailability,
    Insufficient,
}

fn principal_kind(view: &CurrentView, target: EditionId, now: SystemTime) -> Principal {
    if view.conflict == ConflictState::Unresolved {
        return Principal::Conflict;
    }
    if !view.unorderable.is_empty() {
        return Principal::Unorderable;
    }
    if relevant_future(view, now) {
        return Principal::Future;
    }
    if candidates_have_unknown_offset(view) {
        return Principal::OffsetUnknown;
    }
    if candidates_have_unusable_time(view) {
        return Principal::TimeUnknown;
    }
    if latest_is_removed(view) {
        return Principal::Removed;
    }
    if latest_is_access_failure(view) {
        return Principal::Recheck;
    }
    if supported_candidate(view, SourceAvailability::Unavailable, target).is_some() {
        return Principal::Unavailable;
    }
    if supported_candidate(view, SourceAvailability::Reserved, target).is_some() {
        return Principal::Reserved;
    }
    if supported_candidate(view, SourceAvailability::Available, target).is_some() {
        return Principal::Available;
    }
    if view.candidates.first().is_some_and(|candidate| {
        candidate.record().observation().availability() == SourceAvailability::Unknown
    }) {
        return Principal::UnknownAvailability;
    }
    Principal::Insufficient
}

fn principal_text(view: &CurrentView, target: EditionId, kind: Principal) -> String {
    match kind {
        Principal::Conflict => "Unverified -- unresolved conflict".to_owned(),
        Principal::Unorderable => "Unverified -- observation order unknown".to_owned(),
        Principal::Future => "Unverified -- observation time in future".to_owned(),
        Principal::OffsetUnknown => "Unverified -- observation offset unknown".to_owned(),
        Principal::TimeUnknown => "Unverified -- observation time unknown".to_owned(),
        Principal::Removed => format!("Listing removed -- {}", latest_time(view)),
        Principal::Recheck => format!("Could not recheck -- {}", latest_time(view)),
        Principal::Unavailable => {
            stock_candidate_text(view, SourceAvailability::Unavailable, target)
        }
        Principal::Reserved => stock_candidate_text(view, SourceAvailability::Reserved, target),
        Principal::Available => stock_candidate_text(view, SourceAvailability::Available, target),
        Principal::UnknownAvailability => "Unverified -- source availability unknown".to_owned(),
        Principal::Insufficient => "Unverified -- insufficient support".to_owned(),
    }
}

fn checked_available(view: &CurrentView) -> Option<String> {
    let candidate = view
        .candidates
        .iter()
        .filter(|candidate| {
            matches!(
                candidate.assessment.eligibility(),
                Eligibility::Eligible { .. }
            )
        })
        .min_by_key(|candidate| observation_id_of(candidate.record()))?;
    match candidate.assessment.eligibility() {
        Eligibility::Eligible {
            observed_at,
            expires_at,
            ..
        } => Some(format!(
            "Checked available -- {}; expires_unix={}",
            format_unix(observed_at),
            format_bare(expires_at)
        )),
        Eligibility::NotEligible => None,
    }
}

fn relevant_future(view: &CurrentView, now: SystemTime) -> bool {
    view.candidates
        .iter()
        .any(|candidate| instant_is_future(candidate.record(), now))
        || view
            .last_ordered_attempts
            .iter()
            .any(|record| instant_is_future(record, now))
}

fn candidates_have_unknown_offset(view: &CurrentView) -> bool {
    view.candidates.iter().any(|candidate| {
        matches!(
            candidate.record().observation().observed_at().offset(),
            OffsetState::Unknown
        ) && candidate
            .record()
            .observation()
            .observed_at()
            .instant()
            .is_some()
    })
}

fn candidates_have_unusable_time(view: &CurrentView) -> bool {
    view.candidates.iter().any(|candidate| {
        let assertion = candidate.record().observation().observed_at();
        assertion.instant().is_none()
            || (candidate.assessment.temporal() == TemporalAssessment::Unknown
                && matches!(assertion.offset(), OffsetState::Known { .. }))
    })
}

fn latest_is_removed(view: &CurrentView) -> bool {
    view.last_ordered_attempts.iter().any(|record| {
        record.observation().access() == AccessResult::PageGone
            || record.observation().listing() == ListingDisposition::Removed
    })
}

fn latest_is_access_failure(view: &CurrentView) -> bool {
    view.last_ordered_attempts.iter().any(|record| {
        record.observation().access() != AccessResult::Readable
            && record.observation().access() != AccessResult::PageGone
            && record.observation().listing() != ListingDisposition::Removed
    })
}

fn dated_access_line(view: &CurrentView, now: SystemTime) -> Option<String> {
    let mut matches: Vec<&ObservationRecord> = view
        .last_ordered_attempts
        .iter()
        .filter(|record| {
            record.observation().access() != AccessResult::Readable
                && record
                    .observation()
                    .observed_at()
                    .instant()
                    .is_some_and(|instant| instant <= now)
        })
        .collect();
    matches.sort_by_key(|record| {
        (
            record.observation().observed_at().instant(),
            observation_id_of(record),
        )
    });
    let record = matches.last()?;
    let time = format_unix(record.observation().observed_at().instant()?);
    if record.observation().access() == AccessResult::PageGone
        || record.observation().listing() == ListingDisposition::Removed
    {
        Some(format!("Listing removed -- {time}"))
    } else {
        Some(format!("Could not recheck -- {time}"))
    }
}

fn supported_candidate(
    view: &CurrentView,
    availability: SourceAvailability,
    target: EditionId,
) -> Option<&CandidateAssessment> {
    view.candidates
        .iter()
        .find(|candidate| stock_candidate_matches(candidate, availability, target))
}

fn stock_candidate_matches(
    candidate: &CandidateAssessment,
    availability: SourceAvailability,
    target: EditionId,
) -> bool {
    let observation = candidate.record().observation();
    observation.availability() == availability
        && observation.access() == AccessResult::Readable
        && observation.source_statement().is_some()
        && candidate.context().knowledge() == KnowledgeAssessment::Supported
        && matches!(observation.subject(), SubjectIdentity::Resolved(edition) if edition == target)
        && observation.language() == LanguageEvidence::Slovak
        && matches!(
            observation.observed_at().offset(),
            OffsetState::Known { .. }
        )
        && observation.observed_at().instant().is_some()
        && candidate.assessment.temporal() != TemporalAssessment::Unknown
}

fn stock_candidate_text(
    view: &CurrentView,
    availability: SourceAvailability,
    target: EditionId,
) -> String {
    let Some(candidate) = supported_candidate(view, availability, target) else {
        return "Unverified -- insufficient support".to_owned();
    };
    let Some(instant) = candidate.record().observation().observed_at().instant() else {
        return "Unverified -- observation time unknown".to_owned();
    };
    let time = format_unix(instant);
    match availability {
        SourceAvailability::Unavailable => format!("Last observed unavailable -- {time}"),
        SourceAvailability::Reserved => format!("Last observed reserved -- {time}"),
        SourceAvailability::Available => format!("Last observed available -- {time}"),
        SourceAvailability::Unknown => "Unverified -- source availability unknown".to_owned(),
    }
}

fn stock_text(claim: &StockClaim) -> String {
    let time = format_unix(claim.observed_at);
    match claim.availability {
        SourceAvailability::Available => format!("Last observed available -- {time}"),
        SourceAvailability::Reserved => format!("Last observed reserved -- {time}"),
        SourceAvailability::Unavailable => format!("Last observed unavailable -- {time}"),
        SourceAvailability::Unknown => "Unverified -- source availability unknown".to_owned(),
    }
}

fn latest_time(view: &CurrentView) -> String {
    view.last_ordered_attempts
        .iter()
        .filter_map(|record| record.observation().observed_at().instant())
        .max()
        .map(format_unix)
        .unwrap_or_else(|| "unknown".to_owned())
}

fn render_attempt(record: &ObservationRecord) -> String {
    let observation = record.observation();
    let observed = match observation.observed_at().instant() {
        Some(instant) => format_unix(instant),
        None => "unknown".to_owned(),
    };
    format!(
        "attempt={} observed={observed} access={} listing={} availability={}",
        observation.reference().observation().get(),
        access_token(observation.access()),
        listing_token(observation.listing()),
        availability_token(observation.availability())
    )
}

fn render_history_row(entry: &HistoryEntry) -> String {
    let observation = entry.record().observation();
    let observed = match observation.observed_at().instant() {
        Some(instant) => format_bare(instant),
        None => "unknown".to_owned(),
    };
    let review = entry
        .reviews()
        .last()
        .map(|review| knowledge_token(review.knowledge()))
        .unwrap_or("unreviewed");
    format!(
        "{} observed={observed} ingested={} access={} listing={} availability={} review={review}",
        observation.reference().observation().get(),
        format_bare(entry.record().ingested_at()),
        access_token(observation.access()),
        listing_token(observation.listing()),
        availability_token(observation.availability())
    )
}

fn join_ids(ids: impl Iterator<Item = u64>) -> String {
    let mut text = String::new();
    for (index, id) in ids.enumerate() {
        if index > 0 {
            text.push(',');
        }
        text.push_str(&id.to_string());
    }
    if text.is_empty() {
        "none".to_owned()
    } else {
        text
    }
}

fn format_unix(instant: SystemTime) -> String {
    match store::encode_instant(instant) {
        Ok((seconds, 0)) => format!("unix:{seconds}"),
        Ok((seconds, nanos)) => format!("unix:{seconds}.{nanos:09}"),
        Err(_) => "unix:unrepresentable".to_owned(),
    }
}

fn format_bare(instant: SystemTime) -> String {
    match store::encode_instant(instant) {
        Ok((seconds, 0)) => seconds.to_string(),
        Ok((seconds, nanos)) => format!("{seconds}.{nanos:09}"),
        Err(_) => "unknown".to_owned(),
    }
}

fn records_sorted(entries: Vec<&HistoryEntry>) -> Vec<ObservationRecord> {
    let mut records: Vec<_> = entries
        .into_iter()
        .map(|entry| entry.record().clone())
        .collect();
    records.sort_by_key(observation_id_of);
    records
}

fn max_instant(entries: &[&HistoryEntry]) -> Option<SystemTime> {
    entries
        .iter()
        .filter_map(|entry| observed_instant(entry))
        .max()
}

fn at_instant<'a>(entries: &[&'a HistoryEntry], instant: SystemTime) -> Vec<&'a HistoryEntry> {
    let mut selected: Vec<_> = entries
        .iter()
        .copied()
        .filter(|entry| observed_instant(entry) == Some(instant))
        .collect();
    selected.sort_by_key(|entry| entry_id(entry));
    selected
}

fn observed_instant(entry: &HistoryEntry) -> Option<SystemTime> {
    entry.record().observation().observed_at().instant()
}

fn entry_id(entry: &HistoryEntry) -> u64 {
    entry.record().observation().reference().observation().get()
}

fn observation_id_of(record: &ObservationRecord) -> u64 {
    record.observation().reference().observation().get()
}

fn latest_knowledge(
    entry: &HistoryEntry,
) -> (
    KnowledgeAssessment,
    Option<bookradar_core::ReviewerApproval>,
) {
    match entry.reviews.last() {
        None => (KnowledgeAssessment::Unreviewed, None),
        Some(review) => (review.knowledge(), review.approval().cloned()),
    }
}

fn is_rejected(entry: &HistoryEntry) -> bool {
    latest_knowledge(entry).0 == KnowledgeAssessment::Rejected
}

fn intrinsic_conflict(observation: &Observation) -> bool {
    matches!(observation.subject(), SubjectIdentity::Conflicting)
        || observation.language() == LanguageEvidence::Conflicting
}

fn resolved_edition(observation: &Observation) -> Option<EditionId> {
    match observation.subject() {
        SubjectIdentity::Resolved(edition) => Some(edition),
        SubjectIdentity::Unresolved | SubjectIdentity::Conflicting => None,
    }
}

fn explicit_language(language: LanguageEvidence) -> Option<LanguageEvidence> {
    match language {
        LanguageEvidence::Slovak | LanguageEvidence::Czech | LanguageEvidence::Mixed => {
            Some(language)
        }
        LanguageEvidence::Unknown | LanguageEvidence::Conflicting => None,
    }
}

fn explicit_availability(availability: SourceAvailability) -> Option<SourceAvailability> {
    match availability {
        SourceAvailability::Available
        | SourceAvailability::Reserved
        | SourceAvailability::Unavailable => Some(availability),
        SourceAvailability::Unknown => None,
    }
}

fn definite_listing(listing: ListingDisposition) -> Option<ListingDisposition> {
    match listing {
        ListingDisposition::Present | ListingDisposition::Removed => Some(listing),
        ListingDisposition::Uncertain => None,
    }
}

fn instant_is_future(record: &ObservationRecord, now: SystemTime) -> bool {
    record
        .observation()
        .observed_at()
        .instant()
        .is_some_and(|instant| instant > now)
}

fn verification_token(view: &CurrentView) -> &'static str {
    if view.candidates.iter().any(|candidate| {
        matches!(
            candidate.assessment.eligibility(),
            Eligibility::Eligible { .. }
        )
    }) {
        "eligible"
    } else {
        "not-eligible"
    }
}

fn temporal_token(temporal: TemporalAssessment) -> &'static str {
    match temporal {
        TemporalAssessment::WithinInterval => "within-interval",
        TemporalAssessment::Stale => "stale",
        TemporalAssessment::Unknown => "unknown",
    }
}

fn conflict_token(conflict: ConflictState) -> &'static str {
    match conflict {
        ConflictState::Clear => "clear",
        ConflictState::Unresolved => "unresolved",
        ConflictState::Unknown => "unknown",
    }
}

fn recheck_token(recheck: RecheckState) -> &'static str {
    match recheck {
        RecheckState::Clear => "clear",
        RecheckState::Failed => "failed",
        RecheckState::Unknown => "unknown",
    }
}
