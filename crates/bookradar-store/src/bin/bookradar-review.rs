//! Reviewer-governed show, promote, and review commands.
//!
//! `show` and the read half of `promote` open the research ledger read-only.
//! `promote` opens the application store only after the candidate is admitted
//! and the reviewer attestation is complete. `review` writes only the
//! application store.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::env;
use std::path::Path;
use std::process::ExitCode;
use std::time::{Duration, SystemTime};

use bookradar_core::{
    AccessResult, EvidenceMethod, KnowledgeAssessment, LanguageEvidence, ListingDisposition,
    ObservationId, ObservationRef, OfferId, OfferRef, ReviewerApproval, SourceAvailability,
    SourceId, SubjectIdentity,
};
use bookradar_store::{
    CandidateQuery, OpenMode, PromotionRequest, ResearchStore, ReviewDraft, ReviewWorkflowError,
    Store, StoreError, append_application_review, is_research_identifier, promote,
};

const SHOW_USAGE: &str = "show --ledger <path> --producer <identifier> --run <identifier> --finding <identifier> --receipt <positive-integer> --position <nonnegative-integer>";
const PROMOTE_USAGE: &str = "promote --ledger <path> --store <path> --producer <identifier> --run <identifier> --finding <identifier> --receipt <positive-integer> --position <nonnegative-integer> --source-id <positive-integer> --offer-id <positive-integer> --observation-id <positive-integer> --edition-id <positive-integer> --language slovak --access readable --listing present --availability available --source-statement <text> --evidence-method direct-browser|manual --evidence-reference <text> --cover-subject --cover-language --cover-availability --reviewer-approval <text> --assessor <text> --rationale <text> --observed-at <unix-seconds> --observed-offset <seconds> [--captured-at <unix-seconds> --captured-offset <seconds>] --ingested-at <unix-seconds> --assessed-at <unix-seconds> --recorded-at <unix-seconds>";
const REVIEW_USAGE: &str = "review --store <path> --source-id <positive-integer> --offer-id <positive-integer> --observation-id <positive-integer> --expected-revision <nonnegative-integer> --knowledge supported|disputed|rejected --assessor <text> --rationale <text> --assessed-at <unix-seconds> --recorded-at <unix-seconds> [--reviewer-approval <text>]";

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(command) = args.next() else {
        eprintln!("usage: expected show, promote, or review");
        return ExitCode::from(2);
    };
    match dispatch(&command, args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(CliError::Usage(message)) => {
            eprintln!("usage: {message}");
            ExitCode::from(2)
        }
        Err(CliError::Workflow(error)) => {
            eprintln!("{}: {error}", error.category());
            ExitCode::from(1)
        }
    }
}

enum CliError {
    Usage(String),
    Workflow(ReviewWorkflowError),
}

impl From<StoreError> for CliError {
    fn from(error: StoreError) -> Self {
        Self::Workflow(ReviewWorkflowError::Store(error))
    }
}

impl From<ReviewWorkflowError> for CliError {
    fn from(error: ReviewWorkflowError) -> Self {
        Self::Workflow(error)
    }
}

fn dispatch(command: &str, args: impl Iterator<Item = String>) -> Result<(), CliError> {
    match command {
        "show" => show(args),
        "promote" => promote_command(args),
        "review" => review_command(args),
        _ => Err(CliError::Usage(
            "expected show, promote, or review".to_owned(),
        )),
    }
}

fn show(args: impl Iterator<Item = String>) -> Result<(), CliError> {
    let parsed = parse_flags(args, SHOW_USAGE)?;
    reject_unexpected(
        &parsed,
        &[
            "ledger", "producer", "run", "finding", "receipt", "position",
        ],
        SHOW_USAGE,
    )?;
    let ledger = required(&parsed, "ledger", SHOW_USAGE)?;
    let query = candidate_query(&parsed, SHOW_USAGE)?;
    no_switches(&parsed, SHOW_USAGE)?;
    let mut store = ResearchStore::open(Path::new(ledger), OpenMode::ReadOnly)?;
    let Some(candidate) = store.candidate(&query)? else {
        return Err(ReviewWorkflowError::Refused {
            reason: "unknown-finding",
        }
        .into());
    };
    println!("{}", render_show(&candidate));
    Ok(())
}

