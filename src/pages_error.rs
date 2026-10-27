// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Typed execution failures and protocol exit codes.
use std::io;

use thiserror::Error;
/// Failure reported by the documentation pipeline.
#[derive(Debug, Error)]
pub enum PagesError {
    /// Invalid configuration or incompatible input identity.
    #[error("{0}")]
    Config(String),
    /// A required documentation or link gate failed.
    #[error("{0}")]
    Gate(String),
    /// An operating system operation failed.
    #[error(transparent)]
    Io(#[from] io::Error),
    /// The caller cancelled execution.
    #[error("execution cancelled")]
    Cancelled,
}
impl PagesError {
    /// Return the common cpp-infra process exit code.
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Config(_) => 2,
            Self::Gate(_) => 1,
            Self::Io(_) => 3,
            Self::Cancelled => 130,
        }
    }
}

impl PagesError {
    /// Convert a configuration decoder or path error without changing its
    /// detail.
    pub(crate) fn configuration(error: impl std::fmt::Display) -> Self {
        Self::Config(error.to_string())
    }
    /// Convert a documentation parser failure while preserving the gate
    /// message.
    pub(crate) fn gate(error: impl std::fmt::Display) -> Self {
        Self::Gate(error.to_string())
    }
    /// Create a lazy failure for required protocol or path metadata.
    pub(crate) fn required(message: &str) -> impl FnOnce() -> Self + '_ {
        move || Self::Config(message.into())
    }
    /// Preserve the underlying filesystem error when an atomic output fails.
    pub(crate) fn persistence(error: tempfile::PersistError) -> Self {
        Self::Io(error.error)
    }
    /// Preserve a directory traversal failure as an infrastructure error.
    pub(crate) fn traversal(error: walkdir::Error) -> Self {
        Self::Io(std::io::Error::other(error))
    }
}
