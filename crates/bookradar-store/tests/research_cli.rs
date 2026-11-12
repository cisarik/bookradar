//! Process-level proof of the research ingestion commands.

#![forbid(unsafe_code)]

use std::process::Command;

#[test]
fn research_cli_transcript_and_exits_are_stable() {
    let dir = std::env::temp_dir().join(format!("bookradar-research-cli-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir(&dir).expect("temp");
    let ledger = dir.join("demo.sqlite");
    let ledger = ledger.to_str().expect("utf8");
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let partial = root.join("tests/fixtures/research/partial.json");
    let conflict = root.join("tests/fixtures/research/conflict.json");
    let partial = partial.to_str().expect("utf8");
    let conflict = conflict.to_str().expect("utf8");

    let init = run(&["init", ledger]);
    assert_eq!(init.status, 0);
    assert_eq!(init.stdout, "created research_schema=1\n");
    assert!(init.stderr.is_empty());

    let first = run(&["import", ledger, partial, "1793545201"]);
    assert_eq!(first.status, 3);
    assert_eq!(
        first.stdout,
        "run=partial seen=3 admitted=1 replayed=0 rejected=1 quarantined=1 conflicts=0 projected=1\n"
    );
    let replay = run(&["import", ledger, partial, "1793545202"]);
    assert_eq!(replay.status, 3);
    assert_eq!(
        replay.stdout,
        "run=partial seen=3 admitted=0 replayed=1 rejected=1 quarantined=1 conflicts=0 projected=0\n"
    );
    let current = run(&[
        "current",
        ledger,
        "synthetic:producer",
        "synthetic:source",
        "OFFER-A",
        "generation-1",
        "10",
        "1793548800",
    ]);
    assert_eq!(current.status, 0);
    assert_eq!(
        current.stdout,
        "candidates=1 conflict=clear recheck=clear verification=not-eligible\n"
    );
    let changed = run(&["import", ledger, conflict, "1793545203"]);
    assert_eq!(changed.status, 3);
    assert_eq!(
        changed.stdout,
        "run=quarantined seen=1 admitted=0 replayed=0 rejected=0 quarantined=1 conflicts=1 projected=0\n"
    );
    let after = run(&[
        "current",
        ledger,
        "synthetic:producer",
        "synthetic:source",
        "OFFER-A",
        "generation-1",
        "10",
        "1793548800",
    ]);
    assert_eq!(after.status, 0);
    assert_eq!(
        after.stdout,
        "candidates=1 conflict=unresolved recheck=clear verification=not-eligible\n"
    );
    let inspect = run(&["inspect", ledger]);
    assert_eq!(inspect.status, 0);
    assert_eq!(
        inspect.stdout,
        "envelopes=2 runs=1 findings=4 observations=1 reviews=2\n\
         input=1 finding=0 outcome=admitted reason=none knowledge=disputed\n\
         input=1 finding=1 outcome=rejected reason=missing-finding-id\n\
         input=1 finding=2 outcome=quarantined reason=invalid-availability\n\
         input=2 finding=0 outcome=quarantined reason=identity-conflict\n"
    );

    let empty_path = dir.join("empty.sqlite");
    let empty_path = empty_path.to_str().expect("utf8");
    assert_eq!(run(&["init", empty_path]).status, 0);
    let empty = run(&["inspect", empty_path]);
    assert_eq!(empty.status, 0);
    assert_eq!(
        empty.stdout,
        "envelopes=0 runs=0 findings=0 observations=0 reviews=0\n"
    );

    let malformed = dir.join("malformed.json");
    std::fs::write(&malformed, b"{").expect("malformed");
    let malformed = run(&[
        "import",
        empty_path,
        malformed.to_str().expect("utf8"),
        "1793545204",
    ]);
    assert_eq!(malformed.status, 3);
    assert_eq!(
        malformed.stdout,
        "run=quarantined seen=unknown admitted=0 replayed=0 rejected=0 quarantined=0 conflicts=0 projected=0\n"
    );
    assert!(malformed.stderr.is_empty());

    assert_eq!(run(&[]).status, 2);
    assert!(run(&[]).stderr.starts_with("usage:"));
    assert_eq!(run(&["import", empty_path]).status, 2);
    assert_eq!(run(&["current", empty_path, "soon"]).status, 2);
    let missing = run(&[
        "inspect",
        dir.join("missing.sqlite").to_str().expect("utf8"),
    ]);
    assert_eq!(missing.status, 1);
    assert!(missing.stderr.starts_with("not-found:"));
    let _ = std::fs::remove_dir_all(&dir);
}

struct Output {
    status: i32,
    stdout: String,
    stderr: String,
}

fn run(args: &[&str]) -> Output {
    let output = Command::new(env!("CARGO_BIN_EXE_bookradar-research"))
        .args(args)
        .output()
        .expect("spawn");
    Output {
        status: output.status.code().unwrap_or(1),
        stdout: String::from_utf8(output.stdout).expect("stdout"),
        stderr: String::from_utf8(output.stderr).expect("stderr"),
    }
}
