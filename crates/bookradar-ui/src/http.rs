//! Restricted loopback HTTP/1.1. One request per connection, no filesystem assets.

use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, TcpListener, TcpStream};
use std::time::{Duration, Instant};

pub const STYLESHEET: &str = include_str!("style.css");

const HEADER_LIMIT: usize = 8 * 1024;
const HEADER_FIELD_LIMIT: usize = 64;
const HEADER_DEADLINE: Duration = Duration::from_secs(2);
const WRITE_DEADLINE: Duration = Duration::from_secs(2);

const SECURITY: &[(&str, &str)] = &[
    (
        "Content-Security-Policy",
        "default-src 'none'; script-src 'none'; style-src 'self'; img-src 'none'; font-src 'none'; connect-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'",
    ),
    ("X-Content-Type-Options", "nosniff"),
    ("Cache-Control", "no-store"),
    ("Referrer-Policy", "no-referrer"),
    ("Cross-Origin-Resource-Policy", "same-origin"),
];

pub fn bind_listener(addr: std::net::SocketAddr) -> io::Result<(TcpListener, u16)> {
    let listener = TcpListener::bind(addr)?;
    let local = listener.local_addr()?;
    if local.ip() != IpAddr::V4(Ipv4Addr::LOCALHOST) {
        return Err(io::Error::new(
            io::ErrorKind::AddrNotAvailable,
            "listener is not 127.0.0.1",
        ));
    }
    Ok((listener, local.port()))
}

pub fn serve(listener: TcpListener, port: u16, html: &str) -> io::Result<()> {
    for incoming in listener.incoming() {
        let Ok(mut stream) = incoming else {
            continue;
        };
        let _ = handle_connection(&mut stream, port, html);
    }
    Ok(())
}

fn handle_connection(stream: &mut TcpStream, port: u16, html: &str) -> io::Result<()> {
    let decision = match read_headers(stream) {
        Ok(bytes) => decide(&bytes, port, html),
        Err(ReadFailure::Closed) => return Ok(()),
        Err(ReadFailure::Timeout) => Response::error(408, "Request Timeout", "request timeout"),
        Err(ReadFailure::TooLarge) => {
            Response::error(431, "Request Header Fields Too Large", "headers too large")
        }
        Err(ReadFailure::Malformed) => Response::error(400, "Bad Request", "malformed request"),
    };
    let bytes = decision.encode();
    stream.set_write_timeout(Some(WRITE_DEADLINE))?;
    stream.write_all(&bytes)?;
    let _ = stream.shutdown(std::net::Shutdown::Both);
    Ok(())
}

enum ReadFailure {
    Closed,
    Timeout,
    TooLarge,
    Malformed,
}

fn read_headers(stream: &mut TcpStream) -> Result<Vec<u8>, ReadFailure> {
    let deadline = Instant::now() + HEADER_DEADLINE;
    let mut buf = Vec::new();
    let mut tmp = [0u8; 1024];
    loop {
        if let Some(end) = header_end(&buf) {
            if end > HEADER_LIMIT {
                return Err(ReadFailure::TooLarge);
            }
            return Ok(buf[..end].to_vec());
        }
        if buf.len() >= HEADER_LIMIT || has_bare_lf(&buf) {
            return Err(if buf.len() >= HEADER_LIMIT {
                ReadFailure::TooLarge
            } else {
                ReadFailure::Malformed
            });
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(ReadFailure::Timeout);
        }
        stream
            .set_read_timeout(Some(remaining.max(Duration::from_millis(1))))
            .map_err(|_| ReadFailure::Malformed)?;
        match stream.read(&mut tmp) {
            Ok(0) if buf.is_empty() => return Err(ReadFailure::Closed),
            Ok(0) => return Err(ReadFailure::Malformed),
            Ok(count) => buf.extend_from_slice(&tmp[..count]),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error)
                if error.kind() == io::ErrorKind::TimedOut
                    || error.kind() == io::ErrorKind::WouldBlock =>
            {
                return Err(ReadFailure::Timeout);
            }
            Err(_) => return Err(ReadFailure::Malformed),
        }
    }
}

fn header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| index + 4)
}

fn has_bare_lf(buf: &[u8]) -> bool {
    buf.iter()
        .enumerate()
        .any(|(index, byte)| *byte == b'\n' && (index == 0 || buf[index - 1] != b'\r'))
}

struct Response {
    status: u16,
    reason: &'static str,
    content_type: &'static str,
    body: Vec<u8>,
    include_body: bool,
    allow: bool,
}

impl Response {
    fn error(status: u16, reason: &'static str, body: &str) -> Self {
        Self {
            status,
            reason,
            content_type: "text/plain; charset=utf-8",
            body: body.as_bytes().to_vec(),
            include_body: true,
            allow: status == 405,
        }
    }

    fn document(content_type: &'static str, body: Vec<u8>, include_body: bool) -> Self {
        Self {
            status: 200,
            reason: "OK",
            content_type,
            body,
            include_body,
            allow: false,
        }
    }

