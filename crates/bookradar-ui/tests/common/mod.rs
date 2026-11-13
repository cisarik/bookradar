#![allow(dead_code)]

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use bookradar_core::{
    AccessResult, ClaimCoverage, ClaimCoverageState, EditionId, EvidenceMethod, EvidenceReference,
    EvidenceSupport, KnowledgeAssessment, LanguageEvidence, ListingDisposition, Observation,
    ObservationId, ObservationRef, OfferId, OfferRef, ReviewerApproval, SourceAvailability,
    SourceId, SourceStatement, SubjectIdentity, TimestampAssertion,
};
use bookradar_store::{
    ImportContext, ObservationRecord, ResearchStore, ReviewDraft, Store, StoreError,
};
use bookradar_ui::{Config, Dashboard, load};

pub const T0: i64 = 1_793_523_600;
pub const INTERVAL: i64 = 86_400;

pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    pub fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        loop {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("bookradar-ui-{}-{n}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => return Self { path },
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("temp dir: {error}"),
            }
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn child(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

pub struct ChildGuard {
    child: Child,
}

impl ChildGuard {
    pub fn new(child: Child) -> Self {
        Self { child }
    }

    pub fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    pub fn wait_timeout(&mut self, limit: Duration) -> Option<ExitStatus> {
        let start = Instant::now();
        loop {
            match self.child.try_wait() {
                Ok(Some(status)) => return Some(status),
                Ok(None) if start.elapsed() > limit => return None,
                Ok(None) => thread::sleep(Duration::from_millis(20)),
                Err(error) => panic!("wait: {error}"),
            }
        }
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub struct RunningUi {
    guard: ChildGuard,
    pub port: u16,
}

impl Drop for RunningUi {
    fn drop(&mut self) {
        let _ = self.guard.child.kill();
        let _ = self.guard.child.wait();
    }
}

pub fn unix(seconds: i64) -> SystemTime {
    unix_nanos(seconds, 0)
}

pub fn unix_nanos(seconds: i64, nanos: u32) -> SystemTime {
    if seconds >= 0 {
        SystemTime::UNIX_EPOCH + Duration::new(seconds as u64, nanos)
    } else {
        let base = SystemTime::UNIX_EPOCH - Duration::from_secs(seconds.unsigned_abs());
        base + Duration::from_nanos(u64::from(nanos))
    }
}

pub fn known(seconds: i64, nanos: u32) -> TimestampAssertion {
    TimestampAssertion::known(
        Some(&format!("synthetic:{seconds}.{nanos:09}+01:00")),
        3_600,
        unix_nanos(seconds, nanos),
    )
    .expect("known time")
}

pub fn offer(source: u64, offer: u64) -> OfferRef {
    OfferRef::new(
        SourceId::new(source).expect("source"),
        OfferId::new(offer).expect("offer"),
    )
}

pub struct Sample {
    pub id: u64,
    pub source: u64,
    pub offer_id: u64,
    pub subject: SubjectIdentity,
    pub language: LanguageEvidence,
    pub observed: TimestampAssertion,
    pub captured: TimestampAssertion,
    pub ingested: SystemTime,
    pub access: AccessResult,
    pub listing: ListingDisposition,
    pub availability: SourceAvailability,
    pub statement: Option<String>,
    pub evidence: Option<EvidenceSupport>,
    pub requested_url: Option<String>,
    pub resolved_url: Option<String>,
    pub redirect: Option<String>,
    pub provenance: String,
    pub uncertainty: Option<String>,
    pub review: Option<SampleReview>,
}

pub struct SampleReview {
    pub knowledge: KnowledgeAssessment,
    pub approval: Option<String>,
    pub assessor: String,
    pub rationale: String,
    pub assessed: SystemTime,
    pub recorded: SystemTime,
}

impl Sample {
    pub fn qualifying(id: u64, seconds: i64) -> Self {
        Self {
            id,
            source: 1,
            offer_id: 1,
            subject: SubjectIdentity::Resolved(EditionId::new(10).expect("edition")),
            language: LanguageEvidence::Slovak,
            observed: known(seconds, 0),
            captured: known(seconds, 0),
            ingested: unix(seconds + 10),
            access: AccessResult::Readable,
            listing: ListingDisposition::Present,
            availability: SourceAvailability::Available,
            statement: Some(format!("synthetic:statement:{id}")),
            evidence: Some(manual_evidence(&format!("synthetic:evidence:{id}"))),
            requested_url: None,
            resolved_url: None,
            redirect: None,
            provenance: format!("synthetic:provenance:{id}"),
            uncertainty: None,
            review: Some(supported_review(unix(seconds + 10), id)),
        }
    }

    pub fn reference(&self) -> ObservationRef {
        ObservationRef::new(
            offer(self.source, self.offer_id),
            ObservationId::new(self.id).expect("id"),
        )
    }
}

pub fn supported_review(at: SystemTime, id: u64) -> SampleReview {
    SampleReview {
        knowledge: KnowledgeAssessment::Supported,
        approval: Some("synthetic:reviewer".to_owned()),
        assessor: "synthetic:assessor".to_owned(),
        rationale: format!("synthetic:rationale:{id}"),
        assessed: at,
        recorded: at,
    }
}

pub fn manual_evidence(reference: &str) -> EvidenceSupport {
    let covered = ClaimCoverageState::Covered;
    EvidenceSupport::new(
        Some(EvidenceReference::new(reference).expect("reference")),
        EvidenceMethod::Manual,
        ClaimCoverage::new(covered, covered, covered),
    )
}

pub fn create_store(dir: &TempDir, name: &str) -> (PathBuf, Store) {
    let path = dir.child(name);
    let store = Store::create(&path).expect("create store");
    (path, store)
}

pub fn create_ledger(dir: &TempDir, name: &str) -> (PathBuf, ResearchStore) {
    let path = dir.child(name);
    let store = ResearchStore::create(&path).expect("create ledger");
    (path, store)
}

pub fn append(store: &mut Store, sample: &Sample) -> Result<(), StoreError> {
    let record = record(sample)?;
    let review = sample.review.as_ref().map(review_draft).transpose()?;
    store.append(&record, review.as_ref())
}

pub fn record(sample: &Sample) -> Result<ObservationRecord, StoreError> {
    let observation = Observation::new(
        sample.reference(),
        sample.subject,
        sample.language,
        sample.observed.clone(),
        sample.access,
        sample.listing,
        sample.availability,
        sample
            .statement
            .as_deref()
            .map(|text| SourceStatement::new(text).expect("statement")),
        sample.evidence.clone(),
    );
    ObservationRecord::try_new(
        observation,
        sample.ingested,
        sample.captured.clone(),
        sample.requested_url.clone(),
        sample.resolved_url.clone(),
        sample.redirect.clone(),
        sample.provenance.clone(),
        sample.uncertainty.clone(),
    )
}

fn review_draft(review: &SampleReview) -> Result<ReviewDraft, StoreError> {
    ReviewDraft::try_new(
        review.knowledge,
        review
            .approval
            .as_deref()
            .map(|text| ReviewerApproval::new(text).expect("approval")),
        review.assessor.clone(),
        review.rationale.clone(),
        review.assessed,
        review.recorded,
    )
}

pub fn config_at(store: &Path, ledger: &Path, now: SystemTime, now_seconds: i64) -> Config {
    Config {
        store: store.to_path_buf(),
        research_ledger: ledger.to_path_buf(),
        now,
        now_seconds,
        bind: "127.0.0.1:0".parse().expect("bind"),
        producer: "synthetic:producer".to_owned(),
        source_ref: "synthetic:source".to_owned(),
        listing_ref: "OFFER-A".to_owned(),
        listing_generation: "generation-1".to_owned(),
        target: EditionId::new(10).expect("edition"),
    }
}

pub fn load_at(store: &Path, ledger: &Path, now: SystemTime, now_seconds: i64) -> Dashboard {
    load(&config_at(store, ledger, now, now_seconds))
}

pub fn research_fixture(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../bookradar-store/tests/fixtures/research")
        .join(name);
    fs::read(path).unwrap_or_else(|error| panic!("fixture {name}: {error}"))
}

pub fn import_bytes(store: &mut ResearchStore, bytes: &[u8], seconds: i64) {
    store
        .import(bytes, ImportContext::new(unix(seconds)))
        .unwrap_or_else(|error| panic!("import: {error}"));
}

pub fn section<'a>(html: &'a str, id: &str) -> &'a str {
    let marker = format!("id=\"{id}\"");
    let start = html.find(&marker).unwrap_or_else(|| panic!("missing {id}"));
    let rest = &html[start..];
    let end = rest
        .find("</section>")
        .unwrap_or_else(|| panic!("unclosed {id}"));
    &rest[..end]
}

pub fn run_ui(args: &[String]) -> (Option<i32>, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_bookradar-ui"))
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    let mut stdout = child.stdout.take().expect("stdout");
    let mut stderr = child.stderr.take().expect("stderr");
    let mut guard = ChildGuard::new(child);
    let status = guard.wait_timeout(Duration::from_secs(3));
    if status.is_none() {
        guard.stop();
    }
    let mut out = String::new();
    let mut err = String::new();
    let _ = stdout.read_to_string(&mut out);
    let _ = stderr.read_to_string(&mut err);
    let code = status.and_then(|status| status.code());
    (code, out, err)
}

pub fn start_ui(args: &[String]) -> RunningUi {
    let mut child = Command::new(env!("CARGO_BIN_EXE_bookradar-ui"))
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    let stdout = child.stdout.take().expect("stdout");
    let stderr = child.stderr.take().expect("stderr");
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        let _ = reader.read_line(&mut line);
        let _ = tx.send(line);
    });
    let (err_tx, err_rx) = mpsc::channel();
    thread::spawn(move || {
        let mut reader = BufReader::new(stderr);
        let mut text = String::new();
        let _ = reader.read_to_string(&mut text);
        let _ = err_tx.send(text);
    });
    let line = rx.recv_timeout(Duration::from_secs(10)).unwrap_or_default();
    let port = line
        .trim()
        .strip_prefix("listening=http://127.0.0.1:")
        .and_then(|text| text.parse::<u16>().ok())
        .unwrap_or_else(|| {
            let _ = child.kill();
            let err = err_rx
                .recv_timeout(Duration::from_secs(1))
                .unwrap_or_default();
            panic!("listening line {line:?}; stderr {err}");
        });
    RunningUi {
        guard: ChildGuard::new(child),
        port,
    }
}

