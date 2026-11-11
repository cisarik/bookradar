//! Synthetic persistence proof. This binary has four fixed commands.

#![forbid(unsafe_code)]

use std::env;
use std::path::Path;
use std::process::ExitCode;
use std::time::{Duration, SystemTime};

use bookradar_store::{
    Fixture, OpenMode, Store, StoreError, append_fixture, render_current, render_history,
    synthetic_offer, synthetic_target,
};

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(command) = args.next() else {
        eprintln!("usage: expected init, append-fixture, current, or history");
        return ExitCode::from(2);
    };
    match dispatch(&command, &mut args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(CliError::Usage(message)) => {
            eprintln!("usage: {message}");
            ExitCode::from(2)
        }
        Err(CliError::Store(error)) => {
            eprintln!("{}: {error}", error.category());
            ExitCode::from(1)
        }
    }
}

enum CliError {
    Usage(String),
    Store(StoreError),
}

impl From<StoreError> for CliError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

fn dispatch(command: &str, args: &mut impl Iterator<Item = String>) -> Result<(), CliError> {
    match command {
        "init" => init(args),
        "append-fixture" => append(args),
        "current" => current(args),
        "history" => history(args),
        _ => Err(CliError::Usage(
            "expected init, append-fixture, current, or history".to_owned(),
        )),
    }
}

fn init(args: &mut impl Iterator<Item = String>) -> Result<(), CliError> {
    let path = one_path(args, "init <path>")?;
    Store::create(Path::new(&path))?;
    println!("created schema=1 synthetic=true");
    Ok(())
}

fn append(args: &mut impl Iterator<Item = String>) -> Result<(), CliError> {
    let path = required(args, "append-fixture <path> <fixture>")?;
    let name = required(args, "append-fixture <path> <fixture>")?;
    no_more(args, "append-fixture <path> <fixture>")?;
    let Some(fixture) = Fixture::parse(&name) else {
        return Err(CliError::Usage(
            "fixture must be available, blocked, recovered, or older".to_owned(),
        ));
    };
    let mut store = Store::open(Path::new(&path), OpenMode::ReadWrite)?;
    append_fixture(&mut store, fixture)?;
    println!(
        "appended observation={} synthetic=true",
        fixture.observation_id()
    );
    Ok(())
}

fn current(args: &mut impl Iterator<Item = String>) -> Result<(), CliError> {
    let path = required(args, "current <path> <unix-seconds>")?;
    let time = required(args, "current <path> <unix-seconds>")?;
    no_more(args, "current <path> <unix-seconds>")?;
    let now = parse_unix_seconds(&time).ok_or_else(|| {
        CliError::Usage("current time must be an integer number of Unix seconds".to_owned())
    })?;
    let mut store = Store::open(Path::new(&path), OpenMode::ReadOnly)?;
    let view = store.current(synthetic_offer(), synthetic_target(), now)?;
    println!(
        "{}",
        render_current(&view, synthetic_offer(), synthetic_target(), now)
    );
    Ok(())
}

fn history(args: &mut impl Iterator<Item = String>) -> Result<(), CliError> {
    let path = one_path(args, "history <path>")?;
    let mut store = Store::open(Path::new(&path), OpenMode::ReadOnly)?;
    let entries = store.history(synthetic_offer())?;
    println!("{}", render_history(synthetic_offer(), &entries));
    Ok(())
}

fn one_path(args: &mut impl Iterator<Item = String>, usage: &str) -> Result<String, CliError> {
    let path = required(args, usage)?;
    no_more(args, usage)?;
    Ok(path)
}

fn required(args: &mut impl Iterator<Item = String>, usage: &str) -> Result<String, CliError> {
    args.next().ok_or_else(|| CliError::Usage(usage.to_owned()))
}

fn no_more(args: &mut impl Iterator<Item = String>, usage: &str) -> Result<(), CliError> {
    if args.next().is_some() {
        Err(CliError::Usage(usage.to_owned()))
    } else {
        Ok(())
    }
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
