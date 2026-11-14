//! Startup reads. Each database is opened read-only and closed before serving.

use std::time::SystemTime;

use bookradar_core::{OfferId, OfferRef, SourceId};
use bookradar_store::{
    CurrentView, HistoryEntry, OpenMode, ResearchOfferKey, ResearchSnapshot, ResearchStore, Store,
    StoreError,
};

use crate::config::Config;

/// One operator-facing page captured at process start.
#[derive(Clone, Debug)]
pub struct Dashboard {
    pub store_path: String,
    pub ledger_path: String,
    pub now: SystemTime,
    pub now_seconds: i64,
    pub target_edition: u64,
    pub producer: String,
    pub source_ref: String,
    pub listing_ref: String,
    pub listing_generation: String,
    pub source_id: u64,
    pub offer_id: u64,
    pub observations: Slot<ObservationReading>,
    pub research: Slot<ResearchReading>,
}

#[derive(Clone, Debug)]
pub struct ObservationReading {
    pub history: Vec<HistoryEntry>,
    pub current: CurrentView,
}

#[derive(Clone, Debug)]
pub struct ResearchReading {
    pub ledger: ResearchSnapshot,
    pub current: CurrentView,
}

#[derive(Clone, Debug)]
pub enum Slot<T> {
    Ready(T),
    Failed(SlotFailure),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SlotFailure {
    pub category: &'static str,
    pub message: String,
}

pub fn load(config: &Config) -> Dashboard {
    let observations = load_observations(config);
    let research = load_research(config);
    Dashboard {
        store_path: config.store.display().to_string(),
        ledger_path: config.research_ledger.display().to_string(),
        now: config.now,
        now_seconds: config.now_seconds,
        target_edition: config.target.get(),
        producer: config.producer.clone(),
        source_ref: config.source_ref.clone(),
        listing_ref: config.listing_ref.clone(),
        listing_generation: config.listing_generation.clone(),
        source_id: config.source_id,
        offer_id: config.offer_id,
        observations,
        research,
    }
}

fn load_observations(config: &Config) -> Slot<ObservationReading> {
    let loaded = (|| {
        let source = SourceId::new(config.source_id).map_err(StoreError::Domain)?;
        let offer_id = OfferId::new(config.offer_id).map_err(StoreError::Domain)?;
        let offer = OfferRef::new(source, offer_id);
        let mut store = Store::open(&config.store, OpenMode::ReadOnly)?;
        let history = store.history(offer)?;
        let current = store.current(offer, config.target, config.now)?;
        drop(store);
        Ok(ObservationReading { history, current })
    })();
    match loaded {
        Ok(reading) => Slot::Ready(reading),
        Err(error) => Slot::Failed(slot_failure(&error)),
    }
}

fn load_research(config: &Config) -> Slot<ResearchReading> {
    let loaded = (|| {
        let key = ResearchOfferKey::try_new(
            config.producer.clone(),
            config.source_ref.clone(),
            config.listing_ref.clone(),
            config.listing_generation.clone(),
        )
        .map_err(|_| StoreError::UnsupportedFormat {
            reason: "research-key",
        })?;
        let mut store = ResearchStore::open(&config.research_ledger, OpenMode::ReadOnly)?;
        let ledger = store.inspect()?;
        let current = store.current(&key, config.target, config.now)?;
        drop(store);
        Ok(ResearchReading { ledger, current })
    })();
    match loaded {
        Ok(reading) => Slot::Ready(reading),
        Err(error) => Slot::Failed(slot_failure(&error)),
    }
}

fn slot_failure(error: &StoreError) -> SlotFailure {
    match error {
        StoreError::NotFound => SlotFailure {
            category: "not-found",
            message: "Database not found (not-found).".to_owned(),
        },
        StoreError::UnsupportedFormat { reason } => SlotFailure {
            category: "unsupported-schema",
            message: format!("Unsupported database (unsupported-schema: {reason})."),
        },
        StoreError::Io(_) => SlotFailure {
            category: "filesystem",
            message: "Database could not be read (filesystem).".to_owned(),
        },
        _ => SlotFailure {
            category: "storage",
            message: "Database could not be read (storage).".to_owned(),
        },
    }
}
