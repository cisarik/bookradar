//! Process-level proof of the four synthetic commands.

#![forbid(unsafe_code)]

mod common;

use std::process::Command;

use common::TempDir;

#[test]
fn positive_transcript_and_negative_fixture_match_the_accepted_plan() {
    let dir = TempDir::new();
    let path = dir.child("bookradar-poc.sqlite");
    let path = path.to_str().expect("utf8");
    let mut transcript = String::new();
    for args in [
        vec!["init", path],
        vec!["append-fixture", path, "available"],
        vec!["current", path, "1793548800"],
        vec!["append-fixture", path, "blocked"],
        vec!["current", path, "1793548800"],
        vec!["append-fixture", path, "recovered"],
        vec!["append-fixture", path, "older"],
        vec!["current", path, "1793548800"],
        vec!["history", path],
    ] {
        let output = run(&args);
        assert_eq!(output.status, 0, "{args:?}\n{}", output.stderr);
        transcript.push_str(&output.stdout);
    }
    assert_eq!(transcript, POSITIVE);

    let negative = dir.child("bookradar-negative.sqlite");
    let negative = negative.to_str().expect("utf8");
    let mut negative_transcript = String::new();
    for args in [
        vec!["init", negative],
        vec!["append-fixture", negative, "older"],
        vec!["current", negative, "1793548800"],
    ] {
        let output = run(&args);
        assert_eq!(output.status, 0, "{}", output.stderr);
        negative_transcript.push_str(&output.stdout);
    }
    assert_eq!(negative_transcript, NEGATIVE);
    let repeated = run(&["current", negative, "1793548800"]);
    assert_eq!(repeated.status, 0);
    assert_eq!(
        repeated.stdout,
        "synthetic=true latest_attempt=4 access=readable listing=present availability=unavailable\n\
         candidate=4 temporal=within-interval conflict=clear recheck=clear verification=not-eligible\n\
         Last observed unavailable -- unix:1793520000\n"
    );
}

#[test]
fn usage_and_storage_failures_use_stable_exits() {
    let dir = TempDir::new();
    let path = dir.child("store.sqlite");
    let path = path.to_str().expect("utf8");
    assert_eq!(run(&[]).status, 2);
    assert_eq!(run(&["import", path]).status, 2);
    assert_eq!(run(&["append-fixture", path, "missing"]).status, 2);
    assert_eq!(run(&["current", path, "soon"]).status, 2);
    let missing = run(&["current", path, "1793548800"]);
    assert_eq!(missing.status, 1);
    assert!(missing.stderr.starts_with("not-found:"));
    assert!(!dir.child("store.sqlite").exists());

    assert_eq!(run(&["init", path]).status, 0);
    let exists = run(&["init", path]);
    assert_eq!(exists.status, 1);
    assert!(exists.stderr.starts_with("already-exists:"));
    let duplicate = run(&["append-fixture", path, "available"]);
    assert_eq!(duplicate.status, 0);
    let before = run(&["current", path, "1793548800"]);
    let again = run(&["append-fixture", path, "available"]);
    assert_eq!(again.status, 1);
    assert!(again.stderr.starts_with("duplicate-key:"));
    let after = run(&["current", path, "1793548800"]);
    assert_eq!(after.stdout, before.stdout);
    assert!(after.stdout.contains("candidate=1"));
}

#[test]
fn empty_history_exits_zero_with_count_zero() {
    let dir = TempDir::new();
    let path = dir.child("store.sqlite");
    let path = path.to_str().expect("utf8");
    let init = run(&["init", path]);
    assert_eq!(init.status, 0, "{}", init.stderr);
    let history = run(&["history", path]);
    assert_eq!(history.status, 0, "{}", history.stderr);
    assert_eq!(history.stdout, "synthetic=true offer=1/1 count=0\n");
    assert!(history.stderr.is_empty());
}

#[test]
fn empty_current_exits_zero_with_status_empty() {
    let dir = TempDir::new();
    let path = dir.child("store.sqlite");
    let path = path.to_str().expect("utf8");
    let init = run(&["init", path]);
    assert_eq!(init.status, 0, "{}", init.stderr);
    let current = run(&["current", path, "1793548800"]);
    assert_eq!(current.status, 0, "{}", current.stderr);
    assert_eq!(
        current.stdout,
        "synthetic=true offer=1/1 target=10 status=empty\n"
    );
    assert!(current.stderr.is_empty());
}

struct Output {
    status: i32,
    stdout: String,
    stderr: String,
}

fn run(args: &[&str]) -> Output {
    let output = Command::new(env!("CARGO_BIN_EXE_bookradar-poc"))
        .args(args)
        .output()
        .expect("spawn");
    Output {
        status: output.status.code().unwrap_or(1),
        stdout: String::from_utf8(output.stdout).expect("stdout"),
        stderr: String::from_utf8(output.stderr).expect("stderr"),
    }
}

const POSITIVE: &str = "\
created schema=1 synthetic=true
appended observation=1 synthetic=true
synthetic=true latest_attempt=1 access=readable listing=present availability=available
candidate=1 temporal=within-interval conflict=clear recheck=clear verification=eligible
Checked available -- unix:1793523600; expires_unix=1793610000
appended observation=2 synthetic=true
synthetic=true latest_attempt=2 access=rate-limited listing=uncertain availability=unknown
candidate=1 temporal=within-interval conflict=clear recheck=failed verification=not-eligible
Could not recheck -- unix:1793527200
Last observed available -- unix:1793523600
appended observation=3 synthetic=true
appended observation=4 synthetic=true
synthetic=true latest_attempt=3 access=readable listing=present availability=available
candidate=3 temporal=within-interval conflict=clear recheck=clear verification=eligible
Checked available -- unix:1793534400; expires_unix=1793620800
synthetic=true offer=1/1 count=4
4 observed=1793520000 ingested=1793545204 access=readable listing=present availability=unavailable review=supported
1 observed=1793523600 ingested=1793545201 access=readable listing=present availability=available review=supported
2 observed=1793527200 ingested=1793545202 access=rate-limited listing=uncertain availability=unknown review=supported
3 observed=1793534400 ingested=1793545203 access=readable listing=present availability=available review=supported
";

const NEGATIVE: &str = "\
created schema=1 synthetic=true
appended observation=4 synthetic=true
synthetic=true latest_attempt=4 access=readable listing=present availability=unavailable
candidate=4 temporal=within-interval conflict=clear recheck=clear verification=not-eligible
Last observed unavailable -- unix:1793520000
";
