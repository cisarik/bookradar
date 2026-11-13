//! Loopback HTTP behavior of the bookradar-ui binary.

#![forbid(unsafe_code)]

mod common;

use std::fs;
use std::time::{Duration, Instant};

use bookradar_store::{Fixture, append_fixture};
use bookradar_ui::STYLESHEET;

use common::{
    T0, TempDir, body_bytes, create_ledger, create_store, exchange, header_value, request,
    start_ui, status_code, unix,
};

#[test]
fn serves_loopback_dashboard_and_embedded_stylesheet() {
    let dir = TempDir::new();
    let (store, ledger) = populated(&dir);
    let server = start_ui(&ui_args(&store, &ledger, None));
    assert_ne!(server.port, 0);
    let page = exchange(server.port, &request(server.port, "GET", "/", ""));
    let css = exchange(server.port, &request(server.port, "GET", "/style.css", ""));
    assert_eq!(status_code(&page), 200);
    assert_eq!(
        header_value(&page, "Content-Type"),
        "text/html; charset=utf-8"
    );
    assert!(
        body_bytes(&page).windows(4).any(|bytes| bytes == b"<h1>")
            || std::str::from_utf8(body_bytes(&page))
                .unwrap()
                .contains("<h1>BookRadar</h1>")
    );
    assert_eq!(status_code(&css), 200);
    assert_eq!(
        header_value(&css, "Content-Type"),
        "text/css; charset=utf-8"
    );
    assert_eq!(body_bytes(&css), STYLESHEET.as_bytes());
    let css_text = std::str::from_utf8(body_bytes(&css)).unwrap();
    assert!(!css_text.contains("http://"));
    assert!(!css_text.contains("https://"));
    assert!(!css_text.contains("url("));
    let page_text = std::str::from_utf8(body_bytes(&page)).unwrap();
    assert!(page_text.contains("href=\"/style.css\""));
    assert!(!page_text.contains("http://"));
    assert!(!page_text.contains("https://"));
}

#[test]
fn head_returns_get_headers_without_bodies() {
    let dir = TempDir::new();
    let (store, ledger) = populated(&dir);
    let server = start_ui(&ui_args(&store, &ledger, None));
    for target in ["/", "/style.css"] {
        let get = exchange(server.port, &request(server.port, "GET", target, ""));
        let head = exchange(server.port, &request(server.port, "HEAD", target, ""));
        assert_eq!(status_code(&get), 200);
        assert_eq!(status_code(&head), status_code(&get));
        assert_eq!(
            header_value(&head, "Content-Type"),
            header_value(&get, "Content-Type")
        );
        assert_eq!(
            header_value(&head, "Content-Length"),
            header_value(&get, "Content-Length")
        );
        assert_eq!(
            header_value(&get, "Content-Length")
                .parse::<usize>()
                .unwrap(),
            body_bytes(&get).len()
        );
        assert!(body_bytes(&head).is_empty());
        assert_security(&get);
        assert_security(&head);
    }
}

#[test]
fn rejects_mutating_methods_and_non_allowlisted_targets() {
    let dir = TempDir::new();
    let (store, ledger) = populated(&dir);
    let before = fs::read(&store).expect("before");
    let server = start_ui(&ui_args(&store, &ledger, None));
    for method in ["POST", "PUT", "DELETE", "OPTIONS"] {
        let response = exchange(server.port, &request(server.port, method, "/", ""));
        assert_eq!(status_code(&response), 405, "{method}");
        assert_eq!(header_value(&response, "Allow"), "GET, HEAD");
        assert_security(&response);
    }
    let unknown = exchange(server.port, &request(server.port, "GET", "/other", ""));
    assert_eq!(status_code(&unknown), 404);
    for target in [
        "/../secret",
        "/%2e%2e/secret",
        "/?x=1",
        "http://127.0.0.1/secret",
    ] {
        let response = exchange(server.port, &request(server.port, "GET", target, ""));
        assert_eq!(status_code(&response), 400, "{target}");
    }
    assert_eq!(fs::read(&store).expect("after"), before);
}

