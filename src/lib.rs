#[macro_use]
extern crate anyhow;
#[macro_use]
extern crate cascade;
#[macro_use]
extern crate futures;
#[macro_use]
extern crate log;
#[macro_use]
extern crate serde;
#[macro_use]
extern crate smart_default;
#[macro_use]
extern crate thiserror;

// pub mod apt;
pub mod blacklist;
pub mod collate;
pub mod config;
pub mod dpkg;
pub mod errors;
pub mod fetcher;
pub mod git;
pub mod github;
pub mod misc;

use std::cell::RefCell;

thread_local! {
    /// Thread-local string buffer, used as a temporary scratch space.
    pub static STRING_BUF: RefCell<String> = RefCell::new(String::new());
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("configuration error")]
    Config(#[from] config::Error),
    #[error("github error")]
    GitHub(#[from] github::Error),
}
