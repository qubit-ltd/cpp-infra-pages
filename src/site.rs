// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Transactional static site composition and reference validation.
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fs;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

use pulldown_cmark::Options;
use pulldown_cmark::Parser;
use pulldown_cmark::html;
use regex::Captures;
use regex::Regex;
use serde_json::Value;
use serde_json::json;

use crate::config::Config;
use crate::config::input;
use crate::config::relative;
use crate::identity::artifact_digest;
use crate::identity::files;
use crate::identity::sha;
use crate::pages_error::PagesError;
use crate::report::Run;
use crate::report::write_json;
use crate::reports::Evidence;
pub(crate) fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
pub(crate) fn copy_tree(source: &Path, target: &Path) -> Result<(), PagesError> {
    let list = if source.is_dir() {
        files(source)?
    } else {
        vec![PathBuf::from(
            source
                .file_name()
                .ok_or_else(PagesError::required("missing filename"))?,
        )]
    };
    for rel in list {
        crate::api::ensure_active()?;
        let (from, to) = if source.is_dir() {
            (source.join(&rel), target.join(&rel))
        } else {
            (source.to_path_buf(), target.to_path_buf())
        };
        if to.exists() {
            return Err(PagesError::Config(format!("asset output collision: {}", to.display())));
        }
        fs::create_dir_all(to.parent().ok_or_else(PagesError::required("asset has no parent"))?)?;
        fs::copy(from, to)?;
    }
    Ok(())
}
fn normalized(path: &Path) -> Result<PathBuf, PagesError> {
    let mut result = PathBuf::new();
    for c in path.components() {
        match c {
            Component::Normal(s) => result.push(s),
            Component::CurDir => {}
            Component::ParentDir => {
                if !result.pop() {
                    return Err(PagesError::Config("link escapes project".into()));
                }
            }
            _ => return Err(PagesError::Config("absolute local link".into())),
        }
    }
    Ok(result)
}
fn rewrite(url: &str, readme: &str, config: &Config, run: &Run) -> Result<String, PagesError> {
    if url.starts_with('#') || url.starts_with("mailto:") {
        return Ok(url.to_owned());
    }
    if url.starts_with("http:") || url.starts_with("https:") {
        let parsed = url::Url::parse(url).map_err(|e| PagesError::Gate(format!("invalid external URL: {e}")))?;
        if parsed.host_str().is_none() {
            return Err(PagesError::Gate("external link has no host".into()));
        }
        return Ok(url.to_owned());
    }
    if url.starts_with('/') {
        if !url.starts_with(&config.base_path) {
            return Err(PagesError::Gate(format!("internal URL outside base_path: {url}")));
        }
        return Ok(url.to_owned());
    }
    if url.contains(':') || url.contains('\\') {
        return Err(PagesError::Gate(format!("unsupported URL: {url}")));
    }
    let (local, suffix) = match url.find(['?', '#']) {
        Some(i) => (&url[..i], &url[i..]),
        None => (url, ""),
    };
    let local = percent_decode(local)?;
    let source = normalized(&Path::new(readme).parent().unwrap_or(Path::new("")).join(&local))?;
    for (id, language) in &config.languages {
        if source == Path::new(&language.readme) {
            return Ok(format!("{}{id}/{suffix}", config.base_path));
        }
    }
    for asset in &config.assets {
        let from = Path::new(&asset.source);
        if source == from || source.starts_with(from) {
            let output = if input(&run.project, &asset.source)?.is_dir() {
                Path::new(&asset.output).join(source.strip_prefix(from).map_err(PagesError::configuration)?)
            } else {
                PathBuf::from(&asset.output)
            };
            return Ok(format!("{}{}{suffix}", config.base_path, output.to_string_lossy()));
        }
    }
    let source_file = run.project.join(&source);
    if source_file.is_file() {
        let repository = config
            .repository
            .as_ref()
            .ok_or_else(|| PagesError::Config(format!("source link requires repository: {}", source.display())))?;
        let revision = run.report["source_revision"]
            .as_str()
            .ok_or_else(PagesError::required("source link requires a committed revision"))?;
        return Ok(format!(
            "{}/blob/{revision}/{}{suffix}",
            repository.trim_end_matches('/'),
            source.to_string_lossy()
        ));
    }
    Ok(format!("{}{}{suffix}", config.base_path, source.to_string_lossy()))
}
fn percent_decode(value: &str) -> Result<String, PagesError> {
    let mut bytes = Vec::new();
    let mut it = value.bytes();
    while let Some(b) = it.next() {
        if b == b'%' {
            let a = it.next().and_then(|b| (b as char).to_digit(16));
            let c = it.next().and_then(|b| (b as char).to_digit(16));
            match (a, c) {
                (Some(a), Some(c)) => bytes.push((a * 16 + c) as u8),
                _ => return Err(PagesError::Gate("invalid URL encoding".into())),
            }
        } else {
            bytes.push(b)
        }
    }
    String::from_utf8(bytes).map_err(PagesError::gate)
}
fn markdown(text: &str, readme: &str, config: &Config, run: &Run) -> Result<String, PagesError> {
    let mut rendered = String::new();
    html::push_html(
        &mut rendered,
        Parser::new_ext(
            text,
            Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS,
        ),
    );
    let safe = ammonia::Builder::default().link_rel(None).clean(&rendered).to_string();
    let regex = Regex::new(r#"(href|src)="([^"]*)""#).map_err(PagesError::configuration)?;
    let mut error = None;
    let result = regex
        .replace_all(&safe, |c: &Captures<'_>| {
            let raw = c[2].replace("&amp;", "&");
            match rewrite(&raw, readme, config, run) {
                Ok(url) => format!("{}=\"{}\"", &c[1], escape(&url)),
                Err(e) => {
                    error = Some(e);
                    String::new()
                }
            }
        })
        .to_string();
    if let Some(error) = error {
        return Err(error);
    }
    Ok(result)
}
fn navigation(config: &Config, evidence: &Evidence) -> String {
    let mut nav = String::new();
    for (id, language) in &config.languages {
        nav.push_str(&format!(
            "<a href=\"{}{id}/\">{}</a> ",
            escape(&config.base_path),
            escape(&language.label)
        ));
    }
    if config.api.enabled {
        nav.push_str(&format!("<a href=\"{}api/\">API</a> ", escape(&config.base_path)));
    }
    if evidence.coverage_html.is_some() {
        nav.push_str(&format!(
            "<a href=\"{}coverage/\">Coverage</a> ",
            escape(&config.base_path)
        ));
    }
    if evidence.ci.is_some() {
        nav.push_str(&format!(
            "<a href=\"{}ci-summary.json\">CI</a>",
            escape(&config.base_path)
        ));
    }
    nav
}
fn page(config: &Config, nav: &str, body: &str) -> String {
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{}</title></head><body><nav>{nav}</nav><main>{body}</main></body></html>",
        escape(&config.title)
    )
}
pub(crate) fn validate(site: &Path, base: &str) -> Result<(), PagesError> {
    let mut names = BTreeSet::new();
    let mut components = BTreeMap::new();
    let paths = files(site)?;
    for path in &paths {
        crate::api::ensure_active()?;
        let mut prefix = PathBuf::new();
        for component in path.components() {
            prefix.push(component);
            let actual = prefix.to_string_lossy().into_owned();
            let folded = actual.to_lowercase();
            if components.get(&folded).is_some_and(|previous| previous != &actual) {
                return Err(PagesError::Config(format!("case-folded directory collision: {actual}")));
            }
            components.insert(folded, actual);
        }
        let key = path.to_string_lossy().to_lowercase();
        if !names.insert(key) {
            return Err(PagesError::Config(format!(
                "case-folded output collision: {}",
                path.display()
            )));
        }
    }
    for path in &paths {
        crate::api::ensure_active()?;
        if !path.extension().is_some_and(|e| {
            ["html", "htm", "xhtml", "css", "svg"]
                .iter()
                .any(|extension| e.to_string_lossy().eq_ignore_ascii_case(extension))
        }) {
            continue;
        }
        let text = fs::read_to_string(site.join(path))?;
        let mut document_url = url::Url::parse("https://cpp-infra.invalid/").map_err(PagesError::configuration)?;
        document_url.set_path(&format!("{base}{}", path.to_string_lossy().replace('\\', "/")));
        for url in crate::links::urls(
            &text,
            &path.extension().map(|e| e.to_string_lossy()).unwrap_or_default(),
            &document_url,
        )? {
            let url =
                url.trim_matches(|character: char| character.is_ascii_whitespace() || character.is_ascii_control());
            let scheme = url.split_once(':').map(|(scheme, _)| scheme.to_ascii_lowercase());
            if url.is_empty() || url.starts_with('#') || url.starts_with("mailto:") || url.starts_with("data:") {
                continue;
            }
            if matches!(scheme.as_deref(), Some("http" | "https")) || url.starts_with("//") {
                let normalized = if url.starts_with("//") {
                    format!("https:{url}")
                } else {
                    url.to_owned()
                };
                let external = url::Url::parse(&normalized).map_err(PagesError::gate)?;
                if external.host_str().is_none() {
                    return Err(PagesError::Gate("external link missing host".into()));
                }
                continue;
            }
            if url.starts_with("javascript:") {
                return Err(PagesError::Gate("executable URL in site".into()));
            }
            let local = url.split(['?', '#']).next().unwrap_or("");
            let local = percent_decode(local)?;
            let rel = if local.starts_with('/') {
                let s = local
                    .strip_prefix(base)
                    .ok_or_else(|| PagesError::Gate(format!("link outside base_path: {url}")))?;
                normalized(Path::new(s))?
            } else {
                normalized(&path.parent().unwrap_or(Path::new("")).join(local))?
            };
            let mut target = site.join(rel);
            if target.is_dir() {
                target = target.join("index.html");
            }
            if !target.is_file() {
                return Err(PagesError::Gate(format!(
                    "broken internal reference {} -> {url}",
                    path.display()
                )));
            }
        }
    }
    Ok(())
}
pub(crate) fn replace_directory(staging: &Path, target: &Path) -> Result<(), PagesError> {
    let parent = target.parent().ok_or_else(PagesError::required("no output parent"))?;
    let backup = tempfile::tempdir_in(parent)?;
    let previous = backup.path().join("previous");
    if target.exists() {
        fs::rename(target, &previous)?;
    }
    if let Err(error) = fs::rename(staging, target) {
        if previous.exists() {
            fs::rename(previous, target)?;
        }
        return Err(PagesError::Io(error));
    }
    Ok(())
}
pub(crate) fn build(config: &Config, run: &mut Run, evidence: &Evidence) -> Result<PathBuf, PagesError> {
    let temporary = tempfile::tempdir_in(&run.work)?;
    let staging = temporary.path().join("site");
    fs::create_dir(&staging)?;
    for asset in &config.assets {
        let output = relative(&asset.output)?;
        let first = output
            .components()
            .next()
            .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if [
            "api",
            "coverage",
            "index.html",
            "site-manifest.json",
            "ci-summary.json",
            "coverage-badge.json",
            ".nojekyll",
        ]
        .contains(&first.as_str())
            || config.languages.keys().any(|l| l.to_lowercase() == first)
        {
            return Err(PagesError::Config("asset targets a reserved site path".into()));
        }
        copy_tree(&input(&run.project, &asset.source)?, &staging.join(output))?;
    }
    if config.api.enabled {
        copy_tree(&run.work.join("api"), &staging.join("api"))?;
    }
    crate::reports::copy(evidence, &staging)?;
    let nav = navigation(config, evidence);
    for (id, language) in &config.languages {
        let body = markdown(
            &fs::read_to_string(input(&run.project, &language.readme)?)?,
            &language.readme,
            config,
            run,
        )?;
        let dir = staging.join(id);
        fs::create_dir_all(&dir)?;
        fs::write(dir.join("index.html"), page(config, &nav, &body))?;
    }
    let default = &config.languages[&config.default_language];
    let body = markdown(
        &fs::read_to_string(input(&run.project, &default.readme)?)?,
        &default.readme,
        config,
        run,
    )?;
    fs::write(staging.join("index.html"), page(config, &nav, &body))?;
    validate(&staging, &config.base_path)?;
    let mut manifest = BTreeMap::<String, Value>::new();
    for path in files(&staging)? {
        manifest.insert(
            path.to_string_lossy().into_owned(),
            json!({"sha256":sha(&fs::read(staging.join(path))?)}),
        );
    }
    write_json(
        &staging.join("site-manifest.json"),
        &json!({"schema":1,"source_digest":run.report["source_digest"],"config_digest":run.report["config_digest"],"bundle_id":run.report["bundle_id"],"api_fingerprint":run.report["data"]["api_fingerprint"],"files":manifest}),
    )?;
    crate::api::ensure_active()?;
    run.fresh()?;
    let target = run.work.join("site");
    replace_directory(&staging, &target)?;
    run.add_artifact("site", &target)?;
    run.add_artifact("site-manifest", &target.join("site-manifest.json"))?;
    run.report["data"]["site_digest"] = json!(artifact_digest(&target)?);
    Ok(target)
}