pub fn exchange(port: u16, request: &str) -> Vec<u8> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("read timeout");
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .expect("write timeout");
    stream.write_all(request.as_bytes()).expect("write");
    let mut body = Vec::new();
    let mut tmp = [0u8; 4096];
    loop {
        match stream.read(&mut tmp) {
            Ok(0) => break,
            Ok(count) => body.extend_from_slice(&tmp[..count]),
            Err(error) if error.kind() == std::io::ErrorKind::TimedOut => break,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(error) => panic!("read: {error}"),
        }
    }
    body
}

pub fn status_code(response: &[u8]) -> u16 {
    let text = String::from_utf8_lossy(response);
    let line = text.lines().next().unwrap_or("");
    let mut parts = line.split_whitespace();
    assert_eq!(parts.next(), Some("HTTP/1.1"), "{line}");
    parts
        .next()
        .unwrap_or("0")
        .parse()
        .unwrap_or_else(|_| panic!("status {line}"))
}

pub fn header_value<'a>(response: &'a [u8], name: &str) -> &'a str {
    let text = std::str::from_utf8(response).expect("utf8 response");
    let headers = text.split("\r\n\r\n").next().expect("headers");
    headers
        .split("\r\n")
        .skip(1)
        .find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case(name).then(|| value.trim())
        })
        .unwrap_or_else(|| panic!("missing header {name}"))
}

pub fn body_bytes(response: &[u8]) -> &[u8] {
    let marker = b"\r\n\r\n";
    let end = response
        .windows(4)
        .position(|window| window == marker)
        .expect("header end");
    &response[end + 4..]
}

pub fn request(port: u16, method: &str, target: &str, extra: &str) -> String {
    format!("{method} {target} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{extra}\r\n")
}

pub fn patch_be_u32(path: &Path, offset: usize, value: u32) {
    let mut bytes = fs::read(path).expect("read");
    bytes[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
    fs::write(path, bytes).expect("write");
}
