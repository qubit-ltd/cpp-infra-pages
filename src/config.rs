// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Strict project-owned site configuration.
use std::collections::BTreeMap;
use std::fs;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;

use crate::pages_error::PagesError;
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Config {
    pub schema: u32,
    pub title: String,
    pub default_language: String,
    pub base_path: String,
    pub languages: BTreeMap<String, Language>,
    pub api: Api,
    pub reports: Reports,
    #[serde(default)]
    pub assets: Vec<Asset>,
    pub repository: Option<String>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Language {
    pub readme: String,
    pub label: String,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Api {
    pub enabled: bool,
    #[serde(default)]
    pub inputs: Vec<String>,
    #[serde(default = "yes")]
    pub warnings_as_errors: bool,
    #[serde(default)]
    pub defines: Vec<String>,
    #[serde(default = "timeout")]
    pub timeout_seconds: u64,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Reports {
    pub coverage_required: bool,
    pub ci_required: bool,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Asset {
    pub source: String,
    pub output: String,
}
fn yes() -> bool {
    true
}
fn timeout() -> u64 {
    120
}
pub(crate) fn relative(value: &str) -> Result<PathBuf, PagesError> {
    if value.is_empty() || value.contains('\\') || value.contains(['\n', '\r', '\0']) {
        return Err(PagesError::Config(format!("unsafe relative path: {value}")));
    }
    let p = Path::new(value);
    if p.components().any(|c| !matches!(c, Component::Normal(_))) {
        return Err(PagesError::Config(format!("unsafe relative path: {value}")));
    }
    Ok(p.to_path_buf())
}
pub(crate) fn input(project: &Path, value: &str) -> Result<PathBuf, PagesError> {
    let rel = relative(value)?;
    let path = project.join(rel);
    let real = path
        .canonicalize()
        .map_err(|e| PagesError::Config(format!("missing input {value}: {e}")))?;
    if !real.starts_with(project) {
        return Err(PagesError::Config(format!("input escapes project: {value}")));
    }
    let mut cursor = project.to_path_buf();
    for c in Path::new(value).components() {
        cursor.push(c);
        if fs::symlink_metadata(&cursor)?.file_type().is_symlink() {
            return Err(PagesError::Config(format!("symbolic input is not allowed: {value}")));
        }
    }
    Ok(real)
}
fn excluded_from_identity(path: &Path) -> bool {
    path.components().next().is_some_and(|component| {
        [".git", "build", "target"]
            .iter()
            .any(|root| component.as_os_str() == *root)
    }) || [".infra/.tmp", ".infra/transactions", ".infra/.transactions"]
        .iter()
        .any(|root| path.starts_with(root))
}
fn tracked_path(project: &Path, value: &str) -> Result<PathBuf, PagesError> {
    if excluded_from_identity(&relative(value)?) {
        return Err(PagesError::Config(format!(
            "input is outside tracked project identity: {value}"
        )));
    }
    let canonical = input(project, value)?;
    if excluded_from_identity(canonical.strip_prefix(project).map_err(PagesError::configuration)?) {
        return Err(PagesError::Config(format!(
            "input target is outside tracked project identity: {value}"
        )));
    }
    Ok(canonical)
}
pub(crate) fn tracked_policy(project: &Path, value: &str) -> Result<PathBuf, PagesError> {
    let canonical = tracked_path(project, value)?;
    if !canonical.is_file() {
        return Err(PagesError::Config("policy input is not a tracked regular file".into()));
    }
    Ok(canonical)
}
fn tracked_content(project: &Path, value: &str) -> Result<PathBuf, PagesError> {
    let canonical = tracked_path(project, value)?;
    if canonical.is_dir() {
        for relative_path in crate::identity::files(&canonical)? {
            let content = canonical.join(relative_path);
            if excluded_from_identity(content.strip_prefix(project).map_err(PagesError::configuration)?) {
                return Err(PagesError::Config(format!(
                    "asset directory contains untracked content: {}",
                    content.display()
                )));
            }
        }
    } else if !canonical.is_file() {
        return Err(PagesError::Config(
            "content input must be a regular file or directory".into(),
        ));
    }
    Ok(canonical)
}
impl Config {
    pub(crate) fn load(project: &Path) -> Result<Self, PagesError> {
        let path = tracked_policy(project, ".infra/pages/config.toml")?;
        for optional in [".infra/project.toml", ".infra/pages/policy/Doxyfile.in"] {
            if fs::symlink_metadata(project.join(optional)).is_ok() {
                tracked_policy(project, optional)?;
            }
        }
        let config: Self = toml::from_str(&fs::read_to_string(path)?).map_err(PagesError::configuration)?;
        if config.schema != 1 || !config.languages.contains_key(&config.default_language) {
            return Err(PagesError::Config(
                "unsupported schema or missing default language".into(),
            ));
        }
        let base = &config.base_path;
        if !base.starts_with('/')
            || !base.ends_with('/')
            || base.contains(['\\', ':', '?', '#', '%'])
            || base.starts_with("//")
            || base.split('/').any(|s| s == ".." || s == ".")
        {
            return Err(PagesError::Config(
                "base_path must be an absolute site path ending in /".into(),
            ));
        }
        for (id, language) in &config.languages {
            if [
                "api",
                "coverage",
                "assets",
                "pages",
                "cache",
                "index.html",
                "site-manifest.json",
                "ci-summary.json",
                "coverage-badge.json",
            ]
            .contains(&id.to_ascii_lowercase().as_str())
                || id.is_empty()
                || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            {
                return Err(PagesError::Config(format!("invalid language identifier {id}")));
            }
            if !tracked_content(project, &language.readme)?.is_file() {
                return Err(PagesError::Config("language README must be a file".into()));
            }
        }
        for asset in &config.assets {
            tracked_content(project, &asset.source)?;
            relative(&asset.output)?;
        }
        if config.api.enabled && (config.api.inputs.is_empty() || config.api.timeout_seconds == 0) {
            return Err(PagesError::Config(
                "enabled API needs inputs and a positive timeout".into(),
            ));
        }
        for path in &config.api.inputs {
            let directory = input(project, path)?;
            crate::identity::files(&directory)?;
            if !directory.is_dir() {
                return Err(PagesError::Config(
                    "API inputs must be project-owned directories".into(),
                ));
            }
        }
        for define in &config.api.defines {
            if define.is_empty()
                || !define
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_=().,+- ".contains(&b))
            {
                return Err(PagesError::Config("unsupported preprocessor definition".into()));
            }
        }
        if let Some(repository) = &config.repository {
            let url = url::Url::parse(repository).map_err(PagesError::configuration)?;
            if url.scheme() != "https"
                || url.host_str() != Some("github.com")
                || url.query().is_some()
                || url.fragment().is_some()
            {
                return Err(PagesError::Config("repository must be an https GitHub URL".into()));
            }
        }
        Ok(config)
    }
}
