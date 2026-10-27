// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Canonical cross-tool input and artifact digests.
use std::fs;
use std::path::Path;
use std::path::PathBuf;

use sha2::Digest;
use sha2::Sha256;
use walkdir::WalkDir;

use crate::pages_error::PagesError;
pub(crate) fn files(root: &Path) -> Result<Vec<PathBuf>, PagesError> {
    let mut paths = Vec::new();
    for entry in WalkDir::new(root).follow_links(false) {
        let entry = entry.map_err(PagesError::traversal)?;
        if entry.file_type().is_symlink() {
            return Err(PagesError::Config(format!(
                "symbolic artifact: {}",
                entry.path().display()
            )));
        }
        if entry.file_type().is_file() {
            paths.push(
                entry
                    .path()
                    .strip_prefix(root)
                    .map_err(PagesError::configuration)?
                    .to_path_buf(),
            );
        }
    }
    paths.sort_by_key(|p| p.to_string_lossy().replace('\\', "/").into_bytes());
    Ok(paths)
}
pub(crate) fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}
pub(crate) fn digest_files(root: &Path, paths: &[PathBuf]) -> Result<String, PagesError> {
    let mut hash = Sha256::new();
    for path in paths {
        let name = path
            .to_str()
            .ok_or_else(PagesError::required("non-UTF8 input path"))?
            .replace('\\', "/");
        let bytes = fs::read(root.join(path))?;
        hash.update((name.len() as u64).to_le_bytes());
        hash.update(name.as_bytes());
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    }
    Ok(hash.finalize().iter().map(|byte| format!("{byte:02x}")).collect())
}
pub(crate) fn artifact_digest(path: &Path) -> Result<String, PagesError> {
    if path.is_dir() {
        digest_files(path, &files(path)?)
    } else {
        Ok(sha(&fs::read(path)?))
    }
}
pub(crate) fn project_digest(
    project: &Path,
    config: bool,
    report_root: &Path,
    exclusions: &[PathBuf],
) -> Result<String, PagesError> {
    let entries = WalkDir::new(project).follow_links(false).into_iter().filter_entry(|e| {
        if e.depth() == 0 {
            return true;
        }
        let relative = e.path().strip_prefix(project).ok();
        if e.path().starts_with(report_root) {
            return false;
        }
        if let Some(relative) = relative
            && let Some(first) = relative.components().next()
        {
            let name = first.as_os_str();
            if [".git", "build", "target"].iter().any(|s| name == *s) || (!config && name == ".infra") {
                return false;
            }
        }
        true
    });
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(PagesError::traversal)?;
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = entry.path().strip_prefix(project).map_err(PagesError::configuration)?;
        if !config && exclusions.iter().any(|excluded| excluded == rel) {
            continue;
        }
        if config {
            let first = rel.components().next().map(|c| c.as_os_str());
            if first != Some(std::ffi::OsStr::new(".infra"))
                && first != Some(std::ffi::OsStr::new("conan"))
                && !["CMakeLists.txt", "CMakePresets.json", "conanfile.py", "conanfile.txt"]
                    .iter()
                    .any(|s| rel == Path::new(s))
            {
                continue;
            }
            if [".infra/.tmp", ".infra/transactions", ".infra/.transactions"]
                .iter()
                .any(|prefix| rel.starts_with(prefix))
            {
                continue;
            }
        }
        paths.push(rel.to_path_buf());
    }
    paths.sort_by_key(|p| p.to_string_lossy().replace('\\', "/").into_bytes());
    digest_files(project, &paths)
}
