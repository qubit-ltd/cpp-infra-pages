// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Validated file-only exclusions for shared public report outputs.
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

use serde_json::Value;

use crate::cli::Cli;
use crate::config::relative;
use crate::pages_error::PagesError;
use crate::report::read_json;
fn required_error(message: &str) -> impl FnOnce() -> PagesError + '_ {
    move || invalid(message)
}
fn invalid(message: &str) -> PagesError {
    PagesError::Config(format!("unsafe report output exclusion: {message}"))
}
fn location(path: &Path) -> Result<PathBuf, PagesError> {
    let mut existing = path;
    let mut suffix = Vec::new();
    while !existing.exists() {
        suffix.push(
            existing
                .file_name()
                .ok_or_else(required_error("output has no filename"))?
                .to_os_string(),
        );
        existing = existing.parent().ok_or_else(required_error("output has no ancestor"))?;
    }
    let mut canonical = existing.canonicalize()?;
    for part in suffix.into_iter().rev() {
        canonical.push(part);
    }
    Ok(canonical)
}
fn overlaps(project: &Path, output: &Path, input: &Path) -> Result<bool, PagesError> {
    let output = project.join(output);
    let input = project.join(input);
    if output.starts_with(&input) || input.starts_with(&output) {
        return Ok(true);
    }
    let output = location(&output)?;
    let input = location(&input)?;
    Ok(output.starts_with(&input) || input.starts_with(&output))
}
fn protected(project: &Path, path: &Path) -> Result<bool, PagesError> {
    let path = path.strip_prefix(project).unwrap_or(path);
    let first = path
        .components()
        .next()
        .map(|c| c.as_os_str().to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if [
        ".infra",
        ".git",
        "conan",
        "src",
        "include",
        "test",
        "tests",
        "generated",
        "third_party",
        "vendor",
    ]
    .contains(&first.as_str())
        || [
            "CMakePresets.json",
            "CMakeUserPresets.json",
            "compile_commands.json",
            "conan.lock",
        ]
        .iter()
        .any(|name| path == Path::new(name))
    {
        return Ok(true);
    }
    for (file, keys) in [
        (
            ".infra/project.toml",
            vec!["source_roots", "test_roots", "generated_roots", "third_party_roots"],
        ),
        (".infra/pages/config.toml", vec![]),
    ] {
        let configuration_path = project.join(file);
        if !configuration_path.is_file() {
            continue;
        }
        let file = crate::config::tracked_policy(project, file)?;
        let document: toml::Value = toml::from_str(&fs::read_to_string(file)?).map_err(PagesError::configuration)?;
        for key in keys {
            if let Some(roots) = document.get(key) {
                for root in roots
                    .as_array()
                    .ok_or_else(required_error("project roots must be arrays"))?
                {
                    let root = relative(
                        root.as_str()
                            .ok_or_else(required_error("project root must be string"))?,
                    )?;
                    if overlaps(project, path, &root)? {
                        return Ok(true);
                    }
                }
            }
        }
        if let Some(languages) = document.get("languages").and_then(toml::Value::as_table) {
            for language in languages.values() {
                if let Some(readme) = language.get("readme").and_then(toml::Value::as_str)
                    && overlaps(project, path, &relative(readme)?)?
                {
                    return Ok(true);
                }
            }
        }
        if let Some(inputs) = document
            .get("api")
            .and_then(|v| v.get("inputs"))
            .and_then(toml::Value::as_array)
        {
            for input in inputs {
                if overlaps(
                    project,
                    path,
                    &relative(input.as_str().ok_or_else(required_error("API input must be string"))?)?,
                )? {
                    return Ok(true);
                }
            }
        }
        if let Some(assets) = document.get("assets").and_then(toml::Value::as_array) {
            for asset in assets {
                if let Some(source) = asset.get("source").and_then(toml::Value::as_str)
                    && overlaps(project, path, &relative(source)?)?
                {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}
fn family_report(path: &Path) -> Result<(), PagesError> {
    if !path.is_file() {
        return Err(invalid("existing output is not a regular file"));
    }
    let mut report = read_json(path)?;
    if !report["tool"]["name"].as_str().is_some_and(|name| {
        [
            "cpp-infra-tools",
            "cpp-infra-verify",
            "cpp-infra-style",
            "cpp-infra-analyze",
            "cpp-infra-dependency",
            "cpp-infra-coverage",
            "cpp-infra-ci",
            "cpp-infra-pages",
        ]
        .contains(&name)
    }) {
        return Err(invalid("existing file is not a cpp-infra report"));
    }
    for key in ["source_digest", "config_digest"] {
        if report[key].is_null() {
            report[key] = Value::String("0".repeat(64));
        }
    }
    crate::report_validation::common(&report)
}
pub(crate) fn validate_path(project: &Path, value: &str) -> Result<PathBuf, PagesError> {
    if value
        .as_bytes()
        .get(..2)
        .is_some_and(|prefix| prefix[0].is_ascii_alphabetic() && prefix[1] == b':')
    {
        return Err(invalid("drive-prefixed paths are forbidden on every host"));
    }
    if value
        .split('/')
        .any(|component| component.is_empty() || component == "." || component == "..")
    {
        return Err(invalid("exclusion must be a normalized file path"));
    }
    let path = relative(value)?;
    let normalized = path
        .components()
        .map(|component| {
            component
                .as_os_str()
                .to_str()
                .ok_or_else(required_error("non UTF8 path"))
        })
        .collect::<Result<Vec<_>, _>>()?
        .join("/");
    if normalized != value {
        return Err(invalid("exclusion must exactly match normalized path components"));
    }
    if path.extension().is_none_or(|extension| extension != "json") || protected(project, &path)? {
        return Err(invalid("path is a protected input or not JSON"));
    }
    let mut cursor = project.to_path_buf();
    for component in path.components() {
        cursor.push(component);
        if fs::symlink_metadata(&cursor).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
            return Err(invalid("symbolic aliases are not allowed"));
        }
    }
    let full = project.join(&path);
    if full.exists() {
        family_report(&full)?;
    }
    Ok(path)
}
pub(crate) fn validate_output(project: &Path, output: &Path) -> Result<Option<PathBuf>, PagesError> {
    let absolute = if output.is_absolute() {
        output.to_path_buf()
    } else {
        std::env::current_dir()?.join(output)
    };
    if fs::symlink_metadata(&absolute).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(invalid("report output must not be a symbolic link"));
    }
    let canonical = location(&absolute)?;
    if protected(project, &canonical)? {
        return Err(invalid("report output overlaps a protected input"));
    }
    if let Ok(relative_path) = canonical.strip_prefix(project) {
        let first = relative_path.components().next().map(|c| c.as_os_str());
        if ["build", "target"]
            .iter()
            .any(|name| first == Some(std::ffi::OsStr::new(name)))
        {
            return Ok(None);
        }
        return validate_path(
            project,
            relative_path.to_str().ok_or_else(required_error("non UTF8 output"))?,
        )
        .map(Some);
    }
    Ok(None)
}
fn merge(project: &Path, value: &Value, paths: &mut BTreeSet<PathBuf>) -> Result<(), PagesError> {
    if let Some(exclusions) = value.get("source_excluded_paths") {
        for entry in exclusions
            .as_array()
            .ok_or_else(required_error("exclusions must be an array"))?
        {
            paths.insert(validate_path(
                project,
                entry.as_str().ok_or_else(required_error("exclusion must be string"))?,
            )?);
        }
    }
    Ok(())
}
pub(crate) fn resolve(cli: &Cli, project: &Path, context: Option<&Value>) -> Result<Vec<PathBuf>, PagesError> {
    let mut paths = BTreeSet::new();
    if let Some(context) = context {
        merge(project, context, &mut paths)?;
    }
    if let Some(directory) = &cli.input_dir {
        let directory = directory.canonicalize()?;
        for path in crate::identity::files(&directory)? {
            if path.extension().is_some_and(|extension| extension == "json") {
                let report = read_json(&directory.join(path))?;
                if report.get("source_excluded_paths").is_some() {
                    crate::report_validation::common(&report)?;
                    merge(project, &report, &mut paths)?;
                }
            }
        }
    }
    if let Some(context) = context
        && let Some(artifacts) = context["artifacts"].as_array()
    {
        for artifact in artifacts {
            if matches!(artifact["kind"].as_str(), Some("quality-summary" | "coverage-summary")) {
                let path = Path::new(artifact["path"].as_str().ok_or_else(required_error("artifact path"))?);
                let root = project
                    .join("build/infra-runs")
                    .join(context["run_id"].as_str().ok_or_else(required_error("run id"))?)
                    .canonicalize()?;
                let path = if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    root.join(path)
                };
                let path = path.canonicalize()?;
                if !path.starts_with(&root) || artifact["sha256"] != crate::identity::artifact_digest(&path)? {
                    return Err(invalid("context report path or digest mismatch"));
                }
                let report = read_json(&path)?;
                crate::report_validation::common(&report)?;
                merge(project, &report, &mut paths)?;
            }
        }
    }
    if let Some(output) = &cli.report
        && let Some(path) = validate_output(project, output)?
    {
        let full = project.join(&path);
        if full.is_file() {
            merge(project, &read_json(&full)?, &mut paths)?;
        }
        paths.insert(path);
    }
    let mut paths = paths.into_iter().collect::<Vec<_>>();
    paths.sort_by_key(|path| path.to_string_lossy().replace('\\', "/").into_bytes());
    Ok(paths)
}
