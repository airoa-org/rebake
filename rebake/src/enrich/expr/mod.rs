//! Polars expression utilities for geometric types.
//!
//! Provides helper functions for working with quaternions and 3D vectors
//! in Polars expressions.
//!
//! # Responsibilities
//!
//! - Owns: Quaternion and Vector3 expression helpers
//! - Does not own: Enricher stage logic (see parent [`crate::enrich`] module)

mod quaternion;
mod vector3;

pub(crate) use quaternion::QuaternionExpr;
pub(crate) use vector3::Vector3Expr;
