//! Operator argument and bind failures exit before a listener starts.

#![forbid(unsafe_code)]

mod common;

use std::fs;
use std::net::TcpListener;

use common::{TempDir, run_ui};

#[test]
fn invalid_operator_arguments_exit_before_database_or_socket_io() {
    let dir = TempDir::new();
    let missing = dir.child("should-not-exist.sqlite");
    let missing_text = missing.display().to_string();
    let cases = [
        vec![
            "--store".to_owned(),
            missing_text.clone(),
            "--research-ledger".to_owned(),
            missing_text.clone(),
        ],
        vec![
            "--store".to_owned(),
            missing_text.clone(),
            "--research-ledger".to_owned(),
            missing_text.clone(),
            "--now".to_owned(),
            "nope".to_owned(),
        ],
        vec![
            "--store".to_owned(),
            missing_text.clone(),
            "--research-ledger".to_owned(),
            missing_text.clone(),
            "--now".to_owned(),
            "+1".to_owned(),
        ],
        vec![
            "--store".to_owned(),
            missing_text.clone(),
            "--research-ledger".to_owned(),
            missing_text.clone(),
            "--now".to_owned(),
            "9223372036854775808".to_owned(),
        ],
        vec![
            "--store".to_owned(),
            missing_text.clone(),
            "--research-ledger".to_owned(),
            missing_text.clone(),
            "--now".to_owned(),
            "-9223372036854775808".to_owned(),
        ],
        vec![
            "--store".to_owned(),
            missing_text.clone(),
            "--research-ledger".to_owned(),
            missing_text.clone(),
            "--now".to_owned(),
            "1".to_owned(),
            "--target-edition".to_owned(),
            "0".to_owned(),
        ],
        vec![
            "--store".to_owned(),
            missing_text.clone(),
            "--research-ledger".to_owned(),
            missing_text.clone(),
            "--now".to_owned(),
            "1".to_owned(),
            "--target-edition".to_owned(),
            "-1".to_owned(),
        ],
        vec![
            "--store".to_owned(),
            missing_text.clone(),
            "--research-ledger".to_owned(),
            missing_text.clone(),
            "--now".to_owned(),
            "1".to_owned(),
            "--producer".to_owned(),
            "bad id".to_owned(),
        ],
        vec![
            "--store".to_owned(),
            missing_text.clone(),
            "--research-ledger".to_owned(),
            missing_text.clone(),
            "--now".to_owned(),
            "1".to_owned(),
            "--now".to_owned(),
            "2".to_owned(),
        ],
        vec![
            "--store".to_owned(),
            missing_text.clone(),
            "--research-ledger".to_owned(),
            missing_text.clone(),
            "--now".to_owned(),
            "1".to_owned(),
            "--unknown".to_owned(),
            "1".to_owned(),
        ],
    ];
    for args in cases {
        let (code, stdout, stderr) = run_ui(&args);
        assert_eq!(code, Some(2), "{args:?}\n{stderr}");
        assert!(stderr.starts_with("usage:"), "{stderr}");
        assert!(!stdout.contains("listening="), "{stdout}");
        assert!(!missing.exists());
    }
    let (code, stdout, stderr) = run_ui(&["--help".to_owned()]);
    assert_eq!(code, Some(0), "{stderr}");
    assert!(stderr.is_empty(), "{stderr}");
    assert!(stdout.contains("--now"));
    assert!(!stdout.contains("listening="));
    assert!(fs::read_dir(dir.path()).unwrap().next().is_none());
}

#[test]
fn non_loopback_binds_are_rejected() {
    let dir = TempDir::new();
    let missing = dir.child("bind-should-not-exist.sqlite");
    let missing_text = missing.display().to_string();
    for bind in [
        "0.0.0.0:9",
        "8.8.8.8:9",
        "10.1.2.3:9",
        "127.0.0.2:9",
        "localhost:9",
        "[::1]:9",
        "example.test:9",
        "127.0.0.1:65536",
        "127.0.0.1",
        "127.0.0.1:",
    ] {
        let args = vec![
            "--store".to_owned(),
            missing_text.clone(),
            "--research-ledger".to_owned(),
            missing_text.clone(),
            "--now".to_owned(),
            "1".to_owned(),
            "--bind".to_owned(),
            bind.to_owned(),
        ];
        let (code, stdout, stderr) = run_ui(&args);
        assert_eq!(code, Some(2), "{bind}\n{stderr}");
        assert!(stderr.starts_with("usage:"), "{bind} {stderr}");
        assert!(!stdout.contains("listening="), "{bind}");
        assert!(!missing.exists(), "{bind}");
    }
}

#[test]
fn bind_failure_exits_without_address_fallback() {
    let dir = TempDir::new();
    let listener = TcpListener::bind("127.0.0.1:0").expect("reserve");
    let port = listener.local_addr().expect("addr").port();
    let missing = dir.child("not-created.sqlite");
    let missing_text = missing.display().to_string();
    let args = vec![
        "--store".to_owned(),
        missing_text.clone(),
        "--research-ledger".to_owned(),
        missing_text,
        "--now".to_owned(),
        "1".to_owned(),
        "--bind".to_owned(),
        format!("127.0.0.1:{port}"),
    ];
    let (code, stdout, stderr) = run_ui(&args);
    assert_eq!(code, Some(1), "{stdout}\n{stderr}");
    assert!(stderr.starts_with("bind-failure:"), "{stderr}");
    assert!(!stdout.contains("listening="), "{stdout}");
    assert!(!missing.exists());
    assert!(listener.local_addr().is_ok());
}
