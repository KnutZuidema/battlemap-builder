//! Adapters implementing ports for external systems, storage, and interfaces.

pub mod cost_ledger;
pub mod filesystem_blob_storage;
pub mod filesystem_catalog_repository;
pub mod in_memory;
pub mod openrouter;
pub mod reddit;
pub mod simd_image;
#[allow(unused_imports)]
pub use cost_ledger::*;
#[allow(unused_imports)]
pub use filesystem_blob_storage::*;
#[allow(unused_imports)]
pub use filesystem_catalog_repository::*;
#[allow(unused_imports)]
pub use in_memory::*;
#[allow(unused_imports)]
pub use openrouter::*;
#[allow(unused_imports)]
pub use reddit::*;
#[allow(unused_imports)]
pub use simd_image::*;
