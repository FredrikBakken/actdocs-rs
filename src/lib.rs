//! Generate documentation for GitHub Actions and reusable workflows.
//!
//! The crate is split so that the binary is a thin shell over testable pieces:
//!
//! - [`scalar`] models a single optional value from the source YAML and owns
//!   every rule for turning one into Markdown.
//! - [`model`] describes a parsed action or workflow.

pub mod doc;
pub mod model;
pub mod parse;
pub mod render;
pub mod scalar;
pub mod sync;
pub mod target;

pub use model::{ActionInput, ActionSpec, Output, Permission, Secret, WorkflowInput, WorkflowSpec};
pub use parse::{Document, parse};
pub use scalar::Scalar;
