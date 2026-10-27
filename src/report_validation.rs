// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Strict common-report and quality-summary input validation.
use std::collections::BTreeSet;

use serde_json::Value;

use crate::pages_error::PagesError;
fn required_error(message: &str) -> impl FnOnce() -> PagesError + '_ {
    move || invalid(message)
}
fn invalid(message: &str) -> PagesError {
    PagesError::Config(format!("invalid report protocol: {message}"))
}
fn object<'a>(
    value: &'a Value,
    required: &[&str],
    optional: &[&str],
) -> Result<&'a serde_json::Map<String, Value>, PagesError> {
    let map = value.as_object().ok_or_else(required_error("expected object"))?;
    if required.iter().any(|key| !map.contains_key(*key))
        || map
            .keys()
            .any(|key| !required.contains(&key.as_str()) && !optional.contains(&key.as_str()))
    {
        return Err(invalid("missing or unknown object fields"));
    }
    Ok(map)
}
fn strings(value: &Value, keys: &[&str]) -> Result<(), PagesError> {
    if keys.iter().any(|key| value[*key].as_str().is_none()) {
        return Err(invalid("expected string field"));
    }
    Ok(())
}
fn array<'a>(value: &'a Value, key: &str) -> Result<&'a Vec<Value>, PagesError> {
    value[key].as_array().ok_or_else(required_error("expected array field"))
}
fn hash(value: &Value) -> bool {
    value
        .as_str()
        .is_some_and(|v| v.len() == 64 && v.bytes().all(|b| b.is_ascii_hexdigit()))
}
pub(crate) fn common(value: &Value) -> Result<(), PagesError> {
    object(
        value,
        &[
            "schema",
            "tool",
            "bundle_id",
            "run_id",
            "task_id",
            "project",
            "report_root",
            "source_revision",
            "source_digest",
            "config_digest",
            "status",
            "started_at",
            "duration_ms",
            "external_versions",
            "commands",
            "diagnostics",
            "artifacts",
        ],
        &["data", "source_excluded_paths"],
    )?;
    if value.get("source_excluded_paths").is_some_and(|value| {
        value
            .as_array()
            .is_none_or(|paths| paths.iter().any(|path| !path.is_string()))
    }) {
        return Err(invalid("source_excluded_paths must contain strings"));
    }
    if value["schema"] != 1 {
        return Err(invalid("unsupported schema"));
    }
    strings(
        value,
        &[
            "bundle_id",
            "run_id",
            "task_id",
            "project",
            "report_root",
            "status",
            "started_at",
        ],
    )?;
    if value["duration_ms"].as_u64().is_none()
        || !value["external_versions"].is_object()
        || !["passed", "failed", "error", "cancelled", "skipped"]
            .iter()
            .any(|status| value["status"] == *status)
    {
        return Err(invalid("invalid report metadata"));
    }
    if !value["source_revision"].is_null() && !value["source_revision"].is_string() {
        return Err(invalid("invalid source revision"));
    }
    if !hash(&value["source_digest"]) || !hash(&value["config_digest"]) {
        return Err(invalid("quality evidence needs complete input hashes"));
    }
    object(&value["tool"], &["name", "version", "revision"], &["protocol"])?;
    strings(&value["tool"], &["name", "version"])?;
    if (!value["tool"]["revision"].is_null() && !value["tool"]["revision"].is_string())
        || value["tool"].get("protocol").is_some_and(|v| *v != 1)
    {
        return Err(invalid("invalid tool identity"));
    }
    for command in array(value, "commands")? {
        object(
            command,
            &["program", "args", "cwd", "env", "exit_code", "log", "duration_ms"],
            &["timed_out", "cancelled", "stderr_log", "termination"],
        )?;
        for key in ["timed_out", "cancelled"] {
            if command.get(key).is_some_and(|value| !value.is_boolean()) {
                return Err(invalid("command flags must be booleans"));
            }
        }
        if command.get("stderr_log").is_some_and(|value| !value.is_string())
            || command
                .get("termination")
                .is_some_and(|value| !matches!(value.as_str(), Some("timeout" | "cancelled")))
        {
            return Err(invalid("invalid command termination metadata"));
        }
        strings(command, &["program", "cwd", "log"])?;
        if array(command, "args")?.iter().any(|v| !v.is_string())
            || command["duration_ms"].as_u64().is_none()
            || (!command["exit_code"].is_null() && command["exit_code"].as_i64().is_none())
            || command["env"]
                .as_object()
                .is_none_or(|env| env.values().any(|v| !v.is_string()))
        {
            return Err(invalid("invalid command record"));
        }
    }
    for artifact in array(value, "artifacts")? {
        object(artifact, &["kind", "path", "sha256", "required"], &[])?;
        strings(artifact, &["kind", "path"])?;
        if !hash(&artifact["sha256"]) || !artifact["required"].is_boolean() {
            return Err(invalid("invalid artifact record"));
        }
    }
    for diagnostic in array(value, "diagnostics")? {
        if !diagnostic.is_object()
            || ["code", "severity", "message", "project_relative_path"]
                .iter()
                .any(|key| diagnostic.get(*key).is_none())
        {
            return Err(invalid("invalid diagnostic record"));
        }
    }
    Ok(())
}
pub(crate) fn ci(value: &Value) -> Result<(), PagesError> {
    if value["tool"]["name"] != "cpp-infra-ci" || value["data"]["phase"] != "quality" {
        return Err(invalid("quality-summary must identify cpp-infra-ci phase quality"));
    }
    let data = &value["data"];
    if !data["complete"].is_boolean() || data.get("incomplete_remote_coverage").is_some_and(|v| !v.is_boolean()) {
        return Err(invalid("CI completion flags must be booleans"));
    }
    let tasks = array(data, "tasks")?;
    let expected = array(data, "expected_task_ids")?;
    let mut ids = BTreeSet::new();
    for task in tasks {
        object(task, &["task_id", "status", "required"], &["report"])?;
        strings(task, &["task_id", "status"])?;
        let id = task["task_id"].as_str().ok_or_else(required_error("task id"))?;
        if id.is_empty()
            || !ids.insert(id)
            || !task["required"].is_boolean()
            || !["passed", "failed", "error", "cancelled", "skipped", "blocked"]
                .iter()
                .any(|s| task["status"] == *s)
        {
            return Err(invalid("invalid or duplicate task record"));
        }
    }
    let mut expected_ids = BTreeSet::new();
    for id in expected {
        let id = id
            .as_str()
            .filter(|id| !id.is_empty())
            .ok_or_else(required_error("expected task id must be a nonempty string"))?;
        if !expected_ids.insert(id) {
            return Err(invalid("duplicate expected task id"));
        }
    }
    Ok(())
}
