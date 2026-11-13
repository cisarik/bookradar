//! Pure HTML for one already loaded snapshot. This module performs no I/O.

use std::time::SystemTime;

use bookradar_core::{
    AccessResult, ClaimCoverageState, Eligibility, EvidenceMethod, KnowledgeAssessment,
    LanguageEvidence, ListingDisposition, OffsetState, SourceAvailability, SubjectIdentity,
};
use bookradar_store::{
    CandidateAssessment, CurrentView, HistoryEntry, render_current_labels, synthetic_target,
};

use crate::snapshot::{Dashboard, ObservationReading, ResearchReading, Slot, SlotFailure};

const NOTICE: &str = "Verification describes an evidenced source observation, not a guarantee that a purchase will succeed.";

pub fn render_html(dashboard: &Dashboard) -> String {
    let mut out = String::new();
    out.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    out.push_str("<title>BookRadar</title>\n<link rel=\"stylesheet\" href=\"/style.css\">\n");
    out.push_str("</head>\n<body>\n");
    out.push_str("<a class=\"skip-link\" href=\"#content\">Skip to content</a>\n");
    out.push_str("<header class=\"page-header\"><h1>BookRadar</h1>\n");
    out.push_str("<p class=\"banner\">Synthetic data · Local · Read-only</p>\n");
    out.push_str("<p>Evaluation instant — Unix ");
    out.push_str(&dashboard.now_seconds.to_string());
    out.push_str("</p>\n");
    out.push_str(
        "<p>The snapshot and evaluation instant remain fixed until restart.</p>\n</header>\n",
    );
    render_query(&mut out, dashboard);
    out.push_str("<main id=\"content\">\n");
    render_current_section(&mut out, dashboard);
    render_history_section(&mut out, dashboard);
    render_ledger_section(&mut out, dashboard);
    render_research_candidates(&mut out, dashboard);
    out.push_str("</main>\n</body>\n</html>\n");
    out
}

fn render_query(out: &mut String, dashboard: &Dashboard) {
    out.push_str("<section id=\"query\" aria-labelledby=\"query-heading\">\n");
    out.push_str("<h2 id=\"query-heading\">Configured query</h2>\n<dl>\n");
    field(out, "Observation store", &dashboard.store_path);
    field(out, "Research ledger", &dashboard.ledger_path);
    field(
        out,
        "Observation offer",
        &format!(
            "source {}, offer {}",
            bookradar_store::synthetic_offer().source().get(),
            bookradar_store::synthetic_offer().offer().get()
        ),
    );
    field(out, "Target edition", &dashboard.target_edition.to_string());
    field(out, "Producer", &dashboard.producer);
    field(out, "Source reference", &dashboard.source_ref);
    field(out, "Listing reference", &dashboard.listing_ref);
    field(out, "Listing generation", &dashboard.listing_generation);
    field(
        out,
        "Evaluation instant",
        &format!("Unix {}", dashboard.now_seconds),
    );
    out.push_str("</dl>\n</section>\n");
}

fn render_current_section(out: &mut String, dashboard: &Dashboard) {
    out.push_str(
        "<section id=\"current-assessment\" aria-labelledby=\"current-assessment-heading\">\n",
    );
    out.push_str("<h2 id=\"current-assessment-heading\">Current assessment</h2>\n");
    match &dashboard.observations {
        Slot::Failed(failure) => render_failure(out, failure),
        Slot::Ready(reading) => render_current_ready(out, dashboard, reading),
    }
    out.push_str("</section>\n");
}

fn render_current_ready(out: &mut String, dashboard: &Dashboard, reading: &ObservationReading) {
    if reading.history.is_empty() || reading.current.is_empty() {
        out.push_str("<p>No observations for this offer.</p>\n");
        return;
    }
    let target = edition_or_default(dashboard.target_edition);
    let labels = render_current_labels(&reading.current, target, dashboard.now);
    for label in labels {
        out.push_str("<p class=\"truth\">");
        push_escaped(out, &present_label(&label));
        out.push_str("</p>\n");
    }
    out.push_str("<p class=\"notice\">");
    push_escaped(out, NOTICE);
    out.push_str("</p>\n");
    render_current_facets(out, &reading.current);
}