fn promote_command(args: impl Iterator<Item = String>) -> Result<(), CliError> {
    let parsed = parse_flags(args, PROMOTE_USAGE)?;
    reject_unexpected(
        &parsed,
        &[
            "ledger",
            "store",
            "producer",
            "run",
            "finding",
            "receipt",
            "position",
            "source-id",
            "offer-id",
            "observation-id",
            "edition-id",
            "language",
            "access",
            "listing",
            "availability",
            "source-statement",
            "evidence-method",
            "evidence-reference",
            "reviewer-approval",
            "assessor",
            "rationale",
            "observed-at",
            "observed-offset",
            "captured-at",
            "captured-offset",
            "ingested-at",
            "assessed-at",
            "recorded-at",
        ],
        PROMOTE_USAGE,
    )?;
    require_switches(
        &parsed,
        &["cover-subject", "cover-language", "cover-availability"],
        PROMOTE_USAGE,
    )?;
    let ledger = required(&parsed, "ledger", PROMOTE_USAGE)?.to_owned();
    let store = required(&parsed, "store", PROMOTE_USAGE)?.to_owned();
    let request = promotion_request(&parsed)?;
    let mut ledger = ResearchStore::open(Path::new(&ledger), OpenMode::ReadOnly)?;
    promote(&mut ledger, Path::new(&store), &request)?;
    println!(
        "promoted source={} offer={} observation={} provenance=research:{}/{}/{}@{}/{}",
        request.source_id,
        request.offer_id,
        request.observation_id,
        request.query.producer_id,
        request.query.run_id,
        request.query.finding_id,
        request.query.receipt_id,
        request.query.position
    );
    Ok(())
}

fn review_command(args: impl Iterator<Item = String>) -> Result<(), CliError> {
    let parsed = parse_flags(args, REVIEW_USAGE)?;
    reject_unexpected(
        &parsed,
        &[
            "store",
            "source-id",
            "offer-id",
            "observation-id",
            "expected-revision",
            "knowledge",
            "assessor",
            "rationale",
            "assessed-at",
            "recorded-at",
            "reviewer-approval",
        ],
        REVIEW_USAGE,
    )?;
    no_switches(&parsed, REVIEW_USAGE)?;
    let path = required(&parsed, "store", REVIEW_USAGE)?.to_owned();
    let source_id = positive_id(required(&parsed, "source-id", REVIEW_USAGE)?, "source-id")?;
    let offer_id = positive_id(required(&parsed, "offer-id", REVIEW_USAGE)?, "offer-id")?;
    let observation_id = positive_id(
        required(&parsed, "observation-id", REVIEW_USAGE)?,
        "observation-id",
    )?;
    let expected = nonnegative_id(
        required(&parsed, "expected-revision", REVIEW_USAGE)?,
        "expected-revision",
    )?;
    let knowledge = match required(&parsed, "knowledge", REVIEW_USAGE)? {
        "supported" => KnowledgeAssessment::Supported,
        "disputed" => KnowledgeAssessment::Disputed,
        "rejected" => KnowledgeAssessment::Rejected,
        _ => {
            return Err(CliError::Usage(format!(
                "{REVIEW_USAGE}; --knowledge must be supported, disputed, or rejected"
            )));
        }
    };
    let approval = match knowledge {
        KnowledgeAssessment::Supported => Some(text_value(
            required(&parsed, "reviewer-approval", REVIEW_USAGE)?,
            "--reviewer-approval",
            REVIEW_USAGE,
        )?),
        _ => match parsed.values.get("reviewer-approval") {
            None => None,
            Some(value) => Some(text_value(value, "--reviewer-approval", REVIEW_USAGE)?),
        },
    };
    let assessor = text_value(
        required(&parsed, "assessor", REVIEW_USAGE)?,
        "--assessor",
        REVIEW_USAGE,
    )?;
    let rationale = text_value(
        required(&parsed, "rationale", REVIEW_USAGE)?,
        "--rationale",
        REVIEW_USAGE,
    )?;
    let assessed_at = unix_time(
        required(&parsed, "assessed-at", REVIEW_USAGE)?,
        "--assessed-at",
    )?;
    let recorded_at = unix_time(
        required(&parsed, "recorded-at", REVIEW_USAGE)?,
        "--recorded-at",
    )?;
    let draft = ReviewDraft::try_new(
        knowledge,
        approval
            .as_deref()
            .map(ReviewerApproval::new)
            .transpose()
            .map_err(|_| CliError::Usage(format!("{REVIEW_USAGE}; empty reviewer approval")))?,
        assessor,
        rationale,
        assessed_at,
        recorded_at,
    )
    .map_err(|_| CliError::Usage(format!("{REVIEW_USAGE}; empty review text")))?;
    let observation = ObservationRef::new(
        OfferRef::new(
            SourceId::new(source_id).expect("source"),
            OfferId::new(offer_id).expect("offer"),
        ),
        ObservationId::new(observation_id).expect("observation"),
    );
    let mut store = Store::open(Path::new(&path), OpenMode::ReadWrite)?;
    let revision = append_application_review(&mut store, observation, expected, &draft)?;
    println!(
        "reviewed source={source_id} offer={offer_id} observation={observation_id} revision={revision}"
    );
    Ok(())
}

