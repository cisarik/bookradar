//! Structural checks for the versioned research contract.

#![forbid(unsafe_code)]

use std::time::{Duration, SystemTime};

use bookradar_store::{ImportContext, ResearchStore, ValidationOutcome, validate_research};

fn temp() -> Dir {
    tempfile_dir()
}

struct Dir(std::path::PathBuf);

fn tempfile_dir() -> Dir {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "bookradar-research-contract-{}-{n}",
        std::process::id()
    ));
    std::fs::create_dir(&path).expect("temp");
    Dir(path)
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn at(seconds: i64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(seconds as u64)
}

fn ledger(dir: &Dir) -> (std::path::PathBuf, ResearchStore) {
    let path = dir.0.join("ledger.sqlite");
    let store = ResearchStore::create(&path).expect("create");
    (path, store)
}

fn context() -> &'static str {
    r#"{"agent":{"name":"synthetic-agent","version":"0"},"model":{"name":"synthetic-model","version":null},"skills":[],"tools":null,"context":"synthetic-context"}"#
}

fn envelope(findings: &str) -> String {
    format!(
        r#"{{"contract":"bookradar-research","contract_version":1,"producer_id":"synthetic:producer","run_id":"synthetic:run","run_context":{},"reported_run_outcome":"succeeded","findings":[{findings}]}}"#,
        context()
    )
}