fn render_current_facets(out: &mut String, view: &CurrentView) {
    out.push_str("<h3>Candidate facets</h3>\n<dl>\n");
    field(
        out,
        "Candidate observations",
        &join_ids(view.candidates().iter().map(|candidate| {
            candidate
                .record()
                .observation()
                .reference()
                .observation()
                .get()
        })),
    );
    field(
        out,
        "Latest attempts",
        &join_ids(
            view.last_ordered_attempts()
                .iter()
                .map(|record| record.observation().reference().observation().get()),
        ),
    );
    field(
        out,
        "Unorderable observations",
        &join_ids(
            view.unorderable()
                .iter()
                .map(|record| record.observation().reference().observation().get()),
        ),
    );
    field(out, "Conflict", conflict_token(view.conflict()));
    field(out, "Recheck", recheck_token(view.recheck()));
    field(out, "Verification eligibility", verification_token(view));
    out.push_str("</dl>\n");
    for candidate in view.candidates() {
        render_candidate_facet(out, candidate);
    }
}

fn render_candidate_facet(out: &mut String, candidate: &CandidateAssessment) {
    let id = candidate
        .record()
        .observation()
        .reference()
        .observation()
        .get();
    out.push_str("<h3>Candidate ");
    out.push_str(&id.to_string());
    out.push_str("</h3>\n<dl>\n");
    field(
        out,
        "Temporal assessment",
        temporal_token(candidate.assessment().temporal()),
    );
    match candidate.assessment().eligibility() {
        Eligibility::Eligible {
            observed_at,
            expires_at,
            ..
        } => {
            field(out, "Verification eligibility", "eligible");
            field(out, "Observed at", &format_instant(observed_at));
            field(out, "Expires at", &format_instant(expires_at));
        }
        Eligibility::NotEligible => field(out, "Verification eligibility", "not-eligible"),
    }
    out.push_str("</dl>\n");
}

fn render_history_section(out: &mut String, dashboard: &Dashboard) {
    out.push_str(
        "<section id=\"observation-history\" aria-labelledby=\"observation-history-heading\">\n",
    );
    out.push_str("<h2 id=\"observation-history-heading\">Observation history</h2>\n");
    match &dashboard.observations {
        Slot::Failed(failure) => render_failure(out, failure),
        Slot::Ready(reading) if reading.history.is_empty() => {
            out.push_str("<p>No observations for this offer.</p>\n");
        }
        Slot::Ready(reading) => {
            for entry in &reading.history {
                render_history_entry(out, entry);
            }
        }
    }
    out.push_str("</section>\n");
}

fn render_history_entry(out: &mut String, entry: &HistoryEntry) {
    let record = entry.record();
    let observation = record.observation();
    let id = observation.reference().observation().get();
    out.push_str("<details>\n<summary>Observation ");
    out.push_str(&id.to_string());
    out.push_str("</summary>\n<dl>\n");
    field(out, "Observation", &id.to_string());
    field(
        out,
        "Source",
        &observation.reference().offer().source().get().to_string(),
    );
    field(
        out,
        "Offer",
        &observation.reference().offer().offer().get().to_string(),
    );
    field(out, "Subject", &subject_text(observation.subject()));
    field(out, "Language", language_token(observation.language()));
    field(out, "Access", access_token(observation.access()));
    field(
        out,
        "Listing disposition",
        listing_token(observation.listing()),
    );
    field(
        out,
        "Source-reported availability",
        availability_token(observation.availability()),
    );
    field(
        out,
        "Attributed source statement",
        observation
            .source_statement()
            .map(|statement| statement.as_str())
            .unwrap_or("not recorded"),
    );
    render_evidence(out, observation.evidence());
    field(
        out,
        "Requested URL",
        record.requested_url().unwrap_or("not recorded"),
    );
    field(
        out,
        "Resolved URL",
        record.resolved_url().unwrap_or("not recorded"),
    );
    field(
        out,
        "Redirect reference",
        record.redirect_reference().unwrap_or("not recorded"),
    );
    field(out, "Provenance", record.provenance_reference());
    field(
        out,
        "Uncertainty",
        record.uncertainty_rationale().unwrap_or("not recorded"),
    );
    out.push_str("</dl>\n");
    render_timestamp(out, "Observed time", observation.observed_at());
    render_timestamp(out, "Captured time", record.captured_at());
    out.push_str("<p>Ingestion time: ");
    push_escaped(out, &format_instant(record.ingested_at()));
    out.push_str("</p>\n");
    render_reviews(out, entry);
    out.push_str("</details>\n");
}