fn promotion_request(parsed: &Parsed) -> Result<PromotionRequest, CliError> {
    let query = candidate_query(parsed, PROMOTE_USAGE)?;
    let language = required(parsed, "language", PROMOTE_USAGE)?;
    if language != "slovak" {
        return Err(CliError::Usage(format!(
            "{PROMOTE_USAGE}; --language must be slovak"
        )));
    }
    let access = required(parsed, "access", PROMOTE_USAGE)?;
    if access != "readable" {
        return Err(CliError::Usage(format!(
            "{PROMOTE_USAGE}; --access must be readable"
        )));
    }
    let listing = required(parsed, "listing", PROMOTE_USAGE)?;
    if listing != "present" {
        return Err(CliError::Usage(format!(
            "{PROMOTE_USAGE}; --listing must be present"
        )));
    }
    let availability = required(parsed, "availability", PROMOTE_USAGE)?;
    if availability != "available" {
        return Err(CliError::Usage(format!(
            "{PROMOTE_USAGE}; --availability must be available"
        )));
    }
    let method = match required(parsed, "evidence-method", PROMOTE_USAGE)? {
        "direct-browser" => EvidenceMethod::DirectBrowser,
        "manual" => EvidenceMethod::Manual,
        "agent-only" => {
            return Err(CliError::Usage(format!(
                "{PROMOTE_USAGE}; --evidence-method must be direct-browser or manual"
            )));
        }
        _ => {
            return Err(CliError::Usage(format!(
                "{PROMOTE_USAGE}; --evidence-method must be direct-browser or manual"
            )));
        }
    };
    let edition_id = positive_id(required(parsed, "edition-id", PROMOTE_USAGE)?, "edition-id")?;
    let observed_at = unix_time(
        required(parsed, "observed-at", PROMOTE_USAGE)?,
        "--observed-at",
    )?;
    let observed_offset = offset(required(parsed, "observed-offset", PROMOTE_USAGE)?)?;
    let (captured_at, captured_offset) = captured(parsed, observed_at, observed_offset)?;
    Ok(PromotionRequest {
        query,
        source_id: positive_id(required(parsed, "source-id", PROMOTE_USAGE)?, "source-id")?,
        offer_id: positive_id(required(parsed, "offer-id", PROMOTE_USAGE)?, "offer-id")?,
        observation_id: positive_id(
            required(parsed, "observation-id", PROMOTE_USAGE)?,
            "observation-id",
        )?,
        subject: SubjectIdentity::Resolved(
            bookradar_core::EditionId::new(edition_id).expect("edition"),
        ),
        language: LanguageEvidence::Slovak,
        access: AccessResult::Readable,
        listing: ListingDisposition::Present,
        availability: SourceAvailability::Available,
        source_statement: text_value(
            required(parsed, "source-statement", PROMOTE_USAGE)?,
            "--source-statement",
            PROMOTE_USAGE,
        )?,
        evidence_method: method,
        evidence_reference: text_value(
            required(parsed, "evidence-reference", PROMOTE_USAGE)?,
            "--evidence-reference",
            PROMOTE_USAGE,
        )?,
        cover_subject: true,
        cover_language: true,
        cover_availability: true,
        reviewer_approval: Some(text_value(
            required(parsed, "reviewer-approval", PROMOTE_USAGE)?,
            "--reviewer-approval",
            PROMOTE_USAGE,
        )?),
        assessor: text_value(
            required(parsed, "assessor", PROMOTE_USAGE)?,
            "--assessor",
            PROMOTE_USAGE,
        )?,
        rationale: text_value(
            required(parsed, "rationale", PROMOTE_USAGE)?,
            "--rationale",
            PROMOTE_USAGE,
        )?,
        observed_at,
        observed_offset,
        captured_at,
        captured_offset,
        ingested_at: unix_time(
            required(parsed, "ingested-at", PROMOTE_USAGE)?,
            "--ingested-at",
        )?,
        assessed_at: unix_time(
            required(parsed, "assessed-at", PROMOTE_USAGE)?,
            "--assessed-at",
        )?,
        recorded_at: unix_time(
            required(parsed, "recorded-at", PROMOTE_USAGE)?,
            "--recorded-at",
        )?,
    })
}

