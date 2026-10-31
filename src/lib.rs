//! AutoRumble is a small rumble server for Robocode.
//!
//! Clients run battles and upload the results. The server stores every result, works out the
//! scores and serves the rankings. See `README.md` for an overview and how the modules fit
//! together.

pub mod config;
pub mod engine;
pub mod logos;
pub mod pages;
pub mod participants;
pub mod priority;
pub mod protocol;
pub mod scoring;
pub mod store;
pub mod time;
pub mod web;