fn render_evidence(out: &mut String, evidence: Option<&bookradar_core::EvidenceSupport>) {
    let Some(evidence) = evidence else {
        field(out, "Evidence", "not recorded");
        return;
    };
    field(out, "Evidence method", method_token(evidence.method()));
    field(
        out,
        "Evidence reference",
        evidence
            .reference()
            .map(|reference| reference.as_str())
            .unwrap_or("not recorded"),
    );
    let coverage = evidence.coverage();
    field(out, "Subject coverage", coverage_token(coverage.subject()));
    field(
        out,
        "Language coverage",
        coverage_token(coverage.language()),
    );
    field(
        out,
        "Availability coverage",
        coverage_token(coverage.source_availability()),
    );
}

fn render_timestamp(out: &mut String, title: &str, assertion: &bookradar_core::TimestampAssertion) {
    out.push_str("<h3>");
    push_escaped(out, title);
    out.push_str("</h3>\n<dl>\n");
    field(
        out,
        "Original text",
        assertion.original().unwrap_or("unknown"),
    );
    field(out, "Offset", &offset_text(assertion.offset()));
    match assertion
        .instant()
        .map(|instant| (instant, instant_parts(instant)))
    {
        Some((instant, Some((seconds, nanos)))) => {
            field(out, "Unix instant", &format_instant(instant));
            field(out, "Signed seconds", &seconds.to_string());
            field(out, "Nanoseconds", &format!("{nanos:09}"));
        }
        Some((_, None)) | None => {
            field(out, "Unix instant", "unknown");
            field(out, "Signed seconds", "unknown");
            field(out, "Nanoseconds", "unknown");
        }
    }
    out.push_str("</dl>\n");
}

fn render_reviews(out: &mut String, entry: &HistoryEntry) {
    out.push_str("<h3>Reviews</h3>\n");
    if entry.reviews().is_empty() {
        out.push_str("<p>No review recorded.</p>\n");
        return;
    }
    for review in entry.reviews() {
        out.push_str("<article>\n<h4>Revision ");
        out.push_str(&review.revision().to_string());
        out.push_str("</h4>\n<dl>\n");
        field(out, "Knowledge", knowledge_token(review.knowledge()));
        field(
            out,
            "Approval reference",
            review
                .approval()
                .map(|approval| approval.as_str())
                .unwrap_or("not recorded"),
        );
        field(out, "Assessor reference", review.assessor_reference());
        field(out, "Rationale", review.rationale());
        field(out, "Assessed time", &format_instant(review.assessed_at()));
        field(out, "Recorded time", &format_instant(review.recorded_at()));
        out.push_str("</dl>\n</article>\n");
    }
}

fn render_ledger_section(out: &mut String, dashboard: &Dashboard) {
    out.push_str("<section id=\"research-ledger\" aria-labelledby=\"research-ledger-heading\">\n");
    out.push_str("<h2 id=\"research-ledger-heading\">Research ledger</h2>\n");
    match &dashboard.research {
        Slot::Failed(failure) => render_failure(out, failure),
        Slot::Ready(reading) => render_ledger_ready(out, reading),
    }
    out.push_str("</section>\n");
}

