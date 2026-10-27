// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! CLI application execution and cancellable run locking.
use std::fs;
use std::thread;
use std::time::Duration;
use std::time::Instant;

use clap::Parser;
use serde_json::json;

use crate::api;
use crate::artifact;
use crate::cli::Cli;
use crate::cli::Operation;
use crate::config::Config;
use crate::pages_error::PagesError;
use crate::report;
use crate::report::Run;
use crate::report::tool;
use crate::report::write_json;
use crate::reports;
use crate::site;
use crate::source_exclusions;
/// Execute the CLI, writing diagnostics to stderr and common reports to disk.
///
/// Returns cpp-infra's stable process exit code. Configuration, document gate,
/// operating system failures and cancellation are distinguished in reports.
pub fn run() -> i32 {
    let cli = Cli::parse();
    if cli.version {
        if cli.json {
            let mut v = tool();
            v["protocol"] = json!(1);
            println!("{v}");
        } else {
            println!("cpp-infra-pages {}", env!("CARGO_PKG_VERSION"));
        }
        return 0;
    }
    let start = Instant::now();
    let mut run = match Run::create(&cli) {
        Ok(run) => run,
        Err(error) => {
            eprintln!("{error}");
            if let Some(path) = &cli.report {
                let safe = cli
                    .project
                    .canonicalize()
                    .ok()
                    .is_some_and(|project| source_exclusions::validate_output(&project, path).is_ok());
                if !safe {
                    return error.exit_code();
                }
                let mut report = json!({"schema":1,"tool":tool(),"bundle_id":"development","run_id":"uninitialized","task_id":"pages","project":cli.project,"report_root":"","source_revision":null,"source_digest":null,"config_digest":null,"status":"error","started_at":chrono::Utc::now().to_rfc3339(),"duration_ms":start.elapsed().as_millis(),"external_versions":{},"commands":[],"diagnostics":[],"artifacts":[]});
                report["diagnostics"] = json!([{"code":"pages.context","severity":"error","message":error.to_string(),"project_relative_path":null}]);
                if let Err(e) = write_json(path, &report) {
                    eprintln!("cannot write error report: {e}");
                    return 3;
                }
            }
            return error.exit_code();
        }
    };
    api::install_cancel_handler();
    let result = execute(&cli, &mut run).and_then(|()| run.fresh());
    let code = match &result {
        Ok(()) => 0,
        Err(error) => error.exit_code(),
    };
    run.report["status"] = json!(match code {
        0 => "passed",
        1 => "failed",
        130 => "cancelled",
        _ => "error",
    });
    run.report["duration_ms"] = json!(start.elapsed().as_millis());
    if let Err(error) = result {
        eprintln!("{error}");
        run.report["diagnostics"] = json!([{"code":"pages.execution","severity":"error","message":error.to_string(),"project_relative_path":null}]);
    }
    let path = cli.report.clone().unwrap_or_else(|| run.work.join("pages-report.json"));
    if let Err(error) = source_exclusions::validate_output(&run.project, &path) {
        eprintln!("cannot write report {}: {error}", path.display());
        return error.exit_code();
    }
    if let Err(error) = write_json(&path, &run.report) {
        eprintln!("cannot write report {}: {error}", path.display());
        return 3;
    }
    code
}
fn execute(cli: &Cli, run: &mut Run) -> Result<(), PagesError> {
    api::ensure_active()?;
    let config = Config::load(&run.project)?;
    let operation = cli
        .command
        .ok_or_else(PagesError::required("a pages subcommand is required"))?;
    if matches!(operation, Operation::Plan) {
        println!(
            "{}",
            json!({"schema":1,"nodes":[{"id":"pages","tool":"cpp-infra-pages","args":["build"],"needs":[],"profile":run.report["data"]["profile"],"resource_locks":["pages-run-directory"],"required":true,"timeout_seconds":config.api.timeout_seconds,"output":"runtime run directory","discovery":if config.api.enabled {"Doxygen version and generated API pages"}else{"configured README and assets"}}]})
        );
        return Ok(());
    }
    report::reject_symlink(&run.root.join(".pages.lock"))?;
    let lockfile = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(run.root.join(".pages.lock"))?;
    acquire_run_lock(&lockfile, Duration::from_secs(config.api.timeout_seconds.max(1)))?;
    if matches!(operation, Operation::Api) {
        api::generate(&config, run)?;
        return Ok(());
    }
    let evidence = reports::collect(&config, run)?;
    if config.api.enabled {
        api::generate(&config, run)?;
    }
    let site = site::build(&config, run, &evidence)?;
    if matches!(operation, Operation::Artifact | Operation::Deploy) {
        artifact::create(run, &site, matches!(operation, Operation::Deploy))?;
    }
    Ok(())
}

fn acquire_run_lock(lock: &fs::File, timeout: Duration) -> Result<(), PagesError> {
    let started = Instant::now();
    loop {
        api::ensure_active()?;
        match fs2::FileExt::try_lock_exclusive(lock) {
            Ok(()) => {
                api::ensure_active()?;
                return Ok(());
            }
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock
                    || error.raw_os_error() == fs2::lock_contended_error().raw_os_error() =>
            {
                if started.elapsed() >= timeout {
                    return Err(PagesError::Io(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "timed out waiting for pages run lock",
                    )));
                }
                thread::sleep(Duration::from_millis(20));
            }
            Err(error) => return Err(PagesError::Io(error)),
        }
    }
}
