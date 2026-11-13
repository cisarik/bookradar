//! Loopback read-only page for one synthetic BookRadar snapshot.

#![forbid(unsafe_code)]

use std::io::{self, Write};
use std::process::ExitCode;

use bookradar_ui::{CliRequest, bind_listener, help_text, load, parse_args, render_html, serve};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let request = match parse_args(&args) {
        Ok(request) => request,
        Err(message) => {
            eprintln!("usage: {message}");
            return ExitCode::from(2);
        }
    };
    let CliRequest::Run(config) = request else {
        print!("{}", help_text());
        let _ = io::stdout().flush();
        return ExitCode::SUCCESS;
    };
    let dashboard = load(&config);
    let html = render_html(&dashboard);
    let (listener, port) = match bind_listener(config.bind) {
        Ok(bound) => bound,
        Err(error) => {
            eprintln!("bind-failure: {error}");
            return ExitCode::from(1);
        }
    };
    let mut stdout = io::stdout();
    if writeln!(stdout, "listening=http://127.0.0.1:{port}").is_err() || stdout.flush().is_err() {
        eprintln!("bind-failure: stdout unavailable");
        return ExitCode::from(1);
    }
    if let Err(error) = serve(listener, port, &html) {
        eprintln!("bind-failure: {error}");
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}