    fn encode(self) -> Vec<u8> {
        let mut headers = format!(
            "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n",
            self.status,
            self.reason,
            self.content_type,
            self.body.len()
        );
        for (name, value) in SECURITY {
            headers.push_str(name);
            headers.push_str(": ");
            headers.push_str(value);
            headers.push_str("\r\n");
        }
        if self.allow {
            headers.push_str("Allow: GET, HEAD\r\n");
        }
        headers.push_str("\r\n");
        let mut bytes = headers.into_bytes();
        if self.include_body {
            bytes.extend(self.body);
        }
        bytes
    }
}

fn decide(bytes: &[u8], port: u16, html: &str) -> Response {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return Response::error(400, "Bad Request", "malformed request");
    };
    let mut lines = text.split("\r\n");
    let Some(request) = lines.next() else {
        return Response::error(400, "Bad Request", "malformed request");
    };
    let mut headers = Vec::new();
    for line in lines {
        if line.is_empty() {
            break;
        }
        if line.starts_with(' ') || line.starts_with('\t') || !line.contains(':') {
            return Response::error(400, "Bad Request", "malformed request");
        }
        let Some((name, value)) = line.split_once(':') else {
            return Response::error(400, "Bad Request", "malformed request");
        };
        if name.is_empty() || name.bytes().any(|byte| byte.is_ascii_whitespace()) {
            return Response::error(400, "Bad Request", "malformed request");
        }
        headers.push((name.to_ascii_lowercase(), value.trim().to_owned()));
        if headers.len() > HEADER_FIELD_LIMIT {
            return Response::error(431, "Request Header Fields Too Large", "headers too large");
        }
    }
    if let Some(response) = framing_error(&headers) {
        return response;
    }
    let mut parts = request.split(' ');
    let Some(method) = parts.next() else {
        return Response::error(400, "Bad Request", "malformed request");
    };
    let Some(target) = parts.next() else {
        return Response::error(400, "Bad Request", "malformed request");
    };
    let Some(version) = parts.next() else {
        return Response::error(400, "Bad Request", "malformed request");
    };
    if parts.next().is_some() || version != "HTTP/1.1" || !is_method_token(method) {
        return Response::error(400, "Bad Request", "malformed request");
    }
    if host_problem(&headers) {
        return Response::error(400, "Bad Request", "malformed request");
    }
    let head = method == "HEAD";
    if method != "GET" && !head {
        return finish(
            Response::error(405, "Method Not Allowed", "method not allowed"),
            false,
        );
    }
    if target_malformed(target) {
        return finish(
            Response::error(400, "Bad Request", "malformed request"),
            head,
        );
    }
    if foreign_request(&headers, port) {
        return finish(Response::error(403, "Forbidden", "forbidden"), head);
    }
    if target == "/" {
        finish(
            Response::document("text/html; charset=utf-8", html.as_bytes().to_vec(), true),
            head,
        )
    } else if target == "/style.css" {
        finish(
            Response::document(
                "text/css; charset=utf-8",
                STYLESHEET.as_bytes().to_vec(),
                true,
            ),
            head,
        )
    } else {
        finish(Response::error(404, "Not Found", "not found"), head)
    }
}

fn finish(mut response: Response, head: bool) -> Response {
    if head {
        response.include_body = false;
    }
    response
}

fn is_method_token(method: &str) -> bool {
    !method.is_empty()
        && method
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
}

fn framing_error(headers: &[(String, String)]) -> Option<Response> {
    if duplicate(headers, "content-length")
        || duplicate(headers, "transfer-encoding")
        || duplicate(headers, "origin")
        || duplicate(headers, "sec-fetch-site")
        || duplicate(headers, "connection")
    {
        return Some(Response::error(400, "Bad Request", "malformed request"));
    }
    if headers.iter().any(|(name, _)| name == "transfer-encoding") {
        return Some(Response::error(400, "Bad Request", "malformed request"));
    }
    if let Some((_, value)) = headers.iter().find(|(name, _)| name == "content-length") {
        let Ok(length) = value.parse::<u64>() else {
            return Some(Response::error(400, "Bad Request", "malformed request"));
        };
        if length != 0 || value.bytes().any(|byte| !byte.is_ascii_digit()) {
            return Some(Response::error(400, "Bad Request", "malformed request"));
        }
    }
    None
}

fn host_problem(headers: &[(String, String)]) -> bool {
    let count = headers.iter().filter(|(name, _)| name == "host").count();
    count != 1
}

fn foreign_request(headers: &[(String, String)], port: u16) -> bool {
    let expected_host = format!("127.0.0.1:{port}");
    let expected_origin = format!("http://127.0.0.1:{port}");
    let host = headers
        .iter()
        .find(|(name, _)| name == "host")
        .map(|(_, value)| value.as_str())
        .unwrap_or("");
    if host != expected_host {
        return true;
    }
    if let Some((_, origin)) = headers.iter().find(|(name, _)| name == "origin")
        && origin != &expected_origin
    {
        return true;
    }
    headers
        .iter()
        .any(|(name, value)| name == "sec-fetch-site" && value.eq_ignore_ascii_case("cross-site"))
}

fn duplicate(headers: &[(String, String)], name: &str) -> bool {
    headers.iter().filter(|(header, _)| header == name).count() > 1
}

fn target_malformed(target: &str) -> bool {
    target.contains("://")
        || target.contains('?')
        || target.contains('#')
        || target.contains('%')
        || target.contains('\\')
        || !target.starts_with('/')
        || target.split('/').any(|segment| segment == "..")
}
