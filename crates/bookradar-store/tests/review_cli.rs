//! Process-level proof of show, promote, and review.

#![forbid(unsafe_code)]

mod common;

use std::ffi::OsStr;
use std::fs;
use std::process::Command;

use bookradar_store::{ImportContext, ResearchStore, Store};

use common::{Sample, TempDir, append, unix};

const OBSERVED: i64 = 1_793_523_600;

#[test]
fn show_is_read_only_and_prints_identity() {
    let dir = TempDir::new();
    let ledger = dir.child("ledger.sqlite");
    let mut store = ResearchStore::create(&ledger).expect("create");
    store
        .import(&fixture("newer.json"), ImportContext::new(unix(100)))
        .expect("import");
    drop(store);
    let before = fs::read(&ledger).expect("bytes");
    let ledger_text = ledger.to_str().expect("utf8");
    let shown = run([
        "show",
        "--ledger",
        ledger_text,
        "--producer",
        "synthetic:producer",
        "--run",
        "synthetic:run",
        "--finding",
        "synthetic:newer",
        "--receipt",
        "1",
        "--position",
        "0",
    ]);
    assert_eq!(shown.status, 0, "{}", shown.stderr);
    assert!(shown.stdout.contains("producer=synthetic:producer\n"));
    assert!(shown.stdout.contains("finding=synthetic:newer\n"));
    assert!(shown.stdout.contains("receipt=1\n"));
    assert!(shown.stdout.contains("position=0\n"));
    assert!(shown.stdout.contains("outcome=admitted\n"));
    assert!(shown.stdout.contains("ledger_knowledge=unreviewed\n"));
    assert!(shown.stderr.is_empty());
    assert_eq!(fs::read(&ledger).expect("after"), before);

    let store_path = dir.child("store.sqlite");
    drop(Store::create(&store_path).expect("store"));
    let promoted = run(promote_args(
        ledger_text,
        store_path.to_str().expect("utf8"),
        "1",
        "manual",
    ));
    assert_eq!(promoted.status, 0, "{}", promoted.stderr);
    assert!(
        promoted
            .stdout
            .contains("promoted source=1 offer=1 observation=1 ")
    );
    assert_eq!(fs::read(&ledger).expect("ledger after promote"), before);
}

#[test]
fn promote_usage_exits_2_before_io() {
    let dir = TempDir::new();
    let ledger = dir.child("missing-ledger.sqlite");
    let store = dir.child("missing-store.sqlite");
    let ledger_text = ledger.to_str().expect("utf8");
    let store_text = store.to_str().expect("utf8");

    let agent = run(promote_args(ledger_text, store_text, "1", "agent-only"));
    assert_eq!(agent.status, 2, "{}", agent.stderr);
    assert!(agent.stderr.starts_with("usage:"));
    assert!(!ledger.exists());
    assert!(!store.exists());

    let mut zero = promote_args(ledger_text, store_text, "1", "manual");
    replace_flag(&mut zero, "--source-id", "0");
    let zero = run(zero);
    assert_eq!(zero.status, 2, "{}", zero.stderr);
    assert!(!ledger.exists());

    let mut duplicate = promote_args(ledger_text, store_text, "1", "manual");
    duplicate.extend(["--ledger".to_owned(), ledger_text.to_owned()]);
    let duplicate = run(duplicate);
    assert_eq!(duplicate.status, 2, "{}", duplicate.stderr);
    assert!(!store.exists());

    assert_eq!(run(Vec::<String>::new()).status, 2);
    assert!(run(["promote"]).stderr.starts_with("usage:"));
}