#[test]
fn malformed_envelopes_are_quarantined_without_candidates() {
    let dir = temp();
    let (_path, mut store) = ledger(&dir);
    for (bytes, reason) in [
        (b"{".as_slice(), "malformed"),
        (&[0xff, 0xfe][..], "malformed"),
        (br#"{"contract_version":1}"#, "malformed-header"),
    ] {
        let report = validate_research(bytes);
        assert!(report.seen.is_none(), "{reason}");
        assert_eq!(report.envelope_reason, Some(reason));
        let stored = store
            .import(bytes, ImportContext::new(at(20)))
            .expect("import");
        assert!(stored.seen.is_none());
        assert_eq!(stored.admitted, 0);
        assert_eq!(stored.projected, 0);
    }
    let snapshot = store.inspect().expect("inspect");
    assert_eq!(snapshot.observations, 0);
    assert_eq!(snapshot.findings, 0);
    assert_eq!(snapshot.runs, 0);
    assert_eq!(snapshot.inputs.len(), 3);
    assert!(
        snapshot
            .inputs
            .iter()
            .all(|input| !input.raw_bytes.is_empty())
    );
}

#[test]
fn unsupported_version_is_not_interpreted_as_v1() {
    let dir = temp();
    let (_path, mut store) = ledger(&dir);
    let bytes = r#"{"contract":"bookradar-research","contract_version":2,"producer_id":"synthetic:producer","run_id":"synthetic:run","findings":[{"finding_id":"F1","availability":"available"}]}"#;
    let report = validate_research(bytes.as_bytes());
    assert_eq!(report.envelope_reason, Some("unsupported-version"));
    assert!(report.seen.is_none());
    assert!(report.positions.is_empty());
    store
        .import(bytes.as_bytes(), ImportContext::new(at(30)))
        .expect("import");
    let snapshot = store.inspect().expect("inspect");
    assert_eq!(snapshot.runs, 0);
    assert_eq!(snapshot.findings, 0);
    assert_eq!(snapshot.observations, 0);
    assert_eq!(snapshot.envelopes, 1);
}

#[test]
fn duplicate_keys_are_detected_before_object_materialization() {
    let dir = temp();
    let (_path, mut store) = ledger(&dir);
    let nested = format!(
        r#"{{"contract":"bookradar-research","contract_version":1,"producer_id":"synthetic:producer","run_id":"synthetic:run","run_context":{},"reported_run_outcome":"succeeded","findings":[{{"finding_id":"A","finding_id":"B"}}]}}"#,
        context()
    );
    for bytes in [
        br#"{"a":1,"a":2}"#.as_slice(),
        br#"{"a":1,"\u0061":2}"#,
        nested.as_bytes(),
    ] {
        let report = validate_research(bytes);
        assert_eq!(report.envelope_reason, Some("duplicate-key"));
        assert!(report.positions.is_empty());
        let stored = store
            .import(bytes, ImportContext::new(at(40)))
            .expect("import");
        assert_eq!(stored.projected, 0);
        assert!(stored.seen.is_none());
    }
    let snapshot = store.inspect().expect("inspect");
    assert_eq!(snapshot.observations, 0);
    assert_eq!(snapshot.findings, 0);
}

#[test]
fn unknown_fields_and_missing_required_fields_are_not_ignored() {
    let dir = temp();
    let (_path, mut store) = ledger(&dir);
    let header = format!(
        r#"{{"contract":"bookradar-research","contract_version":1,"producer_id":"synthetic:producer","run_id":"synthetic:run","run_context":{},"reported_run_outcome":"succeeded","extra":true,"findings":[]}}"#,
        context()
    );
    let header_report = validate_research(header.as_bytes());
    assert_eq!(header_report.envelope_reason, Some("unknown-field"));
    assert!(header_report.seen.is_none());
    store
        .import(header.as_bytes(), ImportContext::new(at(50)))
        .expect("header");

    let extra = r#"{"finding_id":"F2","extra":1,"source_ref":null,"listing_ref":null,"listing_generation":null,"subject":{"state":"unknown","candidate_ref":null,"identifiers":[]},"times":{"observed_at":{"original":null,"offset_seconds":null,"instant":null},"accessed_at":{"original":null,"offset_seconds":null,"instant":null},"captured_at":{"original":null,"offset_seconds":null,"instant":null},"source_updated_at":{"original":null,"offset_seconds":null,"instant":null}},"facts":{"language":null,"access":null,"listing":null,"availability":null,"source_statement":null,"price":null,"claim_text":null},"evidence":[],"uncertainty":{"rationale":null,"reported_confidence":null},"source_provenance":{"requested_url":null,"resolved_url":null,"redirect_reference":null}}"#;
    let finding = envelope(&format!("{},{extra}", valid_finding("F1", "available")));
    let parsed = validate_research(finding.as_bytes());
    assert!(parsed.envelope_reason.is_none());
    assert_eq!(parsed.positions.len(), 2);
    assert_eq!(parsed.positions[0].outcome, ValidationOutcome::Valid);
    assert_eq!(parsed.positions[1].outcome, ValidationOutcome::Quarantined);
    assert_eq!(parsed.positions[1].reason, "unknown-field");
    let stored = store
        .import(finding.as_bytes(), ImportContext::new(at(51)))
        .expect("finding");
    assert_eq!(stored.admitted, 1);
    assert_eq!(stored.quarantined, 1);
    assert_eq!(stored.projected, 0);
}

#[test]
fn input_limits_reject_without_storage() {
    let dir = temp();
    let (path, mut store) = ledger(&dir);
    let before = std::fs::read(&path).expect("bytes");
    let oversized = vec![b' '; 1_048_577];
    let error = store
        .import(&oversized, ImportContext::new(at(60)))
        .expect_err("oversize");
    assert!(matches!(error, bookradar_store::ResearchError::Oversized));
    drop(store);
    assert_eq!(std::fs::read(&path).expect("after"), before);

    let mut store = ResearchStore::open(&path, bookradar_store::OpenMode::ReadWrite).expect("open");
    let deep = format!("[{}]", "[".repeat(40) + &"]".repeat(40));
    let deep_report = validate_research(deep.as_bytes());
    assert_eq!(deep_report.envelope_reason, Some("nesting-limit"));
    store
        .import(deep.as_bytes(), ImportContext::new(at(61)))
        .expect("depth");
    let mut findings = String::new();
    for index in 0..257 {
        if index > 0 {
            findings.push(',');
        }
        findings.push_str(&valid_finding(&format!("F{index}"), "available"));
    }
    let limited = envelope(&findings);
    assert!(limited.len() <= 1_048_576, "{}", limited.len());
    let limited_report = validate_research(limited.as_bytes());
    assert_eq!(limited_report.envelope_reason, Some("finding-limit"));
    assert!(limited_report.seen.is_none());
    store
        .import(limited.as_bytes(), ImportContext::new(at(62)))
        .expect("limit");
    let snapshot = store.inspect().expect("inspect");
    assert_eq!(snapshot.findings, 0);
    assert_eq!(snapshot.observations, 0);
    assert_eq!(snapshot.envelopes, 2);
}

fn valid_finding(id: &str, availability: &str) -> String {
    format!(
        r#"{{"finding_id":"{id}","source_ref":null,"listing_ref":null,"listing_generation":null,"subject":{{"state":"unknown","candidate_ref":null,"identifiers":[]}},"times":{{"observed_at":{{"original":null,"offset_seconds":null,"instant":null}},"accessed_at":{{"original":null,"offset_seconds":null,"instant":null}},"captured_at":{{"original":null,"offset_seconds":null,"instant":null}},"source_updated_at":{{"original":null,"offset_seconds":null,"instant":null}}}},"facts":{{"language":null,"access":null,"listing":null,"availability":"{availability}","source_statement":null,"price":null,"claim_text":null}},"evidence":[],"uncertainty":{{"rationale":null,"reported_confidence":null}},"source_provenance":{{"requested_url":null,"resolved_url":null,"redirect_reference":null}}}}"#
    )
}
