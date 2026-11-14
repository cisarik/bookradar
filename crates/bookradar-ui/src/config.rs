//! Command-line validation. No filesystem or socket work happens here.

use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use bookradar_core::{EditionId, OfferId, SourceId};
use bookradar_store::ResearchOfferKey;

pub const USAGE: &str = "bookradar-ui --store <path> --research-ledger <path> --now <unix-seconds> [--bind 127.0.0.1:<port>] [--source-id <positive-integer>] [--offer-id <positive-integer>] [--producer <identifier>] [--source-ref <identifier>] [--listing-ref <identifier>] [--listing-generation <identifier>] [--target-edition <positive-integer>]";

/// Checked operator settings for one immutable local page.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    pub store: PathBuf,
    pub research_ledger: PathBuf,
    pub now: SystemTime,
    pub now_seconds: i64,
    pub bind: SocketAddr,
    pub producer: String,
    pub source_ref: String,
    pub listing_ref: String,
    pub listing_generation: String,
    pub source_id: u64,
    pub offer_id: u64,
    pub target: EditionId,
}

pub enum CliRequest {
    Help,
    Run(Box<Config>),
}

pub fn help_text() -> &'static str {
    "bookradar-ui serves one local read-only BookRadar page.\n\
     \n\
     bookradar-ui --store <path> --research-ledger <path> --now <unix-seconds> \\\n\
       [--bind 127.0.0.1:<port>] [--source-id <positive-integer>] \\\n\
       [--offer-id <positive-integer>] [--producer <identifier>] \\\n\
       [--source-ref <identifier>] [--listing-ref <identifier>] \\\n\
       [--listing-generation <identifier>] [--target-edition <positive-integer>]\n\
     \n\
     --now is required. The default bind is 127.0.0.1:0.\n\
     The default observation offer is source 1, offer 1.\n\
     The default target edition is 10.\n\
     The default research key is synthetic:producer, synthetic:source, OFFER-A, generation-1.\n"
}

