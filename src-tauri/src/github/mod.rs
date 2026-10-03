//! GitHub pull requests and issues over the REST API. The token comes from the GitHub CLI if it is
//! installed and logged in (the account picked for the repository, or gh's active one), otherwise
//! from git's own credential store; it lives only in memory and is never written anywhere.

mod accounts;
mod attachments;
mod checkout;
mod checks;
mod client;
mod issues;
#[cfg(test)]
mod live_tests;
mod pulls;
mod repo;

pub use accounts::*;
pub use attachments::*;
pub use checkout::*;
pub use checks::*;
pub use client::*;
pub use issues::*;
pub use pulls::*;
pub use repo::*;
