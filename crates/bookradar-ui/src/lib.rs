//! Local read-only BookRadar page. The binary embeds its stylesheet and does
//! not open a network path except the literal loopback listener.

#![forbid(unsafe_code)]

mod config;
mod http;
mod render;
mod snapshot;

pub use config::{CliRequest, Config, USAGE, help_text, parse_args};
pub use http::{STYLESHEET, bind_listener, serve};
pub use render::render_html;
pub use snapshot::{Dashboard, ObservationReading, ResearchReading, Slot, SlotFailure, load};
