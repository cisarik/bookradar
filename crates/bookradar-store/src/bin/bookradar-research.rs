//! Synthetic research-ingestion proof. This binary has four fixed commands.

#![forbid(unsafe_code)]

use std::env;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::process::ExitCode;
use std::time::{Duration, SystemTime};

use bookradar_core::EditionId;
use bookradar_store::{
    ImportContext, OpenMode, ResearchError, ResearchOfferKey, ResearchStore, StoreError,
    render_import, render_inspect, render_research_current,
};

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(command) = args.next() else {
        eprintln!("usage: expected init, import, inspect, or current");
        return ExitCode::from(2);
    };
    match dispatch(&command, &mut args) {
        Ok(Outcome::Success) => ExitCode::SUCCESS,
        Ok(Outcome::Partial) => ExitCode::from(3),
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

enum Outcome {
    Success,
    Partial,
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

fn dispatch(command: &str, args: &mut impl Iterator<Item = String>) -> Result<Outcome, CliError> {
    match command {
        "init" => init(args),
        "import" => import(args),
        "inspect" => inspect(args),
        "current" => current(args),
        _ => Err(CliError::Usage(
            "expected init, import, inspect, or current".to_owned(),
        )),
    }
}

fn init(args: &mut impl Iterator<Item = String>) -> Result<Outcome, CliError> {
    let path = one(args, "init <ledger>")?;
    ResearchStore::create(Path::new(&path))?;
    println!("created research_schema=1");
    Ok(Outcome::Success)
}

fn import(args: &mut impl Iterator<Item = String>) -> Result<Outcome, CliError> {
    let ledger = required(args, "import <ledger> <input.json> <ingested-unix-seconds>")?;
    let input = required(args, "import <ledger> <input.json> <ingested-unix-seconds>")?;
    let time = required(args, "import <ledger> <input.json> <ingested-unix-seconds>")?;
    no_more(args, "import <ledger> <input.json> <ingested-unix-seconds>")?;
    let ingested = parse_unix_seconds(&time).ok_or_else(|| {
        CliError::Usage("ingested time must be an integer number of Unix seconds".to_owned())
    })?;
    let bytes = read_bounded(Path::new(&input))?;
    let mut store = ResearchStore::open(Path::new(&ledger), OpenMode::ReadWrite)?;
    match store.import(&bytes, ImportContext::new(ingested)) {
        Ok(report) => {
            let partial = report.rejected + report.quarantined > 0 || report.seen.is_none();
            println!("{}", render_import(&report));
            if partial {
                Ok(Outcome::Partial)
            } else {
                Ok(Outcome::Success)
            }
        }
        Err(ResearchError::Oversized) => Err(CliError::Usage("input exceeds 1 MiB".to_owned())),
        Err(ResearchError::InvalidArgument) => {
            Err(CliError::Usage("invalid research argument".to_owned()))
        }
        Err(ResearchError::Store(error)) => Err(CliError::Store(error)),
    }
}

fn inspect(args: &mut impl Iterator<Item = String>) -> Result<Outcome, CliError> {
    let path = one(args, "inspect <ledger>")?;
    let mut store = ResearchStore::open(Path::new(&path), OpenMode::ReadOnly)?;
    let snapshot = store.inspect()?;
    print!("{}", render_inspect(&snapshot));
    Ok(Outcome::Success)
}

fn current(args: &mut impl Iterator<Item = String>) -> Result<Outcome, CliError> {
    let usage = "current <ledger> <producer> <source> <listing> <generation> <target-edition-id> <now-unix-seconds>";
    let ledger = required(args, usage)?;
    let producer = required(args, usage)?;
    let source = required(args, usage)?;
    let listing = required(args, usage)?;
    let generation = required(args, usage)?;
    let edition = required(args, usage)?;
    let time = required(args, usage)?;
    no_more(args, usage)?;
    let key = ResearchOfferKey::try_new(producer, source, listing, generation).map_err(|_| {
        CliError::Usage("producer, source, listing, and generation must be identifiers".to_owned())
    })?;
    let edition = parse_edition(&edition).ok_or_else(|| {
        CliError::Usage("target edition id must be a positive integer".to_owned())
    })?;
    let now = parse_unix_seconds(&time).ok_or_else(|| {
        CliError::Usage("current time must be an integer number of Unix seconds".to_owned())
    })?;
    let mut store = ResearchStore::open(Path::new(&ledger), OpenMode::ReadOnly)?;
    let view = store.current(&key, edition, now)?;
    println!("{}", render_research_current(&view));
    Ok(Outcome::Success)
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, CliError> {
    let file = File::open(path).map_err(StoreError::Io)?;
    let mut reader = file.take(1_048_577);
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).map_err(StoreError::Io)?;
    if bytes.len() > 1_048_576 {
        Err(CliError::Usage("input exceeds 1 MiB".to_owned()))
    } else {
        Ok(bytes)
    }
}

fn one(args: &mut impl Iterator<Item = String>, usage: &str) -> Result<String, CliError> {
    let value = required(args, usage)?;
    no_more(args, usage)?;
    Ok(value)
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

fn parse_edition(text: &str) -> Option<EditionId> {
    if text.is_empty() || text.starts_with('+') {
        return None;
    }
    let id: u64 = text.parse().ok()?;
    EditionId::new(id).ok()
}
