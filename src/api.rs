// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Controlled Doxygen invocation, cache identity and document gates.
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;
use std::time::Instant;

use serde_json::Value;
use serde_json::json;

use crate::config::Config;
use crate::config::input;
use crate::identity::artifact_digest;
use crate::identity::digest_files;
use crate::identity::files;
use crate::identity::sha;
use crate::pages_error::PagesError;
use crate::report::Run;
use crate::report::read_json;
use crate::report::reject_symlink;
use crate::report::write_json;
static CANCELLED: AtomicBool = AtomicBool::new(false);
pub(crate) fn install_cancel_handler() {
    let _ = ctrlc::set_handler(|| CANCELLED.store(true, Ordering::SeqCst));
}
pub(crate) fn ensure_active() -> Result<(), PagesError> {
    if CANCELLED.load(Ordering::SeqCst) {
        Err(PagesError::Cancelled)
    } else {
        Ok(())
    }
}
fn quoted(value: &str) -> Result<String, PagesError> {
    if value.contains(['"', '$', '\n', '\r', '\0']) {
        return Err(PagesError::Config("unsupported Doxygen input characters".into()));
    }
    Ok(format!("\"{value}\""))
}
fn doxygen() -> PathBuf {
    std::env::var_os("CPP_INFRA_DOXYGEN")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("doxygen"))
}
fn command(run: &mut Run, program: &Path, args: &[String], logname: &str, timeout: u64) -> Result<i32, PagesError> {
    let log = run.work.join(logname);
    reject_symlink(&log)?;
    let file = fs::File::create(&log)?;
    let errors = file.try_clone()?;
    let start = Instant::now();
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(&run.project)
        .stdout(Stdio::from(file))
        .stderr(Stdio::from(errors));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let commands = run.report["commands"]
        .as_array_mut()
        .ok_or_else(PagesError::required("invalid command report"))?;
    let command_index = commands.len();
    commands.push(json!({"program":program,"args":args,"cwd":run.project,"exit_code":null,"duration_ms":0,"log":log.strip_prefix(&run.root).map_err(PagesError::configuration)?,"env":{}}));
    let mut child = command.spawn()?;
    let code = loop {
        if let Some(status) = child.try_wait()? {
            break status.code().unwrap_or(1);
        }
        if CANCELLED.load(Ordering::SeqCst) || start.elapsed().as_secs() >= timeout {
            #[cfg(unix)]
            {
                use nix::sys::signal::Signal;
                use nix::sys::signal::killpg;
                use nix::unistd::Pid;
                let _ = killpg(Pid::from_raw(child.id() as i32), Signal::SIGKILL);
            }
            #[cfg(windows)]
            {
                let status = Command::new("taskkill")
                    .args(windows_kill_args(child.id()))
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()?;
                if !status.success() && child.try_wait()?.is_none() {
                    return Err(PagesError::Io(std::io::Error::other(
                        "taskkill failed to terminate the Doxygen process tree",
                    )));
                }
            }
            #[cfg(not(any(unix, windows)))]
            {
                child.kill()?;
            }
            let _ = child.wait();
            run.report["commands"][command_index]["duration_ms"] = json!(start.elapsed().as_millis());
            run.report["data"]["termination"] = json!({"command_index":command_index,"reason":if CANCELLED.load(Ordering::SeqCst){"cancelled"}else{"timeout"}});
            if CANCELLED.load(Ordering::SeqCst) {
                return Err(PagesError::Cancelled);
            }
            return Err(PagesError::Gate("Doxygen timeout".into()));
        }
        thread::sleep(Duration::from_millis(20));
    };
    run.report["commands"][command_index]["exit_code"] = json!(code);
    run.report["commands"][command_index]["duration_ms"] = json!(start.elapsed().as_millis());
    Ok(code)
}
fn inputs_digest(config: &Config, project: &Path) -> Result<String, PagesError> {
    let mut paths = Vec::new();
    for directory in &config.api.inputs {
        let directory = input(project, directory)?;
        for path in files(&directory)? {
            paths.push(
                directory
                    .join(path)
                    .strip_prefix(project)
                    .map_err(PagesError::configuration)?
                    .to_path_buf(),
            );
        }
    }
    paths.sort_by_key(|path| path.to_string_lossy().replace('\\', "/").into_bytes());
    paths.dedup();
    digest_files(project, &paths)
}
pub(crate) fn inputs_fresh(config: &Config, run: &Run, digest: &str) -> Result<(), PagesError> {
    if inputs_digest(config, &run.project)? != digest {
        return Err(PagesError::Config(
            "API input artifacts changed during generation".into(),
        ));
    }
    Ok(())
}
pub(crate) fn generate(config: &Config, run: &mut Run) -> Result<PathBuf, PagesError> {
    if !config.api.enabled {
        return Err(PagesError::Config("API documentation is disabled".into()));
    }
    let binary = doxygen();
    if command(
        run,
        &binary,
        &["--version".into()],
        "doxygen-version.log",
        config.api.timeout_seconds,
    )? != 0
    {
        return Err(PagesError::Gate("Doxygen version command failed".into()));
    }
    let version = fs::read_to_string(run.work.join("doxygen-version.log"))?
        .trim()
        .to_owned();
    if version.is_empty() {
        return Err(PagesError::Config("Doxygen returned an empty version".into()));
    }
    run.report["external_versions"]["doxygen"] = json!(version);
    let input_digest = inputs_digest(config, &run.project)?;
    run.report["data"]["api_input_digest"] = json!(input_digest);
    let template = include_str!("../conf/pages/policy/Doxyfile.in");
    let fingerprint = sha(format!(
        "{}:{}:{}:{}:{}",
        run.report["source_digest"],
        run.report["config_digest"],
        version,
        sha(template.as_bytes()),
        input_digest
    )
    .as_bytes());
    let output = run.work.join("api");
    let cache = run.root.join("pages/.cache");
    reject_symlink(&cache)?;
    fs::create_dir_all(&cache)?;
    let metadata = cache.join(format!("{fingerprint}.json"));
    if metadata.is_file() {
        let cached = read_json(&metadata)?;
        let relative = crate::config::relative(
            cached["path"]
                .as_str()
                .ok_or_else(PagesError::required("invalid API cache path"))?,
        )?;
        let previous = run.root.join(relative).canonicalize()?;
        if !previous.starts_with(&run.root) {
            return Err(PagesError::Config("API cache escapes run root".into()));
        }
        if cached["fingerprint"] == fingerprint
            && previous.join("index.html").is_file()
            && cached["sha256"] == artifact_digest(&previous)?
        {
            run.fresh()?;
            inputs_fresh(config, run, &input_digest)?;
            crate::site::copy_tree(&previous, &output)?;
            run.report["data"]["api_fingerprint"] = json!(fingerprint);
            run.add_artifact("api", &output)?;
            return Ok(output);
        }
    }
    let temporary = tempfile::tempdir_in(&run.work)?;
    let staging = temporary.path().join("doxygen");
    fs::create_dir(&staging)?;
    let inputs = config
        .api
        .inputs
        .iter()
        .map(|s| input(&run.project, s).and_then(|p| quoted(&p.to_string_lossy())))
        .collect::<Result<Vec<_>, _>>()?
        .join(" ");
    let defines = config
        .api
        .defines
        .iter()
        .map(|s| quoted(s))
        .collect::<Result<Vec<_>, _>>()?
        .join(" ");
    let doxyfile = template
        .replace("@TITLE@", &quoted(&config.title)?)
        .replace("@OUTPUT@", &quoted(&staging.to_string_lossy())?)
        .replace("@INPUTS@", &inputs)
        .replace("@TAGFILE@", &quoted(&staging.join("api.tag.xml").to_string_lossy())?)
        .replace("@DEFINES@", &defines)
        .replace("@WARNINGS@", if config.api.warnings_as_errors { "YES" } else { "NO" })
        .replace("@PROJECT@", &quoted(&run.project.to_string_lossy())?);
    let config_path = run.work.join("Doxyfile");
    reject_symlink(&config_path)?;
    fs::write(&config_path, doxyfile)?;
    if command(
        run,
        &binary,
        &[config_path.to_string_lossy().into_owned()],
        "doxygen.log",
        config.api.timeout_seconds,
    )? != 0
    {
        return Err(PagesError::Gate("Doxygen documentation or warnings gate failed".into()));
    }
    let generated = staging.join("html");
    if !generated.join("index.html").is_file() || fs::metadata(generated.join("index.html"))?.len() == 0 {
        return Err(PagesError::Gate("Doxygen produced no HTML index".into()));
    }
    let tagfile = staging.join("api.tag.xml");
    if !tagfile.is_file() || !fs::read_to_string(&tagfile)?.contains("<compound ") {
        return Err(PagesError::Gate("Doxygen produced no documented API entities".into()));
    }
    let api_files = files(&generated)?;
    if !api_files
        .iter()
        .any(|p| p.extension().is_some_and(|e| e == "html") && p != Path::new("index.html"))
    {
        return Err(PagesError::Gate("Doxygen produced no required API pages".into()));
    }
    // Doxygen 1.9 uses shared CSS variables with directory-relative aliases.
    // Normalize these generated aliases so the API subtree remains portable.
    let css_path = generated.join("doxygen.css");
    if css_path.is_file() {
        let css = fs::read_to_string(&css_path)?;
        let regex = regex::Regex::new(r#"url\((["'])(?:\.\./)*([^"']+)["']\)"#).map_err(PagesError::configuration)?;
        let normalized = regex.replace_all(&css, |captures: &regex::Captures<'_>| {
            let name = &captures[2];
            if generated.join(name).is_file() {
                format!("url('{}')", name)
            } else if generated.join("search").join(name).is_file() {
                format!("url('search/{}')", name)
            } else {
                captures[0].to_owned()
            }
        });
        fs::write(css_path, normalized.as_bytes())?;
    }
    crate::site::validate(&generated, "/")?;
    run.fresh()?;
    inputs_fresh(config, run, &input_digest)?;
    crate::site::replace_directory(&generated, &output)?;
    write_json(
        &metadata,
        &json!({"schema":1,"fingerprint":fingerprint,"path":output.strip_prefix(&run.root).map_err(PagesError::configuration)?,"sha256":artifact_digest(&output)?}),
    )?;
    run.add_artifact("api", &output)?;
    run.report["data"]["api_fingerprint"] = Value::String(fingerprint);
    Ok(output)
}

#[cfg(any(windows, test))]
fn windows_kill_args(pid: u32) -> Vec<String> {
    vec!["/PID".into(), pid.to_string(), "/T".into(), "/F".into()]
}
#[cfg(test)]
mod process_tree_tests {
    #[test]
    fn test_windows_termination_requests_descendants_and_forced_exit() {
        assert_eq!(super::windows_kill_args(123), ["/PID", "123", "/T", "/F"]);
    }
}
