// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Common schema-1 reports and authenticated run contexts.
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use chrono::Utc;
use serde_json::Value;
use serde_json::json;

use crate::cli::Cli;
use crate::identity::artifact_digest;
use crate::identity::project_digest;
use crate::identity::sha;
use crate::pages_error::PagesError;
static COUNTER: AtomicU64 = AtomicU64::new(0);
pub(crate) struct Run {
    pub project: PathBuf,
    pub root: PathBuf,
    pub work: PathBuf,
    pub report: Value,
    pub inputs: Vec<Value>,
}
pub(crate) fn tool() -> Value {
    json!({"name":"cpp-infra-pages","version":env!("CARGO_PKG_VERSION"),"revision":option_env!("CPP_INFRA_BUILD_REVISION")})
}
pub(crate) fn read_json(path: &Path) -> Result<Value, PagesError> {
    serde_json::from_slice(&fs::read(path)?)
        .map_err(|e| PagesError::Config(format!("invalid JSON {}: {e}", path.display())))
}
pub(crate) fn reject_symlink(path: &Path) -> Result<(), PagesError> {
    if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(PagesError::Config(format!(
            "symbolic output is not allowed: {}",
            path.display()
        )));
    }
    Ok(())
}
pub(crate) fn write_json(path: &Path, value: &Value) -> Result<(), PagesError> {
    let parent = path.parent().ok_or_else(PagesError::required("output has no parent"))?;
    fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer_pretty(&mut temp, value).map_err(std::io::Error::other)?;
    temp.persist(path).map_err(PagesError::persistence)?;
    Ok(())
}
fn source_revision(project: &Path) -> Result<Option<String>, PagesError> {
    let marker = project.join(".git");
    let git_dir = if marker.is_dir() {
        marker
    } else if marker.is_file() {
        let contents = fs::read_to_string(&marker)?;
        let path = contents
            .trim()
            .strip_prefix("gitdir: ")
            .ok_or_else(PagesError::required("invalid .git indirection"))?;
        let path = Path::new(path);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            project.join(path)
        }
    } else {
        return Ok(None);
    };
    let head = match fs::read_to_string(git_dir.join("HEAD")) {
        Ok(head) => head,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let valid = |value: &str| value.len() == 40 && value.bytes().all(|b| b.is_ascii_hexdigit());
    if valid(head.trim()) {
        return Ok(Some(head.trim().to_owned()));
    }
    let Some(reference) = head.trim().strip_prefix("ref: ") else {
        return Err(PagesError::Config("invalid Git HEAD".into()));
    };
    let relative = crate::config::relative(reference)?;
    let mut roots = vec![git_dir.clone()];
    if let Ok(common) = fs::read_to_string(git_dir.join("commondir")) {
        roots.push(git_dir.join(common.trim()));
    }
    for root in roots {
        if let Ok(revision) = fs::read_to_string(root.join(&relative))
            && valid(revision.trim())
        {
            return Ok(Some(revision.trim().to_owned()));
        }
        if let Ok(packed) = fs::read_to_string(root.join("packed-refs")) {
            for line in packed.lines() {
                if let Some((revision, name)) = line.split_once(' ')
                    && name == reference
                    && valid(revision)
                {
                    return Ok(Some(revision.to_owned()));
                }
            }
        }
    }
    Ok(None)
}
impl Run {
    pub(crate) fn create(cli: &Cli) -> Result<Self, PagesError> {
        let project = cli.project.canonicalize()?;
        if !project.is_dir() {
            return Err(PagesError::Config("project must be a directory".into()));
        }
        let context = cli.run_context.as_ref().map(|p| read_json(p)).transpose()?;
        let run_id = context
            .as_ref()
            .and_then(|v| v["run_id"].as_str())
            .map(str::to_owned)
            .unwrap_or_else(|| {
                format!(
                    "{}-{}-{}",
                    Utc::now().format("%Y%m%dT%H%M%S%f"),
                    std::process::id(),
                    COUNTER.fetch_add(1, Ordering::Relaxed)
                )
            });
        if run_id.is_empty() || !run_id.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b)) {
            return Err(PagesError::Config("invalid run_id".into()));
        }
        let root = project.join("build/infra-runs").join(&run_id);
        let mut cursor = project.clone();
        for component in root
            .strip_prefix(&project)
            .map_err(PagesError::configuration)?
            .components()
        {
            cursor.push(component);
            if cursor.exists() && fs::symlink_metadata(&cursor)?.file_type().is_symlink() {
                return Err(PagesError::Config("run root contains a symbolic link".into()));
            }
        }
        fs::create_dir_all(&root)?;
        let root = root.canonicalize()?;
        if !root.starts_with(&project) {
            return Err(PagesError::Config("run root escapes project".into()));
        }
        let exclusions = crate::source_exclusions::resolve(cli, &project, context.as_ref())?;
        let source_digest = project_digest(&project, false, &root, &exclusions)?;
        let config_digest = project_digest(&project, true, &root, &[])?;
        let mut inputs = Vec::new();
        let mut bundle = "development".to_owned();
        let mut task = "pages".to_owned();
        if let Some(c) = &context {
            if c["schema"] != 1
                || c["project"].as_str() != project.to_str()
                || c["config_digest"] != config_digest
                || c["source_digest"] != source_digest
                || c["report_root"].as_str() != root.to_str()
            {
                return Err(PagesError::Config(
                    "run-context identity, digest or report_root mismatch".into(),
                ));
            }
            if !matches!(c.get("profile"), Some(Value::Null))
                && c.get("profile").and_then(Value::as_str).is_none_or(str::is_empty)
            {
                return Err(PagesError::Config("run-context profile missing".into()));
            }
            bundle = c["bundle_id"]
                .as_str()
                .ok_or_else(PagesError::required("missing bundle_id"))?
                .to_owned();
            if bundle != "development"
                && !(bundle
                    .strip_prefix("main-")
                    .is_some_and(|digest| digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())))
            {
                return Err(PagesError::Config("invalid production main bundle".into()));
            }
            if bundle != "development"
                && !tool()["revision"].as_str().is_some_and(|revision| {
                    revision.len() == 40 && revision.bytes().all(|byte| byte.is_ascii_hexdigit())
                })
            {
                return Err(PagesError::Config(
                    "production main requires a released 40-hex tool revision".into(),
                ));
            }
            task = c["task_id"]
                .as_str()
                .ok_or_else(PagesError::required("missing task_id"))?
                .to_owned();
            match (c["runtime_snapshot"].as_str(), c["runtime_snapshot_digest"].as_str()) {
                (Some(path), Some(expected)) => {
                    let path = Path::new(path);
                    if !path.is_absolute() {
                        return Err(PagesError::Config("runtime_snapshot must be absolute".into()));
                    }
                    let bytes = fs::read(path)?;
                    if sha(&bytes) != expected {
                        return Err(PagesError::Config("runtime snapshot digest mismatch".into()));
                    }
                    let snapshot: Value = serde_json::from_slice(&bytes).map_err(PagesError::configuration)?;
                    if snapshot["schema"] != 1 || snapshot["bundle_id"] != bundle {
                        return Err(PagesError::Config("runtime snapshot bundle or schema mismatch".into()));
                    }
                    let own = tool();
                    let entries = snapshot["tools"]
                        .as_array()
                        .ok_or_else(PagesError::required("runtime snapshot tools missing"))?;
                    let matches = entries.iter().filter(|t| t["name"] == own["name"]).collect::<Vec<_>>();
                    if matches.len() != 1 {
                        return Err(PagesError::Config(
                            "runtime snapshot must identify this tool exactly once".into(),
                        ));
                    }
                    if !snapshot["offline"].is_boolean() || !snapshot["stale"].is_boolean() {
                        return Err(PagesError::Config("runtime snapshot flags must be booleans".into()));
                    }
                    let mut names = std::collections::BTreeSet::new();
                    for item in entries {
                        let name = item["name"]
                            .as_str()
                            .filter(|name| !name.is_empty())
                            .ok_or_else(PagesError::required("snapshot tool name missing"))?;
                        let binary = item["binary"]
                            .as_str()
                            .ok_or_else(PagesError::required("snapshot binary missing"))?;
                        let binary = Path::new(binary);
                        if !names.insert(name)
                            || !binary.is_absolute()
                            || !binary.is_file()
                            || item["version"].as_str().is_none()
                            || item["protocol"] != 1
                            || (bundle != "development"
                                && !item["revision"]
                                    .as_str()
                                    .is_some_and(|r| r.len() == 40 && r.bytes().all(|b| b.is_ascii_hexdigit())))
                            || item["executable_sha256"] != artifact_digest(binary)?
                        {
                            return Err(PagesError::Config(
                                "runtime snapshot binary is missing or incompatible".into(),
                            ));
                        }
                    }
                    let entry = matches[0];
                    if entry["version"] != own["version"]
                        || entry["revision"] != own["revision"]
                        || entry["protocol"] != 1
                        || entry["executable_sha256"] != artifact_digest(&std::env::current_exe()?)?
                    {
                        return Err(PagesError::Config(
                            "runtime snapshot executable identity mismatch".into(),
                        ));
                    }
                }
                (None, None) if bundle == "development" => {}
                _ => {
                    return Err(PagesError::Config(
                        "runtime snapshot path and digest are required together".into(),
                    ));
                }
            }
            if let Some(artifacts) = c["artifacts"].as_array() {
                for artifact in artifacts {
                    let path = Path::new(
                        artifact["path"]
                            .as_str()
                            .ok_or_else(PagesError::required("invalid artifact path"))?,
                    );
                    let path = if path.is_absolute() {
                        path.to_path_buf()
                    } else {
                        root.join(path)
                    };
                    let path = path.canonicalize()?;
                    if !path.starts_with(&root) || artifact["sha256"] != artifact_digest(&path)? {
                        return Err(PagesError::Config("context artifact path or hash mismatch".into()));
                    }
                    inputs.push(json!({"kind":artifact["kind"],"path":path}));
                }
            }
        }
        if let Some(input_dir) = &cli.input_dir {
            let dir = input_dir.canonicalize()?;
            for path in crate::identity::files(&dir)? {
                if path.extension().is_some_and(|s| s == "json") {
                    inputs.push(json!({"kind":"report","path":dir.join(path)}));
                }
            }
        }
        let pages = root.join("pages");
        reject_symlink(&pages)?;
        fs::create_dir_all(&pages)?;
        let work = tempfile::Builder::new().prefix("call-").tempdir_in(&pages)?.keep();
        let revision = source_revision(&project)?;
        let report = json!({"schema":1,"tool":tool(),"bundle_id":bundle,"run_id":run_id,"task_id":task,"project":project,"report_root":root,"source_revision":revision,"source_excluded_paths":exclusions.iter().map(|path|path.to_string_lossy().replace('\\',"/")).collect::<Vec<_>>(),"source_digest":source_digest,"config_digest":config_digest,"status":"error","started_at":Utc::now().to_rfc3339(),"duration_ms":0,"external_versions":{},"commands":[],"diagnostics":[],"artifacts":[],"data":{"offline":cli.offline,"profile":context.as_ref().map(|c|c["profile"].clone()).unwrap_or(json!("docs"))}});
        Ok(Self {
            project,
            root,
            work,
            report,
            inputs,
        })
    }
    pub(crate) fn fresh(&self) -> Result<(), PagesError> {
        let configuration = crate::config::Config::load(&self.project)?;
        if let Some(expected) = self.report["data"]["api_input_digest"].as_str() {
            crate::api::inputs_fresh(&configuration, self, expected)?;
        }
        let exclusions = self.report["source_excluded_paths"]
            .as_array()
            .ok_or_else(PagesError::required("missing source exclusion list"))?
            .iter()
            .map(|value| {
                crate::source_exclusions::validate_path(
                    &self.project,
                    value
                        .as_str()
                        .ok_or_else(PagesError::required("invalid source exclusion"))?,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        if self.report["source_digest"] != project_digest(&self.project, false, &self.root, &exclusions)?
            || self.report["config_digest"] != project_digest(&self.project, true, &self.root, &[])?
        {
            return Err(PagesError::Config(
                "project inputs changed during pages invocation".into(),
            ));
        }
        Ok(())
    }
    pub(crate) fn add_artifact(&mut self, kind: &str, path: &Path) -> Result<(), PagesError> {
        let rel = path.strip_prefix(&self.root).map_err(PagesError::configuration)?;
        let artifact = json!({"kind":kind,"path":rel.to_string_lossy().replace('\\',"/"),"sha256":artifact_digest(path)?,"required":true});
        self.report["artifacts"]
            .as_array_mut()
            .ok_or_else(PagesError::required("invalid report artifacts"))?
            .push(artifact);
        Ok(())
    }
}
