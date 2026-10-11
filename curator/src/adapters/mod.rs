//! Adapters implementing ports for external systems, storage, and interfaces.

pub mod filesystem_storage;
pub mod openrouter;
pub mod reddit;

#[allow(unused_imports)]
pub use filesystem_storage::*;
#[allow(unused_imports)]
pub use openrouter::*;
#[allow(unused_imports)]
pub use reddit::*;