#[test]
fn enforces_host_origin_and_response_headers() {
    let dir = TempDir::new();
    let (store, ledger) = populated(&dir);
    let server = start_ui(&ui_args(&store, &ledger, None));
    let port = server.port;
    let ok = exchange(port, &request(port, "GET", "/", ""));
    assert_eq!(status_code(&ok), 200);
    assert_security(&ok);
    assert!(header_value_opt(&ok, "Access-Control-Allow-Origin").is_none());

    let missing = exchange(port, "GET / HTTP/1.1\r\n\r\n");
    assert_eq!(status_code(&missing), 400);
    let duplicate = exchange(
        port,
        &format!("GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nHost: 127.0.0.1:{port}\r\n\r\n"),
    );
    assert_eq!(status_code(&duplicate), 400);

    let foreign_host = exchange(port, "GET / HTTP/1.1\r\nHost: example.test\r\n\r\n");
    assert_eq!(status_code(&foreign_host), 403);
    let foreign_origin = exchange(
        port,
        &format!("GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: http://evil.example\r\n\r\n"),
    );
    assert_eq!(status_code(&foreign_origin), 403);
    let cross_site = exchange(
        port,
        &format!("GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nSec-Fetch-Site: cross-site\r\n\r\n"),
    );
    assert_eq!(status_code(&cross_site), 403);
    for response in [
        &missing,
        &duplicate,
        &foreign_host,
        &foreign_origin,
        &cross_site,
    ] {
        assert_security(response);
        assert!(header_value_opt(response, "Access-Control-Allow-Origin").is_none());
    }
    let same_origin = exchange(
        port,
        &format!(
            "GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: http://127.0.0.1:{port}\r\nSec-Fetch-Site: same-origin\r\n\r\n"
        ),
    );
    assert_eq!(status_code(&same_origin), 200);
}

#[test]
fn bounds_requests_and_rejects_ambiguous_framing() {
    let dir = TempDir::new();
    let (store, ledger) = populated(&dir);
    let server = start_ui(&ui_args(&store, &ledger, None));
    let port = server.port;
    let many = format!(
        "GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{}Connection: close\r\n\r\n",
        "X-Extra: 1\r\n".repeat(70)
    );
    assert_eq!(status_code(&exchange(port, &many)), 431);
    let oversized = format!(
        "GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Pad: {}\r\n\r\n",
        "a".repeat(9000)
    );
    assert_eq!(status_code(&exchange(port, &oversized)), 431);
    assert_eq!(status_code(&exchange(port, "NOT A REQUEST\r\n\r\n")), 400);
    assert_eq!(
        status_code(&exchange(
            port,
            &format!("GET / HTTP/1.0\r\nHost: 127.0.0.1:{port}\r\n\r\n")
        )),
        400
    );
    assert_eq!(
        status_code(&exchange(
            port,
            &format!("GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: 5\r\n\r\nhello")
        )),
        400
    );
    assert_eq!(
        status_code(&exchange(
            port,
            &format!(
                "GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nTransfer-Encoding: chunked\r\n\r\n"
            )
        )),
        400
    );
    assert_eq!(
        status_code(&exchange(
            port,
            &format!(
                "GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: 0\r\nContent-Length: 0\r\n\r\n"
            )
        )),
        400
    );

    let started = Instant::now();
    let timed = exchange_timeout(port, "GET / HTTP/1.1\r\nHost: 127.0.0.1:");
    assert_eq!(status_code(&timed), 408);
    assert!(started.elapsed() >= Duration::from_millis(1500));
    assert!(started.elapsed() < Duration::from_secs(5));

    let pipelined = format!(
        "GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\nGET /style.css HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n"
    );
    let once = exchange(port, &pipelined);
    let text = std::str::from_utf8(&once).unwrap();
    assert_eq!(text.matches("HTTP/1.1").count(), 1);
    assert!(text.starts_with("HTTP/1.1 200"));

    let again = exchange(port, &request(port, "GET", "/style.css", ""));
    assert_eq!(status_code(&again), 200);
    assert_eq!(body_bytes(&again), STYLESHEET.as_bytes());
}