fn captured(
    parsed: &Parsed,
    observed_at: SystemTime,
    observed_offset: i32,
) -> Result<(SystemTime, i32), CliError> {
    match (
        parsed.values.get("captured-at"),
        parsed.values.get("captured-offset"),
    ) {
        (None, None) => Ok((observed_at, observed_offset)),
        (Some(at), Some(offset_text)) => {
            Ok((unix_time(at, "--captured-at")?, offset(offset_text)?))
        }
        _ => Err(CliError::Usage(format!(
            "{PROMOTE_USAGE}; --captured-at and --captured-offset must be supplied together"
        ))),
    }
}

fn candidate_query(parsed: &Parsed, usage: &str) -> Result<CandidateQuery, CliError> {
    let producer = identifier(required(parsed, "producer", usage)?, "producer", usage)?;
    let run = identifier(required(parsed, "run", usage)?, "run", usage)?;
    let finding = identifier(required(parsed, "finding", usage)?, "finding", usage)?;
    Ok(CandidateQuery {
        producer_id: producer,
        run_id: run,
        finding_id: finding,
        receipt_id: positive_id(required(parsed, "receipt", usage)?, "receipt")?,
        position: nonnegative_id(required(parsed, "position", usage)?, "position")?,
    })
}

fn render_show(candidate: &bookradar_store::CandidateRecord) -> String {
    format!(
        "producer={}\nrun={}\nfinding={}\nreceipt={}\nposition={}\noutcome={}\nreason={}\nledger_knowledge={}\nsource_ref={}\nlisting_ref={}\nlisting_generation={}\nledger_source_id={}\nledger_offer_id={}\nledger_observation_id={}\ncanonical_len={}\ncanonical={}\n",
        candidate.producer_id,
        candidate.run_id,
        candidate.finding_id,
        candidate.receipt_id,
        candidate.position,
        candidate.outcome,
        candidate.reason,
        optional_text(candidate.ledger_knowledge.as_deref()),
        optional_text(candidate.source_ref.as_deref()),
        optional_text(candidate.listing_ref.as_deref()),
        optional_text(candidate.listing_generation.as_deref()),
        optional_id(candidate.ledger_source_id),
        optional_id(candidate.ledger_offer_id),
        optional_id(candidate.ledger_observation_id),
        candidate.canonical_finding.as_ref().map_or(0, Vec::len),
        candidate
            .canonical_finding
            .as_deref()
            .map(escape_data)
            .unwrap_or_else(|| "none".to_owned())
    )
}

fn optional_text(value: Option<&str>) -> &str {
    value.unwrap_or("none")
}

fn optional_id(value: Option<u64>) -> String {
    match value {
        Some(value) => value.to_string(),
        None => "none".to_owned(),
    }
}

fn escape_data(bytes: &[u8]) -> String {
    let mut out = String::new();
    for &byte in bytes {
        match byte {
            b'\\' => out.push_str("\\\\"),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            b'\t' => out.push_str("\\t"),
            0x20..=0x7e => out.push(char::from(byte)),
            _ => out.push_str(&format!("\\x{byte:02x}")),
        }
    }
    out
}

struct Parsed {
    values: BTreeMap<String, String>,
    switches: BTreeMap<String, ()>,
}

fn parse_flags(args: impl Iterator<Item = String>, usage: &str) -> Result<Parsed, CliError> {
    let mut values = BTreeMap::new();
    let mut switches = BTreeMap::new();
    let mut args = args.peekable();
    while let Some(flag) = args.next() {
        if !flag.starts_with("--") || flag == "--" {
            return Err(CliError::Usage(format!("{usage}; unexpected argument")));
        }
        let name = flag.trim_start_matches("--");
        if name.is_empty() {
            return Err(CliError::Usage(format!("{usage}; unexpected argument")));
        }
        if matches!(
            name,
            "cover-subject" | "cover-language" | "cover-availability"
        ) {
            if switches.insert(name.to_owned(), ()).is_some() {
                return Err(CliError::Usage(format!("{usage}; duplicate --{name}")));
            }
            continue;
        }
        let Some(value) = args.next() else {
            return Err(CliError::Usage(format!(
                "{usage}; missing value for --{name}"
            )));
        };
        if value.starts_with("--") {
            return Err(CliError::Usage(format!(
                "{usage}; missing value for --{name}"
            )));
        }
        if values.insert(name.to_owned(), value).is_some() {
            return Err(CliError::Usage(format!("{usage}; duplicate --{name}")));
        }
    }
    Ok(Parsed { values, switches })
}

