//! Mayasaba core/controller: cross-subsystem application services.
//!
//! A service owns the meaning of a concept, validates the request against the contract, and delegates
//! persistence to the owning storage crate. It never becomes a second authority for a concept another
//! service owns (AGENTS.md section 6).

pub mod project_service;

pub use project_service::{CreateProjectOutcome, ProjectService, ProjectValidationError};