#[test]
fn refused_promote_exits_1_and_does_not_create_store() {
    let dir = TempDir::new();
    let ledger = dir.child("ledger.sqlite");
    let mut research = ResearchStore::create(&ledger).expect("create");
    research
        .import(&fixture("partial.json"), ImportContext::new(unix(100)))
        .expect("import");
    drop(research);
    let before = fs::read(&ledger).expect("bytes");
    let store = dir.child("absent.sqlite");
    let mut args = promote_args(
        ledger.to_str().expect("utf8"),
        store.to_str().expect("utf8"),
        "1",
        "manual",
    );
    replace_flag(&mut args, "--finding", "F3");
    replace_flag(&mut args, "--position", "2");
    let refused = run(args.iter().map(String::as_str));
    assert_eq!(refused.status, 1, "{}", refused.stderr);
    assert!(refused.stderr.starts_with("not-admitted:"));
    assert!(!store.exists());
    assert_eq!(fs::read(&ledger).expect("after"), before);

    replace_flag(&mut args, "--finding", "missing");
    replace_flag(&mut args, "--receipt", "9");
    let unknown = run(args.iter().map(String::as_str));
    assert_eq!(unknown.status, 1, "{}", unknown.stderr);
    assert!(unknown.stderr.starts_with("unknown-finding:"));
    assert!(!store.exists());
}

#[test]
fn review_conflict_and_regressed_time_exit_1() {
    let dir = TempDir::new();
    let path = dir.child("store.sqlite");
    let mut store = Store::create(&path).expect("create");
    append(&mut store, &Sample::qualifying(1, OBSERVED)).expect("append");
    drop(store);
    let path = path.to_str().expect("utf8");
    let conflict = run(review_args(path, "0", &(OBSERVED + 30).to_string()));
    assert_eq!(conflict.status, 1, "{}", conflict.stderr);
    assert!(conflict.stderr.starts_with("review-conflict:"));
    let regressed = run(review_args(path, "1", &(OBSERVED + 9).to_string()));
    assert_eq!(regressed.status, 1, "{}", regressed.stderr);
    assert!(regressed.stderr.starts_with("storage:"));
    assert!(regressed.stderr.contains("assessment-time-regressed"));
}

struct Output {
    status: i32,
    stdout: String,
    stderr: String,
}

fn run<I, S>(args: I) -> Output
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = Command::new(env!("CARGO_BIN_EXE_bookradar-review"))
        .args(args)
        .output()
        .expect("spawn");
    Output {
        status: output.status.code().unwrap_or(1),
        stdout: String::from_utf8(output.stdout).expect("stdout"),
        stderr: String::from_utf8(output.stderr).expect("stderr"),
    }
}

fn promote_args(ledger: &str, store: &str, source_id: &str, method: &str) -> Vec<String> {
    let observed = OBSERVED.to_string();
    let later = (OBSERVED + 10).to_string();
    [
        "promote",
        "--ledger",
        ledger,
        "--store",
        store,
        "--producer",
        "synthetic:producer",
        "--run",
        "synthetic:run",
        "--finding",
        "synthetic:newer",
        "--receipt",
        "1",
        "--position",
        "0",
        "--source-id",
        source_id,
        "--offer-id",
        "1",
        "--observation-id",
        "1",
        "--edition-id",
        "10",
        "--language",
        "slovak",
        "--access",
        "readable",
        "--listing",
        "present",
        "--availability",
        "available",
        "--source-statement",
        "reviewer:statement",
        "--evidence-method",
        method,
        "--evidence-reference",
        "reviewer:evidence",
        "--cover-subject",
        "--cover-language",
        "--cover-availability",
        "--reviewer-approval",
        "reviewer:approval",
        "--assessor",
        "reviewer:assessor",
        "--rationale",
        "reviewer:rationale",
        "--observed-at",
        observed.as_str(),
        "--observed-offset",
        "3600",
        "--ingested-at",
        later.as_str(),
        "--assessed-at",
        later.as_str(),
        "--recorded-at",
        later.as_str(),
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn review_args(store: &str, expected: &str, assessed: &str) -> Vec<String> {
    [
        "review",
        "--store",
        store,
        "--source-id",
        "1",
        "--offer-id",
        "1",
        "--observation-id",
        "1",
        "--expected-revision",
        expected,
        "--knowledge",
        "rejected",
        "--assessor",
        "reviewer:assessor",
        "--rationale",
        "reviewer:later",
        "--assessed-at",
        assessed,
        "--recorded-at",
        assessed,
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn replace_flag(args: &mut [String], flag: &str, value: &str) {
    let index = args.iter().position(|item| item == flag).expect(flag);
    args[index + 1] = value.to_owned();
}

fn fixture(name: &str) -> Vec<u8> {
    fs::read(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/research")
            .join(name),
    )
    .unwrap_or_else(|error| panic!("fixture {name}: {error}"))
}