fn reject_unexpected(parsed: &Parsed, allowed: &[&str], usage: &str) -> Result<(), CliError> {
    for name in parsed.values.keys() {
        if !allowed.contains(&name.as_str()) {
            return Err(CliError::Usage(format!("{usage}; unknown option --{name}")));
        }
    }
    Ok(())
}

fn no_switches(parsed: &Parsed, usage: &str) -> Result<(), CliError> {
    if let Some(name) = parsed.switches.keys().next() {
        return Err(CliError::Usage(format!("{usage}; unknown option --{name}")));
    }
    Ok(())
}

fn require_switches(parsed: &Parsed, names: &[&str], usage: &str) -> Result<(), CliError> {
    for name in parsed.switches.keys() {
        if !names.contains(&name.as_str()) {
            return Err(CliError::Usage(format!("{usage}; unknown option --{name}")));
        }
    }
    for name in names {
        if !parsed.switches.contains_key(*name) {
            return Err(CliError::Usage(format!("{usage}; missing --{name}")));
        }
    }
    Ok(())
}

fn required<'a>(parsed: &'a Parsed, name: &str, usage: &str) -> Result<&'a str, CliError> {
    parsed
        .values
        .get(name)
        .map(String::as_str)
        .ok_or_else(|| CliError::Usage(format!("{usage}; missing --{name}")))
}

fn identifier(value: &str, name: &str, usage: &str) -> Result<String, CliError> {
    if is_research_identifier(value) {
        Ok(value.to_owned())
    } else {
        Err(CliError::Usage(format!(
            "{usage}; --{name} must be an identifier"
        )))
    }
}

fn text_value(value: &str, flag: &str, usage: &str) -> Result<String, CliError> {
    if value.is_empty() || value.contains('\0') {
        Err(CliError::Usage(format!("{usage}; {flag} must be nonempty")))
    } else {
        Ok(value.to_owned())
    }
}

fn positive_id(value: &str, name: &str) -> Result<u64, CliError> {
    parse_u64(value, false).ok_or_else(|| {
        CliError::Usage(format!(
            "expected show, promote, or review; --{name} must be a positive integer that fits the store"
        ))
    })
}

fn nonnegative_id(value: &str, name: &str) -> Result<u64, CliError> {
    parse_u64(value, true).ok_or_else(|| {
        CliError::Usage(format!(
            "expected show, promote, or review; --{name} must be a nonnegative integer that fits the store"
        ))
    })
}

fn parse_u64(value: &str, allow_zero: bool) -> Option<u64> {
    if value.is_empty() || value.starts_with('+') || value.starts_with('-') {
        return None;
    }
    let parsed: u64 = value.parse().ok()?;
    if !allow_zero && parsed == 0 {
        return None;
    }
    i64::try_from(parsed).ok()?;
    Some(parsed)
}

fn unix_time(value: &str, flag: &str) -> Result<SystemTime, CliError> {
    parse_unix_seconds(value)
        .ok_or_else(|| CliError::Usage(format!("{flag} must be an integer number of Unix seconds")))
}

fn parse_unix_seconds(text: &str) -> Option<SystemTime> {
    if text.is_empty() || text.starts_with('+') {
        return None;
    }
    let seconds: i64 = text.parse().ok()?;
    if seconds >= 0 {
        Some(SystemTime::UNIX_EPOCH + Duration::from_secs(u64::try_from(seconds).ok()?))
    } else {
        let absolute = u64::try_from(seconds.checked_neg()?).ok()?;
        SystemTime::UNIX_EPOCH.checked_sub(Duration::from_secs(absolute))
    }
}

fn offset(text: &str) -> Result<i32, CliError> {
    if text.is_empty() || text.starts_with('+') {
        return Err(CliError::Usage(
            "--observed-offset must be an integer from -64800 through 64800".to_owned(),
        ));
    }
    let value: i32 = text.parse().map_err(|_| {
        CliError::Usage("--observed-offset must be an integer from -64800 through 64800".to_owned())
    })?;
    if (-64_800..=64_800).contains(&value) {
        Ok(value)
    } else {
        Err(CliError::Usage(
            "--observed-offset must be an integer from -64800 through 64800".to_owned(),
        ))
    }
}