pub fn parse_args(args: &[String]) -> Result<CliRequest, String> {
    if args.len() == 1 && args[0] == "--help" {
        return Ok(CliRequest::Help);
    }
    let mut store = None;
    let mut research_ledger = None;
    let mut now = None;
    let mut bind = None;
    let mut producer = None;
    let mut source_ref = None;
    let mut listing_ref = None;
    let mut listing_generation = None;
    let mut source_id = None;
    let mut offer_id = None;
    let mut target = None;
    let mut index = 0;
    while index < args.len() {
        let flag = &args[index];
        if !flag.starts_with("--") || flag == "--" {
            return Err(format!("{USAGE}; unexpected argument"));
        }
        let Some(value) = args.get(index + 1).filter(|value| !value.starts_with("--")) else {
            return Err(format!("{USAGE}; missing value for {flag}"));
        };
        match flag.as_str() {
            "--store" => assign_once(&mut store, value.clone(), flag)?,
            "--research-ledger" => assign_once(&mut research_ledger, value.clone(), flag)?,
            "--now" => assign_once(&mut now, value.clone(), flag)?,
            "--bind" => assign_once(&mut bind, value.clone(), flag)?,
            "--producer" => assign_once(&mut producer, value.clone(), flag)?,
            "--source-ref" => assign_once(&mut source_ref, value.clone(), flag)?,
            "--listing-ref" => assign_once(&mut listing_ref, value.clone(), flag)?,
            "--listing-generation" => assign_once(&mut listing_generation, value.clone(), flag)?,
            "--source-id" => assign_once(&mut source_id, value.clone(), flag)?,
            "--offer-id" => assign_once(&mut offer_id, value.clone(), flag)?,
            "--target-edition" => assign_once(&mut target, value.clone(), flag)?,
            "--help" => {
                return Err(format!("{USAGE}; --help must be the only argument"));
            }
            _ => return Err(format!("{USAGE}; unknown option {flag}")),
        }
        index += 2;
    }
    let store = store.ok_or_else(|| format!("{USAGE}; missing --store"))?;
    let research_ledger =
        research_ledger.ok_or_else(|| format!("{USAGE}; missing --research-ledger"))?;
    let now_text = now.ok_or_else(|| format!("{USAGE}; missing --now"))?;
    let (now_seconds, now) = parse_unix_seconds(&now_text)
        .ok_or_else(|| format!("{USAGE}; --now must be a representable integer Unix seconds"))?;
    let bind = match bind {
        None => SocketAddr::from((Ipv4Addr::new(127, 0, 0, 1), 0)),
        Some(text) => parse_bind(&text).ok_or_else(|| {
            format!("{USAGE}; --bind must be the literal address 127.0.0.1:<port>")
        })?,
    };
    let producer = producer.unwrap_or_else(|| "synthetic:producer".to_owned());
    let source_ref = source_ref.unwrap_or_else(|| "synthetic:source".to_owned());
    let listing_ref = listing_ref.unwrap_or_else(|| "OFFER-A".to_owned());
    let listing_generation = listing_generation.unwrap_or_else(|| "generation-1".to_owned());
    if ResearchOfferKey::try_new(&producer, &source_ref, &listing_ref, &listing_generation).is_err()
    {
        return Err(format!(
            "{USAGE}; producer, source, listing, and generation must be identifiers"
        ));
    }
    let target = match target {
        None => EditionId::new(10).expect("edition 10"),
        Some(text) => parse_edition(&text)
            .ok_or_else(|| format!("{USAGE}; --target-edition must be a positive integer"))?,
    };
    let source_id = match source_id {
        None => 1,
        Some(text) => parse_store_id(&text).ok_or_else(|| {
            format!("{USAGE}; --source-id must be a positive integer that fits the store")
        })?,
    };
    let offer_id = match offer_id {
        None => 1,
        Some(text) => parse_store_id(&text).ok_or_else(|| {
            format!("{USAGE}; --offer-id must be a positive integer that fits the store")
        })?,
    };
    Ok(CliRequest::Run(Box::new(Config {
        store: PathBuf::from(store),
        research_ledger: PathBuf::from(research_ledger),
        now,
        now_seconds,
        bind,
        producer,
        source_ref,
        listing_ref,
        listing_generation,
        source_id,
        offer_id,
        target,
    })))
}

fn assign_once(slot: &mut Option<String>, value: String, flag: &str) -> Result<(), String> {
    if slot.is_some() {
        Err(format!("{USAGE}; duplicate {flag}"))
    } else {
        *slot = Some(value);
        Ok(())
    }
}

fn parse_unix_seconds(text: &str) -> Option<(i64, SystemTime)> {
    if text.is_empty() || text.starts_with('+') {
        return None;
    }
    let seconds: i64 = text.parse().ok()?;
    let time = if seconds >= 0 {
        let secs = u64::try_from(seconds).ok()?;
        SystemTime::UNIX_EPOCH.checked_add(Duration::from_secs(secs))?
    } else {
        let absolute = u64::try_from(seconds.checked_neg()?).ok()?;
        SystemTime::UNIX_EPOCH.checked_sub(Duration::from_secs(absolute))?
    };
    Some((seconds, time))
}

fn parse_bind(text: &str) -> Option<SocketAddr> {
    let (ip, port) = text.rsplit_once(':')?;
    if ip != "127.0.0.1" || port.is_empty() || !port.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    if port.len() > 1 && port.starts_with('0') {
        return None;
    }
    let port: u16 = port.parse().ok()?;
    Some(SocketAddr::from((Ipv4Addr::new(127, 0, 0, 1), port)))
}

fn parse_store_id(text: &str) -> Option<u64> {
    if text.is_empty() || text.starts_with('+') || text.starts_with('-') {
        return None;
    }
    let id: u64 = text.parse().ok()?;
    if i64::try_from(id).is_err() {
        return None;
    }
    SourceId::new(id).ok()?;
    OfferId::new(id).ok()?;
    Some(id)
}

fn parse_edition(text: &str) -> Option<EditionId> {
    if text.is_empty() || text.starts_with('+') || text.starts_with('-') {
        return None;
    }
    let id: u64 = text.parse().ok()?;
    EditionId::new(id).ok()
}