fn render_ledger_ready(out: &mut String, reading: &ResearchReading) {
    let ledger = &reading.ledger;
    if ledger.envelopes == 0 && ledger.findings == 0 && ledger.inputs.is_empty() {
        out.push_str("<p>Research ledger is empty.</p>\n");
    }
    out.push_str("<dl>\n");
    field(out, "Envelopes", &ledger.envelopes.to_string());
    field(out, "Runs", &ledger.runs.to_string());
    field(out, "Findings", &ledger.findings.to_string());
    field(
        out,
        "Projected observations",
        &ledger.observations.to_string(),
    );
    field(out, "Reviews", &ledger.reviews.to_string());
    field(out, "Editions", &ledger.editions.to_string());
    out.push_str("</dl>\n");
    render_receipt_table(out, reading);
    render_finding_table(out, reading);
}

fn render_receipt_table(out: &mut String, reading: &ResearchReading) {
    out.push_str("<div class=\"table-wrap\"><table>\n<caption>Research receipts</caption>\n");
    out.push_str("<thead><tr><th scope=\"col\">Receipt</th><th scope=\"col\">Disposition</th>");
    out.push_str("<th scope=\"col\">Reported outcome</th></tr></thead>\n<tbody>\n");
    for input in &reading.ledger.inputs {
        out.push_str("<tr><td>");
        push_escaped(out, &input.receipt_id.to_string());
        out.push_str("</td><td class=\"preserve\">");
        push_escaped(out, &input.disposition);
        out.push_str("</td><td class=\"preserve\">");
        push_escaped(out, input.reported_outcome.as_deref().unwrap_or("unknown"));
        out.push_str("</td></tr>\n");
    }
    out.push_str("</tbody>\n</table></div>\n");
}

fn render_finding_table(out: &mut String, reading: &ResearchReading) {
    out.push_str("<div class=\"table-wrap\"><table>\n<caption>Research findings</caption>\n");
    out.push_str("<thead><tr><th scope=\"col\">Receipt</th><th scope=\"col\">Position</th>");
    out.push_str("<th scope=\"col\">Outcome</th><th scope=\"col\">Reason</th>");
    out.push_str("<th scope=\"col\">Knowledge</th><th scope=\"col\">Projected observation</th>");
    out.push_str("</tr></thead>\n<tbody>\n");
    for row in &reading.ledger.findings_rows {
        out.push_str("<tr><td>");
        push_escaped(out, &row.receipt_id.to_string());
        out.push_str("</td><td>");
        push_escaped(out, &row.position.to_string());
        out.push_str("</td><td class=\"preserve\">");
        push_escaped(out, &row.outcome);
        out.push_str("</td><td class=\"preserve\">");
        push_escaped(out, &row.reason);
        out.push_str("</td><td class=\"preserve\">");
        push_escaped(out, row.knowledge.as_deref().unwrap_or("not recorded"));
        out.push_str("</td><td>");
        match row.observation_id {
            Some(id) => push_escaped(out, &id.to_string()),
            None => push_escaped(out, "not projected"),
        }
        out.push_str("</td></tr>\n");
    }
    out.push_str("</tbody>\n</table></div>\n");
}

fn render_research_candidates(out: &mut String, dashboard: &Dashboard) {
    out.push_str(
        "<section id=\"research-current-candidates\" aria-labelledby=\"research-current-heading\">\n",
    );
    out.push_str("<h2 id=\"research-current-heading\">Research current candidates</h2>\n");
    out.push_str("<dl>\n");
    field(out, "Producer", &dashboard.producer);
    field(out, "Source reference", &dashboard.source_ref);
    field(out, "Listing reference", &dashboard.listing_ref);
    field(out, "Listing generation", &dashboard.listing_generation);
    field(out, "Target edition", &dashboard.target_edition.to_string());
    field(
        out,
        "Evaluation instant",
        &format!("Unix {}", dashboard.now_seconds),
    );
    out.push_str("</dl>\n");
    match &dashboard.research {
        Slot::Failed(failure) => render_failure(out, failure),
        Slot::Ready(reading) => render_research_ready(out, reading),
    }
    out.push_str("</section>\n");
}

