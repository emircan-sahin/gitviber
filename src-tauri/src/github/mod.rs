//! GitHub pull requests and issues over the REST API. The token comes from the GitHub CLI if it is
//! installed and logged in, otherwise from git's own credential store; it lives only in
//! memory and is never written anywhere.

mod attachments;
mod checkout;
mod client;
mod issues;
#[cfg(test)]
mod live_tests;
mod pulls;
mod repo;

pub use attachments::*;
pub use checkout::*;
pub use client::*;
pub use issues::*;
pub use pulls::*;
pub use repo::*;
