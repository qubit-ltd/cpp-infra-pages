// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Quality evidence discovery with input identity validation.
use std::fs;
use std::path::Path;
use std::path::PathBuf;

use serde_json::Value;
use serde_json::json;

use crate::config::Config;
use crate::identity::artifact_digest;
use crate::pages_error::PagesError;
use crate::report::Run;
use crate::report::read_json;
use crate::report::write_json;
pub(crate) struct Evidence {
    pub ci: Option<Value>,
    pub coverage: Option<Value>,
    pub coverage_html: Option<PathBuf>,
}
pub(crate) fn collect(config: &Config, run: &Run) -> Result<Evidence, PagesError> {
    let mut evidence = Evidence {
        ci: None,
        coverage: None,
        coverage_html: None,
    };
    for input in &run.inputs {
        let path = Path::new(
            input["path"]
                .as_str()
                .ok_or_else(PagesError::required("invalid report input"))?,
        );
        let report = read_json(path)?;
        let name = report["tool"]["name"].as_str().unwrap_or("");
        let is_ci = input["kind"] == "quality-summary" || report["data"]["phase"] == "quality";
        let is_coverage = input["kind"] == "coverage-summary" || name == "cpp-infra-coverage";
        if !is_ci && !is_coverage {
            continue;
        }
        crate::report_validation::common(&report)?;
        if is_ci {
            crate::report_validation::ci(&report)?;
        }
        for key in ["schema", "bundle_id", "source_digest", "config_digest"] {
            if report[key] != run.report[key] {
                return Err(PagesError::Config(format!(
                    "stale or incompatible {key} in {}",
                    path.display()
                )));
            }
        }
        if report["status"] != "passed" {
            return Err(PagesError::Gate(format!("quality report failed: {}", path.display())));
        }
        if is_ci {
            let data = &report["data"];
            if data["complete"] != true || data["incomplete_remote_coverage"] == true {
                return Err(PagesError::Gate("CI quality-summary is incomplete".into()));
            }
            let tasks = data["tasks"]
                .as_array()
                .ok_or_else(PagesError::required("missing CI tasks"))?;
            if tasks.is_empty() || tasks.iter().any(|t| t["required"] == true && t["status"] != "passed") {
                return Err(PagesError::Gate(
                    "CI quality-summary contains failed required tasks".into(),
                ));
            }
            let expected = data["expected_task_ids"]
                .as_array()
                .ok_or_else(PagesError::required("missing expected CI task inventory"))?;
            if expected.is_empty() || expected.iter().any(|id| !tasks.iter().any(|t| t["task_id"] == *id)) {
                return Err(PagesError::Gate("CI quality-summary is missing expected tasks".into()));
            }
            evidence.ci = Some(report.clone());
        }
        if is_coverage {
            if name != "cpp-infra-coverage" {
                return Err(PagesError::Config(
                    "coverage-summary must identify cpp-infra-coverage".into(),
                ));
            }
            let lines = &report["data"]["summary"]["lines"];
            let covered = lines["covered"]
                .as_u64()
                .ok_or_else(PagesError::required("coverage covered count must be unsigned"))?;
            let total = lines["total"]
                .as_u64()
                .ok_or_else(PagesError::required("coverage total count must be unsigned"))?;
            let percent = lines["percent"]
                .as_f64()
                .ok_or_else(PagesError::required("coverage percentage must be numeric"))?;
            if covered > total || !percent.is_finite() || !(0.0..=100.0).contains(&percent) {
                return Err(PagesError::Config(
                    "coverage counts or percentage outside valid range".into(),
                ));
            }
            let declared_root = report["report_root"]
                .as_str()
                .and_then(|p| PathBuf::from(p).canonicalize().ok())
                .filter(|p| p.starts_with(run.project.join("build/infra-runs")));
            let portable_root = path
                .parent()
                .ok_or_else(PagesError::required("coverage report has no parent"))?
                .canonicalize()?;
            let portable_root = if portable_root.starts_with(&run.root) {
                Some(portable_root)
            } else {
                None
            };
            for artifact in report["artifacts"]
                .as_array()
                .ok_or_else(PagesError::required("missing coverage artifacts"))?
            {
                if artifact["kind"] != "coverage-html" {
                    continue;
                }
                let path = crate::config::relative(
                    artifact["path"]
                        .as_str()
                        .ok_or_else(PagesError::required("invalid coverage artifact"))?,
                )?;
                let root = portable_root
                    .as_ref()
                    .filter(|root| root.join(&path).exists())
                    .or(declared_root.as_ref())
                    .ok_or_else(PagesError::required("coverage artifact has no controlled report root"))?;
                let path = root.join(path).canonicalize()?;
                if !path.starts_with(root)
                    || artifact["sha256"] != artifact_digest(&path)?
                    || !path.join("index.html").is_file()
                {
                    return Err(PagesError::Config("coverage HTML is missing or hash mismatch".into()));
                }
                evidence.coverage_html = Some(path);
            }
            evidence.coverage = Some(report);
        }
    }
    if config.reports.ci_required && evidence.ci.is_none() {
        return Err(PagesError::Config("required CI quality-summary missing".into()));
    }
    if config.reports.coverage_required && (evidence.coverage.is_none() || evidence.coverage_html.is_none()) {
        return Err(PagesError::Config("required coverage report or HTML missing".into()));
    }
    Ok(evidence)
}
pub(crate) fn copy(evidence: &Evidence, site: &Path) -> Result<(), PagesError> {
    if let Some(ci) = &evidence.ci {
        write_json(&site.join("ci-summary.json"), ci)?;
    }
    if let Some(coverage) = &evidence.coverage {
        let percent = coverage["data"]["summary"]["lines"]["percent"]
            .as_f64()
            .ok_or_else(PagesError::required("coverage summary line percent missing"))?;
        write_json(
            &site.join("coverage-badge.json"),
            &json!({"schemaVersion":1,"label":"coverage","message":format!("{percent:.1}%"),"color":if percent>=80.0 {"brightgreen"}else{"orange"}}),
        )?;
    }
    if let Some(source) = &evidence.coverage_html {
        crate::site::copy_tree(source, &site.join("coverage"))?;
    }
    fs::write(site.join(".nojekyll"), "")?;
    Ok(())
}
