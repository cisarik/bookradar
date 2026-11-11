//! Local observation history for one BookRadar offer.
//!
//! The store appends caller-supplied observations and reviews. Current-candidate
//! selection calls `bookradar-core` for freshness and eligibility.

#![forbid(unsafe_code)]

mod fixtures;
mod history;
mod store;

pub use fixtures::{Fixture, append_fixture, synthetic_offer, synthetic_target};
pub use history::{
    CandidateAssessment, CurrentView, HistoryEntry, StockClaim, render_current, render_history,
};
pub use store::{ObservationRecord, OpenMode, ReviewDraft, ReviewRevision, Store, StoreError};

/// Version string of the SQLite library compiled into this crate.
pub fn bundled_sqlite_version() -> &'static str {
    rusqlite::version()
}
