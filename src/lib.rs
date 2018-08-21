//! A Rust client for BigML's REST API.

// Needed for error-chain.
#![recursion_limit = "1024"]

#![warn(missing_docs)]

#[macro_use]
extern crate bigml_derive;
extern crate chrono;
#[macro_use]
extern crate failure;
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate log;
extern crate mime;
extern crate reqwest;
extern crate serde;
#[macro_use]
extern crate serde_derive;
#[cfg_attr(test, macro_use)]
extern crate serde_json;
extern crate url;
extern crate uuid;

pub use client::Client;
pub use errors::*;
pub use progress::{ProgressCallback, ProgressOptions};
pub use wait::WaitOptions;

#[macro_use]
pub mod wait;
mod client;
mod errors;
mod multipart_form_data;
mod progress;
pub mod resource;
