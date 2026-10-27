// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Safe documentation sites, reproducible archives, and deployment handoffs.
mod api;
mod application;
mod artifact;
pub mod cli;
mod config;
mod identity;
mod links;
pub mod pages_error;
mod report;
mod report_validation;
mod reports;
mod site;
mod source_exclusions;

pub use crate::application::run;

#[cfg(test)]
mod tests;