fn render_research_ready(out: &mut String, reading: &ResearchReading) {
    if reading.current.candidates().is_empty() {
        out.push_str("<p>No research candidates for this query.</p>\n");
        return;
    }
    if reading
        .current
        .candidates()
        .iter()
        .any(|candidate| !research_candidate_expected(candidate))
    {
        out.push_str(
            "<p class=\"error\">Unexpected research assessment — verification display suppressed</p>\n",
        );
        return;
    }
    for candidate in reading.current.candidates() {
        render_research_card(out, candidate, &reading.current);
    }
}

fn render_research_card(out: &mut String, candidate: &CandidateAssessment, view: &CurrentView) {
    let title = match candidate.context().knowledge() {
        KnowledgeAssessment::Disputed => "Research candidate — disputed",
        _ => "Research candidate — unreviewed",
    };
    out.push_str("<article class=\"candidate-card\">\n<h3>");
    push_escaped(out, title);
    out.push_str("</h3>\n<p class=\"truth\">Unverified</p>\n<dl>\n");
    field(out, "Evidence method", "agent-only");
    field(out, "Approval reference", "not recorded");
    field(
        out,
        "Temporal assessment",
        temporal_token(candidate.assessment().temporal()),
    );
    field(out, "Conflict", conflict_token(view.conflict()));
    field(out, "Recheck", recheck_token(view.recheck()));
    field(out, "Verification eligibility", "not-eligible");
    field(
        out,
        "Attributed source statement",
        candidate
            .record()
            .observation()
            .source_statement()
            .map(|statement| statement.as_str())
            .unwrap_or("not recorded"),
    );
    out.push_str("</dl>\n</article>\n");
}

fn research_candidate_expected(candidate: &CandidateAssessment) -> bool {
    if !matches!(
        candidate.context().knowledge(),
        KnowledgeAssessment::Unreviewed | KnowledgeAssessment::Disputed
    ) {
        return false;
    }
    if candidate.context().reviewer_approval().is_some() {
        return false;
    }
    let Some(evidence) = candidate.record().observation().evidence() else {
        return false;
    };
    if evidence.method() != EvidenceMethod::AgentOnly {
        return false;
    }
    matches!(
        candidate.assessment().eligibility(),
        Eligibility::NotEligible
    )
}

fn render_failure(out: &mut String, failure: &SlotFailure) {
    out.push_str("<p class=\"error\">");
    push_escaped(out, &failure.message);
    out.push_str("</p>\n");
}

fn present_label(raw: &str) -> String {
    raw.replace("; expires_unix=", "; expires at Unix ")
        .replace("unix:", "Unix ")
        .replace(" -- ", " — ")
}

fn field(out: &mut String, label: &str, value: &str) {
    out.push_str("<dt>");
    push_escaped(out, label);
    out.push_str("</dt><dd class=\"preserve\">");
    push_escaped(out, value);
    out.push_str("</dd>\n");
}

fn push_escaped(out: &mut String, text: &str) {
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(character),
        }
    }
}

fn join_ids(ids: impl Iterator<Item = u64>) -> String {
    let mut text = String::new();
    for (index, id) in ids.enumerate() {
        if index > 0 {
            text.push_str(", ");
        }
        text.push_str(&id.to_string());
    }
    if text.is_empty() {
        "none".to_owned()
    } else {
        text
    }
}

fn edition_or_default(id: u64) -> bookradar_core::EditionId {
    bookradar_core::EditionId::new(id).unwrap_or_else(|_| synthetic_target())
}

fn format_instant(instant: SystemTime) -> String {
    match instant_parts(instant) {
        Some((seconds, 0)) => format!("Unix {seconds}"),
        Some((seconds, nanos)) => format!("Unix {seconds}.{nanos:09}"),
        None => "unknown".to_owned(),
    }
}