#[test]
fn serving_never_changes_databases_or_creates_missing_files() {
    let dir = TempDir::new();
    let (store, ledger) = populated(&dir);
    let store_bytes = fs::read(&store).expect("store");
    let ledger_bytes = fs::read(&ledger).expect("ledger");
    let before = dir_names(&dir);
    let server = start_ui(&ui_args(&store, &ledger, None));
    for _ in 0..3 {
        assert_eq!(
            status_code(&exchange(
                server.port,
                &request(server.port, "GET", "/", "")
            )),
            200
        );
    }
    drop(server);
    assert_eq!(fs::read(&store).expect("store after"), store_bytes);
    assert_eq!(fs::read(&ledger).expect("ledger after"), ledger_bytes);
    assert_eq!(dir_names(&dir), before);

    let missing_store = dir.child("absent-observations.sqlite");
    let missing_ledger = dir.child("absent-research.sqlite");
    let entries_before = dir_names(&dir);
    let missing = start_ui(&ui_args(&missing_store, &missing_ledger, None));
    let page = exchange(missing.port, &request(missing.port, "GET", "/", ""));
    assert_eq!(status_code(&page), 200);
    assert!(
        std::str::from_utf8(body_bytes(&page))
            .unwrap()
            .contains("Database not found (not-found).")
    );
    drop(missing);
    assert!(!missing_store.exists());
    assert!(!missing_ledger.exists());
    assert_eq!(dir_names(&dir), entries_before);
}

fn populated(dir: &TempDir) -> (std::path::PathBuf, std::path::PathBuf) {
    let (store, mut observations) = create_store(dir, "observations.sqlite");
    for fixture in [
        Fixture::Available,
        Fixture::Blocked,
        Fixture::Recovered,
        Fixture::Older,
    ] {
        append_fixture(&mut observations, fixture).expect("fixture");
    }
    drop(observations);
    let (ledger, _research) = create_ledger(dir, "research.sqlite");
    let _ = unix(T0);
    (store, ledger)
}

fn ui_args(store: &std::path::Path, ledger: &std::path::Path, bind: Option<u16>) -> Vec<String> {
    let mut args = vec![
        "--store".to_owned(),
        store.display().to_string(),
        "--research-ledger".to_owned(),
        ledger.display().to_string(),
        "--now".to_owned(),
        "1793548800".to_owned(),
    ];
    if let Some(port) = bind {
        args.push("--bind".to_owned());
        args.push(format!("127.0.0.1:{port}"));
    }
    args
}

fn assert_security(response: &[u8]) {
    assert!(header_value(response, "Content-Security-Policy").contains("default-src 'none'"));
    assert!(header_value(response, "Content-Security-Policy").contains("script-src 'none'"));
    assert!(header_value(response, "Content-Security-Policy").contains("style-src 'self'"));
    assert_eq!(header_value(response, "X-Content-Type-Options"), "nosniff");
    assert_eq!(header_value(response, "Cache-Control"), "no-store");
    assert_eq!(header_value(response, "Referrer-Policy"), "no-referrer");
    assert_eq!(
        header_value(response, "Cross-Origin-Resource-Policy"),
        "same-origin"
    );
    assert_eq!(header_value(response, "Connection"), "close");
    assert!(header_value_opt(response, "Date").is_none());
}

fn header_value_opt<'a>(response: &'a [u8], name: &str) -> Option<&'a str> {
    let text = std::str::from_utf8(response).expect("utf8");
    let headers = text.split("\r\n\r\n").next().unwrap_or("");
    headers.split("\r\n").skip(1).find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.eq_ignore_ascii_case(name).then(|| value.trim())
    })
}

fn exchange_timeout(port: u16, request: &str) -> Vec<u8> {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("timeout");
    stream.write_all(request.as_bytes()).expect("write");
    let mut body = Vec::new();
    let mut tmp = [0u8; 1024];
    loop {
        match stream.read(&mut tmp) {
            Ok(0) => break,
            Ok(count) => body.extend_from_slice(&tmp[..count]),
            Err(_) => break,
        }
    }
    body
}

fn dir_names(dir: &TempDir) -> Vec<String> {
    let mut names = fs::read_dir(dir.path())
        .expect("read dir")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect::<Vec<_>>();
    names.sort();
    names
}
