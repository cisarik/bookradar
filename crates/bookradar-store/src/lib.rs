//! Local observation history for one BookRadar offer.
//!
//! The store appends caller-supplied observations and reviews. Current-candidate
//! selection calls `bookradar-core` for freshness and eligibility.

#![forbid(unsafe_code)]

mod fixtures;
mod history;
mod research;
mod research_contract;
mod store;

pub use fixtures::{Fixture, append_fixture, synthetic_offer, synthetic_target};
pub use history::{
    CandidateAssessment, CurrentView, HistoryEntry, StockClaim, render_current, render_history,
};
pub use research::{
    ImportContext, ImportReport, ResearchError, ResearchOfferKey, ResearchSnapshot, ResearchStore,
    render_import, render_inspect, render_research_current,
};
pub use research_contract::{
    ValidationOutcome, ValidationPosition, ValidationReport, validate_research,
};
pub use store::{ObservationRecord, OpenMode, ReviewDraft, ReviewRevision, Store, StoreError};

/// Version string of the SQLite library compiled into this crate.
pub fn bundled_sqlite_version() -> &'static str {
    rusqlite::version()
}