fn instant_parts(instant: SystemTime) -> Option<(i64, u32)> {
    match instant.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(duration) => Some((
            i64::try_from(duration.as_secs()).ok()?,
            duration.subsec_nanos(),
        )),
        Err(error) => {
            let duration = error.duration();
            let nanos = duration.subsec_nanos();
            if nanos == 0 {
                Some((i64::try_from(duration.as_secs()).ok()?.checked_neg()?, 0))
            } else {
                let whole = duration.as_secs().checked_add(1)?;
                Some((
                    i64::try_from(whole).ok()?.checked_neg()?,
                    1_000_000_000 - nanos,
                ))
            }
        }
    }
}

fn offset_text(offset: OffsetState) -> String {
    match offset {
        OffsetState::Known {
            seconds_east_of_utc,
        } => format!("known, {seconds_east_of_utc} seconds east of UTC"),
        OffsetState::Unknown => "unknown".to_owned(),
    }
}

fn subject_text(subject: SubjectIdentity) -> String {
    match subject {
        SubjectIdentity::Resolved(edition) => format!("resolved, edition {}", edition.get()),
        SubjectIdentity::Unresolved => "unresolved".to_owned(),
        SubjectIdentity::Conflicting => "conflicting".to_owned(),
    }
}

fn language_token(language: LanguageEvidence) -> &'static str {
    match language {
        LanguageEvidence::Slovak => "slovak",
        LanguageEvidence::Czech => "czech",
        LanguageEvidence::Mixed => "mixed",
        LanguageEvidence::Unknown => "unknown",
        LanguageEvidence::Conflicting => "conflicting",
    }
}

fn access_token(access: AccessResult) -> &'static str {
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

fn listing_token(listing: ListingDisposition) -> &'static str {
    match listing {
        ListingDisposition::Present => "present",
        ListingDisposition::Removed => "removed",
        ListingDisposition::Uncertain => "uncertain",
    }
}

fn availability_token(availability: SourceAvailability) -> &'static str {
    match availability {
        SourceAvailability::Available => "available",
        SourceAvailability::Reserved => "reserved",
        SourceAvailability::Unavailable => "unavailable",
        SourceAvailability::Unknown => "unknown",
    }
}

fn method_token(method: EvidenceMethod) -> &'static str {
    match method {
        EvidenceMethod::DirectBrowser => "direct-browser",
        EvidenceMethod::Manual => "manual",
        EvidenceMethod::AgentOnly => "agent-only",
    }
}

fn knowledge_token(knowledge: KnowledgeAssessment) -> &'static str {
    match knowledge {
        KnowledgeAssessment::Unreviewed => "unreviewed",
        KnowledgeAssessment::Supported => "supported",
        KnowledgeAssessment::Disputed => "disputed",
        KnowledgeAssessment::Rejected => "rejected",
    }
}

fn coverage_token(state: ClaimCoverageState) -> &'static str {
    match state {
        ClaimCoverageState::Covered => "covered",
        ClaimCoverageState::Missing => "missing",
    }
}

fn temporal_token(temporal: bookradar_core::TemporalAssessment) -> &'static str {
    match temporal {
        bookradar_core::TemporalAssessment::WithinInterval => "within-interval",
        bookradar_core::TemporalAssessment::Stale => "stale",
        bookradar_core::TemporalAssessment::Unknown => "unknown",
    }
}

fn conflict_token(conflict: bookradar_core::ConflictState) -> &'static str {
    match conflict {
        bookradar_core::ConflictState::Clear => "clear",
        bookradar_core::ConflictState::Unresolved => "unresolved",
        bookradar_core::ConflictState::Unknown => "unknown",
    }
}

fn recheck_token(recheck: bookradar_core::RecheckState) -> &'static str {
    match recheck {
        bookradar_core::RecheckState::Clear => "clear",
        bookradar_core::RecheckState::Failed => "failed",
        bookradar_core::RecheckState::Unknown => "unknown",
    }
}

fn verification_token(view: &CurrentView) -> &'static str {
    if view.candidates().iter().any(|candidate| {
        matches!(
            candidate.assessment().eligibility(),
            Eligibility::Eligible { .. }
        )
    }) {
        "eligible"
    } else {
        "not-eligible"
    }
}
