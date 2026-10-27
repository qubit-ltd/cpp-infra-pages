// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Command-line adapter types.
use std::path::PathBuf;

use clap::Parser;
use clap::Subcommand;
/// Shared global options and selected documentation operation.
#[derive(Debug, Parser)]
#[command(name = "cpp-infra-pages", disable_version_flag = true)]
pub struct Cli {
    #[arg(long, global = true, default_value = ".")]
    pub(crate) project: PathBuf,
    #[arg(long, global = true)]
    pub(crate) report: Option<PathBuf>,
    #[arg(long, global = true)]
    pub(crate) offline: bool,
    #[arg(long, global = true)]
    pub(crate) run_context: Option<PathBuf>,
    #[arg(long, global = true)]
    pub(crate) input_dir: Option<PathBuf>,
    #[arg(long)]
    pub(crate) version: bool,
    #[arg(long)]
    pub(crate) json: bool,
    #[command(subcommand)]
    pub(crate) command: Option<Operation>,
}
#[derive(Debug, Clone, Copy, Subcommand)]
pub(crate) enum Operation {
    Plan,
    Api,
    Build,
    Artifact,
    Deploy,
}
