// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::fs;
use std::path::Path;
use std::process::Command;
use std::process::Output;

use serde_json::Value;
use tempfile::TempDir;

fn fixture() -> TempDir {
    let dir = tempfile::tempdir().expect("fixture");
    fs::create_dir_all(dir.path().join(".infra/pages")).expect("config dir");
    fs::write(dir.path().join("README.md"), "# Hello\n[中文](README.zh_CN.md)\n![Logo](images/logo.svg)\n<script>alert(1)</script>\n<a href=\"javascript:alert(1)\">bad</a>").expect("readme");
    fs::write(dir.path().join("README.zh_CN.md"), "# 你好\n[English](README.md)").expect("chinese");
    fs::create_dir(dir.path().join("images")).expect("assets");
    fs::write(
        dir.path().join("images/logo.svg"),
        "<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>",
    )
    .expect("logo");
    fs::write(
        dir.path().join(".infra/pages/config.toml"),
        r#"schema = 1
title = "Demo <safe>"
default_language = "en"
base_path = "/demo/"
[languages.en]
readme = "README.md"
label = "English"
[languages.zh_CN]
readme = "README.zh_CN.md"
label = "简体中文"
[api]
enabled = false
[reports]
coverage_required = false
ci_required = false
[[assets]]
source = "images"
output = "assets"
"#,
    )
    .expect("config");
    dir
}
fn run(p: &Path, command: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cpp-infra-pages"))
        .args([
            "--project",
            p.to_str().expect("path"),
            "--report",
            p.join("build/report.json").to_str().expect("report"),
            command,
        ])
        .output()
        .expect("cli")
}
fn report(p: &Path) -> Value {
    serde_json::from_slice(&fs::read(p.join("build/report.json")).expect("report file")).expect("report json")
}
fn artifact(p: &Path, kind: &str) -> std::path::PathBuf {
    let r = report(p);
    let a = r["artifacts"]
        .as_array()
        .expect("artifacts")
        .iter()
        .find(|a| a["kind"] == kind)
        .expect("artifact kind");
    Path::new(r["report_root"].as_str().expect("root")).join(a["path"].as_str().expect("path"))
}
#[test]
fn test_build_sanitizes_markdown_rewrites_assets_and_bilingual_links() {
    let p = fixture();
    let output = run(p.path(), "build");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let site = artifact(p.path(), "site");
    let html = fs::read_to_string(site.join("en/index.html")).expect("html");
    assert!(html.contains("/demo/assets/logo.svg"));
    assert!(html.contains("/demo/zh_CN/"));
    assert!(!html.contains("<script"));
    assert!(!html.contains("javascript:"));
    assert!(html.contains("Demo &lt;safe&gt;"));
    assert!(site.join("site-manifest.json").is_file());
    assert_eq!(report(p.path())["status"], "passed");
}
#[test]
fn test_artifact_is_stable_and_has_no_wrapper_or_links() {
    let p = fixture();
    assert!(run(p.path(), "artifact").status.success());
    let first = fs::read(artifact(p.path(), "pages-archive")).expect("archive");
    assert!(run(p.path(), "artifact").status.success());
    let second = fs::read(artifact(p.path(), "pages-archive")).expect("second");
    assert_eq!(first, second);
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(first.as_slice()));
    let entries = archive
        .entries()
        .expect("entries")
        .map(|e| {
            let e = e.expect("entry");
            assert!(e.header().entry_type().is_file());
            e.path().expect("path").into_owned()
        })
        .collect::<Vec<_>>();
    assert!(entries.iter().any(|p| p == Path::new("index.html")));
}
#[test]
fn test_rejects_missing_required_reports_and_writes_error_report() {
    let p = fixture();
    let config = p.path().join(".infra/pages/config.toml");
    fs::write(
        &config,
        fs::read_to_string(&config)
            .expect("config")
            .replace("coverage_required = false", "coverage_required = true"),
    )
    .expect("required");
    assert_eq!(run(p.path(), "build").status.code(), Some(2));
    assert_eq!(report(p.path())["status"], "error");
}
#[test]
fn test_rejects_broken_links_configuration_typos_and_case_collisions() {
    let p = fixture();
    fs::write(p.path().join("README.md"), "[missing](no-such-file.html)").expect("broken");
    assert_eq!(run(p.path(), "build").status.code(), Some(1));
    fs::write(p.path().join("README.md"), "# OK").expect("readme");
    fs::write(p.path().join("images/LOGO.svg"), "collision").expect("collision");
    assert_eq!(run(p.path(), "build").status.code(), Some(2));
    let config = p.path().join(".infra/pages/config.toml");
    fs::write(
        &config,
        format!("unknown = 3\n{}", fs::read_to_string(&config).expect("read config")),
    )
    .expect("typo");
    assert_eq!(run(p.path(), "plan").status.code(), Some(2));
}
#[test]
fn test_version_is_protocol_json_without_fabricated_revision() {
    let output = Command::new(env!("CARGO_BIN_EXE_cpp-infra-pages"))
        .args(["--version", "--json"])
        .output()
        .expect("version");
    assert!(output.status.success());
    let v: Value = serde_json::from_slice(&output.stdout).expect("json");
    assert_eq!(v["protocol"], 1);
    assert_eq!(v["name"], "cpp-infra-pages");
}

fn config_api(p: &Path) {
    let path = p.join(".infra/pages/config.toml");
    let config = fs::read_to_string(&path).expect("config").replace(
        "enabled = false",
        "enabled = true\ninputs = [\"include\"]\nwarnings_as_errors = true",
    );
    fs::write(path, config).expect("api config");
    fs::create_dir_all(p.join("include")).expect("include");
    fs::write(p.join("include/demo.hpp"),"/** @file demo.hpp\n * Public arithmetic.\n */\n/** Add two values.\n * @param a First value.\n * @param b Second value.\n * @return Sum.\n */\nint add(int a, int b);\n").expect("header");
}
#[test]
#[ignore = "requires Doxygen; run with PAGES_TEST_DOXYGEN and --include-ignored"]
fn test_real_doxygen_documentation_and_warning_gate() {
    let binary = std::env::var_os("PAGES_TEST_DOXYGEN").unwrap_or_else(|| std::ffi::OsString::from("doxygen"));
    let p = fixture();
    config_api(p.path());
    let invoke = || {
        Command::new(env!("CARGO_BIN_EXE_cpp-infra-pages"))
            .env("CPP_INFRA_DOXYGEN", &binary)
            .args([
                "--project",
                p.path().to_str().expect("path"),
                "--report",
                p.path().join("build/report.json").to_str().expect("report"),
                "build",
            ])
            .output()
            .expect("doxygen CLI")
    };
    let output = invoke();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(artifact(p.path(), "site").join("api/index.html").is_file());
    let first = report(p.path());
    let mut context = first.clone();
    context["profile"] = Value::String("docs".into());
    context["artifacts"] = serde_json::json!([]);
    let context_file = p.path().join("build/api-context.json");
    fs::write(&context_file, serde_json::to_vec(&context).expect("api context")).expect("context file");
    let reused = Command::new(env!("CARGO_BIN_EXE_cpp-infra-pages"))
        .env("CPP_INFRA_DOXYGEN", &binary)
        .args([
            "--project",
            p.path().to_str().expect("p"),
            "--run-context",
            context_file.to_str().expect("context"),
            "--report",
            p.path().join("build/report.json").to_str().expect("report"),
            "build",
        ])
        .output()
        .expect("cached build");
    assert!(reused.status.success(), "{}", String::from_utf8_lossy(&reused.stderr));
    let cached = report(p.path());
    assert_eq!(
        cached["commands"].as_array().expect("commands").len(),
        1,
        "cached API should query version but not regenerate"
    );
    assert_eq!(first["data"]["api_fingerprint"], cached["data"]["api_fingerprint"]);

    fs::write(
        p.path().join("include/demo.hpp"),
        "/** @file demo.hpp */\nint undocumented(int input);\n",
    )
    .expect("undocumented");
    let bad = invoke();
    assert_eq!(bad.status.code(), Some(1), "{}", String::from_utf8_lossy(&bad.stderr));
    assert!(
        report(p.path())["commands"]
            .as_array()
            .expect("commands")
            .iter()
            .any(|c| c["exit_code"].as_i64().is_some_and(|code| code != 0))
    );
}
#[test]
fn test_run_context_rejects_changed_source_and_preserves_successful_site() {
    let p = fixture();
    assert!(run(p.path(), "build").status.success());
    let initial = report(p.path());
    let site = artifact(p.path(), "site");
    let original = fs::read(site.join("en/index.html")).expect("site");
    let context = p.path().join("build/context.json");
    let mut c = initial.clone();
    c["profile"] = Value::String("docs".into());
    fs::write(&context, serde_json::to_vec(&c).expect("context")).expect("context file");
    fs::write(p.path().join("README.md"), "[Broken](missing.html)").expect("changed input");
    let output = Command::new(env!("CARGO_BIN_EXE_cpp-infra-pages"))
        .args([
            "--project",
            p.path().to_str().expect("project"),
            "--run-context",
            context.to_str().expect("context"),
            "--report",
            p.path().join("build/failed.json").to_str().expect("report"),
            "build",
        ])
        .output()
        .expect("context CLI");
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(fs::read(site.join("en/index.html")).expect("retained"), original);
}
#[test]
fn test_quality_report_freshness_and_complete_task_inventory() {
    let p = fixture();
    assert!(run(p.path(), "build").status.success());
    let initial = report(p.path());
    let root = Path::new(initial["report_root"].as_str().expect("root"));
    let mut ci = initial.clone();
    ci["tool"]["name"] = Value::String("cpp-infra-ci".into());
    ci["data"] = serde_json::json!({"phase":"quality","complete":true,"expected_task_ids":["test","style"],"tasks":[{"task_id":"test","status":"passed","required":true}]});
    let directory = root.join("evidence");
    fs::create_dir(&directory).expect("evidence");
    fs::write(
        directory.join("quality-summary.json"),
        serde_json::to_vec(&ci).expect("ci"),
    )
    .expect("ci report");
    let invoke = || {
        Command::new(env!("CARGO_BIN_EXE_cpp-infra-pages"))
            .args([
                "--project",
                p.path().to_str().expect("p"),
                "--input-dir",
                directory.to_str().expect("inputs"),
                "--report",
                p.path().join("build/report.json").to_str().expect("report"),
                "build",
            ])
            .output()
            .expect("quality CLI")
    };
    assert_eq!(invoke().status.code(), Some(1));
    ci["data"]["tasks"]
        .as_array_mut()
        .expect("tasks")
        .push(serde_json::json!({"task_id":"style","status":"passed","required":true}));
    fs::write(
        directory.join("quality-summary.json"),
        serde_json::to_vec(&ci).expect("ci"),
    )
    .expect("valid ci");
    assert!(invoke().status.success());
    ci["source_digest"] = Value::String("old".into());
    fs::write(
        directory.join("quality-summary.json"),
        serde_json::to_vec(&ci).expect("ci"),
    )
    .expect("stale ci");
    assert_eq!(invoke().status.code(), Some(2));
}
#[cfg(unix)]
#[test]
fn test_rejects_symbolic_assets_and_run_roots_before_writing_outside_project() {
    use std::os::unix::fs::symlink;
    let p = fixture();
    let outside = tempfile::tempdir().expect("outside");
    symlink(outside.path(), p.path().join("images/link")).expect("asset symlink");
    assert_eq!(run(p.path(), "build").status.code(), Some(2));
    let other = fixture();
    symlink(outside.path(), other.path().join("build")).expect("build symlink");
    assert_eq!(run(other.path(), "build").status.code(), Some(2));
    assert!(!outside.path().join("infra-runs").exists());
}
#[test]
fn test_deploy_outputs_only_local_handoff() {
    let p = fixture();
    assert!(run(p.path(), "deploy").status.success());
    let handoff: Value =
        serde_json::from_slice(&fs::read(artifact(p.path(), "deploy-handoff")).expect("handoff")).expect("json");
    assert_eq!(handoff["requires_authenticated_workflow"], true);
    assert!(report(p.path())["commands"].as_array().expect("commands").is_empty());
}

#[cfg(unix)]
fn fake_doxygen(p: &Path, mode: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let script = p.join("build/fake-doxygen.py");
    fs::create_dir_all(script.parent().expect("parent")).expect("build");
    let body = format!(
        r#"#!/usr/bin/env python3
import pathlib, re, sys, time
if '--version' in sys.argv:
    print('1.9.8-test')
    sys.exit(0)
mode={mode:?}
if mode == 'timeout':
    time.sleep(30)
if mode == 'failure':
    print('warning: undocumented member', file=sys.stderr)
    sys.exit(1)
if mode == 'empty':
    sys.exit(0)
text=pathlib.Path(sys.argv[1]).read_text()
out=pathlib.Path(re.search(r'OUTPUT_DIRECTORY = "([^\"]+)"', text).group(1))/'html'
out.mkdir(parents=True)
(out/'index.html').write_text('<html><a href="files.html">Files</a></html>')
(out/'files.html').write_text('<html>No documented entities</html>')
"#
    );
    fs::write(&script, body).expect("fake executable");
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).expect("mode");
    script
}
#[cfg(unix)]
#[test]
fn test_doxygen_missing_failure_timeout_and_empty_output_fail_explicitly() {
    for (mode, code) in [
        ("failure", 1),
        ("empty", 1),
        ("timeout", 1),
        ("missing", 3),
        ("no-api", 1),
    ] {
        let p = fixture();
        config_api(p.path());
        let binary = fake_doxygen(p.path(), mode);
        let path = p.path().join(".infra/pages/config.toml");
        let config = fs::read_to_string(&path).expect("config").replace(
            "warnings_as_errors = true",
            "warnings_as_errors = true\ntimeout_seconds = 1",
        );
        fs::write(path, config).expect("timeout config");
        let output = Command::new(env!("CARGO_BIN_EXE_cpp-infra-pages"))
            .env(
                "CPP_INFRA_DOXYGEN",
                if mode == "missing" {
                    p.path().join("not-an-executable")
                } else {
                    binary
                },
            )
            .args([
                "--project",
                p.path().to_str().expect("p"),
                "--report",
                p.path().join("build/report.json").to_str().expect("report"),
                "api",
            ])
            .output()
            .expect("API CLI");
        assert_eq!(
            output.status.code(),
            Some(code),
            "mode={mode} {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(report(p.path())["status"], if code == 1 { "failed" } else { "error" });
        if mode == "timeout" {
            assert!(
                report(p.path())["commands"].as_array().expect("commands").len() >= 2,
                "timed-out commands must be recorded"
            );
        }
    }
}
#[test]
fn test_case_folded_directory_collisions_are_rejected() {
    let p = fixture();
    let path = p.path().join(".infra/pages/config.toml");
    let config = fs::read_to_string(&path).expect("config");
    fs::write(
        path,
        format!("{config}\n[[assets]]\nsource = \"other\"\noutput = \"Assets\"\n"),
    )
    .expect("extra asset");
    fs::create_dir(p.path().join("other")).expect("other");
    fs::write(p.path().join("other/extra.txt"), "extra").expect("extra");
    assert_eq!(run(p.path(), "build").status.code(), Some(2));
}
#[test]
fn test_single_language_and_base_path_validation() {
    let p = fixture();
    let path = p.path().join(".infra/pages/config.toml");
    let config = fs::read_to_string(&path).expect("config").replace(
        "[languages.zh_CN]\nreadme = \"README.zh_CN.md\"\nlabel = \"简体中文\"\n",
        "",
    );
    fs::write(&path, &config).expect("single config");
    fs::write(p.path().join("README.md"), "# Single language").expect("single readme");
    assert!(run(p.path(), "build").status.success());
    let html = fs::read_to_string(artifact(p.path(), "site").join("index.html")).expect("root");
    assert!(!html.contains("zh_CN"));
    for base in ["https://bad/", "/../", "//host/", "/bad", "/%2e%2e/"] {
        fs::write(&path, config.replace("/demo/", base)).expect("bad base");
        assert_eq!(run(p.path(), "plan").status.code(), Some(2));
    }
}

fn digest_tree(path: &Path) -> String {
    use sha2::Digest;
    use sha2::Sha256;
    let mut list = walkdir::WalkDir::new(path)
        .into_iter()
        .map(|e| e.expect("entry"))
        .filter(|e| e.file_type().is_file())
        .map(|e| e.path().strip_prefix(path).expect("relative").to_path_buf())
        .collect::<Vec<_>>();
    list.sort();
    let mut hash = Sha256::new();
    for rel in list {
        let name = rel.to_string_lossy();
        let bytes = fs::read(path.join(&rel)).expect("content");
        hash.update((name.len() as u64).to_le_bytes());
        hash.update(name.as_bytes());
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    }
    hash.finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}
#[test]
fn test_coverage_input_identity_html_hash_and_badge() {
    let p = fixture();
    assert!(run(p.path(), "build").status.success());
    let initial = report(p.path());
    let root = Path::new(initial["report_root"].as_str().expect("root"));
    let directory = root.join("evidence");
    let html = root.join("coverage-html");
    fs::create_dir(&directory).expect("evidence");
    fs::create_dir(&html).expect("coverage html");
    fs::write(html.join("index.html"), "<html>real coverage fixture</html>").expect("html");
    let mut coverage = initial.clone();
    coverage["tool"]["name"] = Value::String("cpp-infra-coverage".into());
    coverage["data"] = serde_json::json!({"summary":{"lines":{"covered":9,"total":10,"percent":90.0}},"passed":true});
    coverage["artifacts"] = serde_json::json!([{"kind":"coverage-html","path":"coverage-html","sha256":digest_tree(&html),"required":true}]);
    let input = directory.join("coverage-public-report.json");
    fs::write(&input, serde_json::to_vec(&coverage).expect("report")).expect("public report");
    let invoke = || {
        Command::new(env!("CARGO_BIN_EXE_cpp-infra-pages"))
            .args([
                "--project",
                p.path().to_str().expect("project"),
                "--input-dir",
                directory.to_str().expect("inputs"),
                "--report",
                p.path().join("build/report.json").to_str().expect("report"),
                "build",
            ])
            .output()
            .expect("coverage CLI")
    };
    let output = invoke();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let site = artifact(p.path(), "site");
    assert!(site.join("coverage/index.html").is_file());
    let badge: Value =
        serde_json::from_slice(&fs::read(site.join("coverage-badge.json")).expect("badge")).expect("json");
    assert_eq!(badge["message"], "90.0%");
    fs::write(html.join("index.html"), "tampered").expect("change html");
    assert_eq!(invoke().status.code(), Some(2));
}
#[test]
fn test_error_reports_accept_relative_output_in_current_directory() {
    let p = fixture();
    let output = Command::new(env!("CARGO_BIN_EXE_cpp-infra-pages"))
        .current_dir(p.path())
        .args(["--report", "report.json", "build"])
        .output()
        .expect("relative report");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(p.path().join("report.json").is_file());
}
#[cfg(unix)]
#[test]
fn test_source_links_use_github_revision_without_local_paths() {
    use std::os::unix::fs::PermissionsExt;
    let p = fixture();
    fs::write(p.path().join("demo.hpp"), "void f();").expect("source");
    fs::create_dir(p.path().join(".git")).expect("git fixture");
    fs::write(p.path().join(".git/HEAD"), "0123456789abcdef0123456789abcdef01234567\n").expect("fixture revision");
    fs::write(p.path().join("README.md"), "[Source](demo.hpp)").expect("link");
    let path = p.path().join(".infra/pages/config.toml");
    fs::write(
        &path,
        format!(
            "repository = \"https://github.com/example/demo\"\n{}",
            fs::read_to_string(&path).expect("config")
        ),
    )
    .expect("repo config");
    let bin = p.path().join("build/bin");
    fs::create_dir_all(&bin).expect("bin");
    let git = bin.join("git");
    fs::write(
        &git,
        "#!/usr/bin/env python3\nprint('0123456789abcdef0123456789abcdef01234567')\n",
    )
    .expect("fake git");
    fs::set_permissions(git, fs::Permissions::from_mode(0o755)).expect("mode");
    let path_var = format!("{}:{}", bin.display(), std::env::var("PATH").expect("PATH"));
    let output = Command::new(env!("CARGO_BIN_EXE_cpp-infra-pages"))
        .env("PATH", path_var)
        .args([
            "--project",
            p.path().to_str().expect("p"),
            "--report",
            p.path().join("build/report.json").to_str().expect("report"),
            "build",
        ])
        .output()
        .expect("source CLI");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let html = fs::read_to_string(artifact(p.path(), "site").join("en/index.html")).expect("html");
    assert!(html.contains("https://github.com/example/demo/blob/0123456789abcdef0123456789abcdef01234567/demo.hpp"));
    assert!(!html.contains(p.path().to_str().expect("local path")));
}

#[test]
fn test_runtime_snapshot_authenticates_bundle_and_current_executable() {
    use sha2::Digest;
    use sha2::Sha256;
    let p = fixture();
    assert!(run(p.path(), "build").status.success());
    let mut context = report(p.path());
    context["profile"] = Value::String("docs".into());
    context["artifacts"] = serde_json::json!([]);
    let executable = Path::new(env!("CARGO_BIN_EXE_cpp-infra-pages"));
    let version: Value = serde_json::from_slice(
        &Command::new(executable)
            .args(["--version", "--json"])
            .output()
            .expect("version")
            .stdout,
    )
    .expect("version json");
    let snapshot = serde_json::json!({"schema":1,"bundle_id":"development","tools":[{"name":"cpp-infra-pages","version":version["version"],"revision":version["revision"],"protocol":1,"binary":executable,"executable_sha256":Sha256::digest(fs::read(executable).expect("exe")).iter().map(|byte| format!("{byte:02x}")).collect::<String>()}],"offline":true,"stale":false});
    let path = p.path().join("build/snapshot.json");
    let bytes = serde_json::to_vec(&snapshot).expect("snapshot");
    fs::write(&path, &bytes).expect("snapshot file");
    context["runtime_snapshot"] = serde_json::json!(path);
    context["runtime_snapshot_digest"] = serde_json::json!(
        Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    );
    let context_path = p.path().join("build/context.json");
    fs::write(&context_path, serde_json::to_vec(&context).expect("context")).expect("context file");
    let invoke = || {
        Command::new(executable)
            .args([
                "--project",
                p.path().to_str().expect("p"),
                "--run-context",
                context_path.to_str().expect("context"),
                "build",
            ])
            .output()
            .expect("CLI")
    };
    let output = invoke();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));

    let mut missing = snapshot.clone();
    missing["tools"][0]["binary"] = Value::String("/definitely/missing/pages".into());
    let bytes = serde_json::to_vec(&missing).expect("missing snapshot");
    fs::write(&path, &bytes).expect("snapshot");
    context["runtime_snapshot_digest"] = serde_json::json!(
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    );
    fs::write(&context_path, serde_json::to_vec(&context).expect("context")).expect("context");
    assert_eq!(invoke().status.code(), Some(2), "snapshot.binary must exist");
    for bundle in ["main-not-a-valid-digest".to_owned(), format!("main-{}", "a".repeat(64))] {
        let mut forged = snapshot.clone();
        forged["bundle_id"] = serde_json::json!(bundle);
        context["bundle_id"] = serde_json::json!(bundle);
        let bytes = serde_json::to_vec(&forged).expect("forged");
        fs::write(&path, &bytes).expect("snapshot");
        context["runtime_snapshot_digest"] = serde_json::json!(
            Sha256::digest(bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        );
        fs::write(&context_path, serde_json::to_vec(&context).expect("context")).expect("context");
        assert_eq!(
            invoke().status.code(),
            Some(2),
            "main may not use a development executable"
        );
    }
    context["bundle_id"] = serde_json::json!("development");
    let mut wrong = snapshot.clone();
    wrong["tools"][0]["executable_sha256"] = Value::String("bad-hash".into());
    let bytes = serde_json::to_vec(&wrong).expect("wrong");
    fs::write(&path, &bytes).expect("wrong snapshot");
    context["runtime_snapshot_digest"] = serde_json::json!(
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    );
    fs::write(&context_path, serde_json::to_vec(&context).expect("wrong context")).expect("context");
    assert_eq!(invoke().status.code(), Some(2));
}

#[test]
fn test_failed_staging_build_preserves_successful_output_in_same_run() {
    let p = fixture();
    assert!(run(p.path(), "build").status.success());
    let initial = report(p.path());
    let site = artifact(p.path(), "site");
    let before = fs::read(site.join("en/index.html")).expect("old html");
    fs::write(p.path().join("README.md"), "[Missing](missing.html)").expect("broken readme");
    assert_eq!(run(p.path(), "build").status.code(), Some(1));
    let mut context = report(p.path());
    context["run_id"] = initial["run_id"].clone();
    context["report_root"] = initial["report_root"].clone();
    context["artifacts"] = serde_json::json!([]);
    context["profile"] = Value::String("docs".into());
    let path = p.path().join("build/same-run.json");
    fs::write(&path, serde_json::to_vec(&context).expect("context")).expect("write context");
    let output = Command::new(env!("CARGO_BIN_EXE_cpp-infra-pages"))
        .args([
            "--project",
            p.path().to_str().expect("p"),
            "--run-context",
            path.to_str().expect("context"),
            "build",
        ])
        .output()
        .expect("same run");
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(fs::read(site.join("en/index.html")).expect("retained"), before);
}

#[test]
fn test_downloaded_coverage_report_uses_portable_relative_artifacts() {
    let p = fixture();
    assert!(run(p.path(), "build").status.success());
    let initial = report(p.path());
    let root = Path::new(initial["report_root"].as_str().expect("root"));
    let evidence = root.join("downloaded");
    let html = evidence.join("coverage-html");
    fs::create_dir_all(&html).expect("download");
    fs::write(html.join("index.html"), "<html>downloaded coverage</html>").expect("html");
    let mut coverage = initial.clone();
    coverage["tool"]["name"] = Value::String("cpp-infra-coverage".into());
    coverage["project"] = Value::String("/different/runner/project".into());
    coverage["report_root"] = Value::String("/different/runner/project/build/infra-runs/id".into());
    coverage["data"] = serde_json::json!({"summary":{"lines":{"covered":9,"total":10,"percent":90.0}},"passed":true});
    coverage["artifacts"] = serde_json::json!([{"kind":"coverage-html","path":"coverage-html","sha256":digest_tree(&html),"required":true}]);
    let input = evidence.join("coverage-tool.json");
    fs::write(&input, serde_json::to_vec(&coverage).expect("report")).expect("coverage");
    let mut context = initial.clone();
    context["profile"] = Value::String("docs".into());
    context["artifacts"] = serde_json::json!([{"kind":"coverage-summary","path":input,"sha256":({use sha2::{Digest,Sha256};Sha256::digest(fs::read(&input).expect("bytes")).iter().map(|byte| format!("{byte:02x}")).collect::<String>()}),"required":true}]);
    let path = root.join("context.json");
    fs::write(&path, serde_json::to_vec(&context).expect("context")).expect("context file");
    let output = Command::new(env!("CARGO_BIN_EXE_cpp-infra-pages"))
        .args([
            "--project",
            p.path().to_str().expect("p"),
            "--run-context",
            path.to_str().expect("context"),
            "--report",
            p.path().join("build/report.json").to_str().expect("report"),
            "build",
        ])
        .output()
        .expect("CLI");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(artifact(p.path(), "site").join("coverage/index.html").exists());
}

#[cfg(unix)]
#[test]
fn test_plan_does_not_execute_external_programs() {
    use std::os::unix::fs::PermissionsExt;
    let p = fixture();
    let bin = p.path().join("build/bin");
    fs::create_dir_all(&bin).expect("bin");
    let git = bin.join("git");
    let marker = p.path().join("build/git-invoked");
    fs::write(&git,format!("#!/usr/bin/env python3\nimport pathlib\npathlib.Path({:?}).write_text('invoked')\nprint('0123456789abcdef0123456789abcdef01234567')\n",marker.to_str().expect("marker"))).expect("fake git");
    fs::set_permissions(git, fs::Permissions::from_mode(0o755)).expect("executable");
    let path_var = format!("{}:{}", bin.display(), std::env::var("PATH").expect("PATH"));
    let output = Command::new(env!("CARGO_BIN_EXE_cpp-infra-pages"))
        .env("PATH", path_var)
        .args(["--project", p.path().to_str().expect("project"), "plan"])
        .output()
        .expect("plan");
    assert!(output.status.success());
    assert!(!marker.exists(), "plan must not execute git or other external tools");
}

#[cfg(unix)]
#[test]
fn test_rejects_doxygen_configuration_interpolation() {
    let p = fixture();
    config_api(p.path());
    let binary = fake_doxygen(p.path(), "empty");
    let config = p.path().join(".infra/pages/config.toml");
    fs::write(
        &config,
        fs::read_to_string(&config)
            .expect("config")
            .replace("Demo <safe>", "$(PAGES_TEST_SECRET)"),
    )
    .expect("unsafe title");
    let output = Command::new(env!("CARGO_BIN_EXE_cpp-infra-pages"))
        .env("CPP_INFRA_DOXYGEN", binary)
        .env("PAGES_TEST_SECRET", "must-not-leak")
        .args([
            "--project",
            p.path().to_str().expect("p"),
            "--report",
            p.path().join("build/report.json").to_str().expect("report"),
            "api",
        ])
        .output()
        .expect("API CLI");
    assert_eq!(output.status.code(), Some(2));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("must-not-leak"));
}

#[cfg(unix)]
#[test]
fn test_cancellation_terminates_doxygen_and_writes_cancelled_report() {
    use std::time::Duration;
    use std::time::Instant;

    use nix::sys::signal::Signal;
    use nix::sys::signal::kill;
    use nix::unistd::Pid;
    let p = fixture();
    config_api(p.path());
    let binary = fake_doxygen(p.path(), "timeout");
    let mut child = Command::new(env!("CARGO_BIN_EXE_cpp-infra-pages"))
        .env("CPP_INFRA_DOXYGEN", binary)
        .args([
            "--project",
            p.path().to_str().expect("p"),
            "--report",
            p.path().join("build/report.json").to_str().expect("report"),
            "api",
        ])
        .spawn()
        .expect("API process");
    let started = Instant::now();
    loop {
        let running = p
            .path()
            .join("build/infra-runs")
            .read_dir()
            .ok()
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .any(|entry| {
                walkdir::WalkDir::new(entry.path())
                    .into_iter()
                    .filter_map(Result::ok)
                    .any(|file| file.file_name() == "doxygen.log")
            });
        if running {
            break;
        }
        assert!(started.elapsed() < Duration::from_secs(5), "Doxygen should start");
        std::thread::sleep(Duration::from_millis(10));
    }
    kill(Pid::from_raw(child.id() as i32), Signal::SIGINT).expect("cancel");
    let status = child.wait().expect("cancel exit");
    assert_eq!(status.code(), Some(130));
    assert_eq!(report(p.path())["status"], "cancelled");
    assert!(report(p.path())["data"]["termination"]["reason"] == "cancelled");
}

#[cfg(unix)]
#[test]
fn test_review_api_input_subtree_symlinks_are_rejected_before_doxygen() {
    use std::os::unix::fs::symlink;
    let p = fixture();
    config_api(p.path());
    let outside = tempfile::tempdir().expect("private");
    fs::write(
        outside.path().join("secret.hpp"),
        "/** @file secret.hpp */\n/** SECRET_OUTSIDE */\nint secret();",
    )
    .expect("secret");
    symlink(outside.path(), p.path().join("include/external")).expect("symlink");
    let executable = fake_doxygen(p.path(), "failure");
    let output = Command::new(env!("CARGO_BIN_EXE_cpp-infra-pages"))
        .env("CPP_INFRA_DOXYGEN", executable)
        .args(["--project", p.path().to_str().expect("p"), "api"])
        .output()
        .expect("API");
    assert_eq!(output.status.code(), Some(2));
}
#[test]
fn test_review_malformed_quality_reports_cannot_become_green() {
    let p = fixture();
    assert!(run(p.path(), "build").status.success());
    let mut report = report(p.path());
    report.as_object_mut().expect("object").remove("tool");
    report["data"] = serde_json::json!({"phase":"quality","complete":true,"expected_task_ids":["x"],"tasks":[{"task_id":"x","status":"failed","required":"true"}]});
    let directory = p.path().join("build/evidence");
    fs::create_dir(&directory).expect("evidence");
    fs::write(directory.join("fake.json"), serde_json::to_vec(&report).expect("json")).expect("fake");
    let output = Command::new(env!("CARGO_BIN_EXE_cpp-infra-pages"))
        .args([
            "--project",
            p.path().to_str().expect("p"),
            "--input-dir",
            directory.to_str().expect("input"),
            "build",
        ])
        .output()
        .expect("CLI");
    assert_eq!(output.status.code(), Some(2));
}
#[test]
fn test_review_transaction_filters_are_exact_root_paths() {
    let p = fixture();
    assert!(run(p.path(), "plan").status.success());
    let initial = report(p.path())["config_digest"].clone();
    for name in [".tmp", "transactions", ".transactions"] {
        let dir = p.path().join(".infra").join(name);
        fs::create_dir_all(&dir).expect("txn");
        fs::write(dir.join("pending.json"), name).expect("pending");
        assert!(run(p.path(), "plan").status.success());
        assert_eq!(report(p.path())["config_digest"], initial, "{name} should be excluded");
    }
    fs::create_dir_all(p.path().join(".infra/pages/.txn-legitimate")).expect("normal");
    fs::write(p.path().join(".infra/pages/.txn-legitimate/policy"), "normal input").expect("input");
    assert!(run(p.path(), "plan").status.success());
    assert_ne!(report(p.path())["config_digest"], initial);
}
#[test]
fn test_review_reserved_language_names_are_rejected() {
    for id in ["api", "coverage", "assets", "API", "site-manifest.json"] {
        let p = fixture();
        let path = p.path().join(".infra/pages/config.toml");
        let config = fs::read_to_string(&path)
            .expect("config")
            .replace("default_language = \"en\"", &format!("default_language = \"{id}\""))
            .replace("[languages.en]", &format!("[languages.{id}]"));
        fs::write(path, config).expect("language");
        assert_eq!(run(p.path(), "build").status.code(), Some(2), "reserved {id}");
    }
}
#[test]
fn test_review_html_and_css_static_url_forms_are_checked() {
    for html in [
        "<img src=missing.png>",
        "<A HREF='missing.html'>bad</A>",
        "<img srcset='missing.png 1x, other.png 2x'>",
        "<style>@import 'missing.css';</style>",
        "<video POSTER=missing.png></video>",
    ] {
        let p = fixture();
        fs::write(p.path().join("images/page.html"), html).expect("html");
        assert_eq!(run(p.path(), "build").status.code(), Some(1), "{html}");
    }
}
#[test]
fn test_review_repeated_context_calls_preserve_private_artifacts() {
    let p = fixture();
    assert!(run(p.path(), "artifact").status.success());
    let first = report(p.path());
    let first_archive = artifact(p.path(), "pages-archive");
    let original = fs::read(&first_archive).expect("archive");
    let mut context = first.clone();
    context["profile"] = Value::String("docs".into());
    context["artifacts"] = serde_json::json!([]);
    let path = p.path().join("build/context.json");
    fs::write(&path, serde_json::to_vec(&context).expect("context")).expect("context");
    let output = Command::new(env!("CARGO_BIN_EXE_cpp-infra-pages"))
        .args([
            "--project",
            p.path().to_str().expect("p"),
            "--run-context",
            path.to_str().expect("context"),
            "--report",
            p.path().join("build/report.json").to_str().expect("report"),
            "artifact",
        ])
        .output()
        .expect("CLI");
    assert!(output.status.success());
    let second_archive = artifact(p.path(), "pages-archive");
    assert_ne!(first_archive, second_archive);
    assert_eq!(fs::read(first_archive).expect("retained"), original);
    assert!(second_archive.to_str().expect("path").contains("/pages/"));
}
#[cfg(unix)]
#[test]
fn test_review_timeout_commands_match_closed_protocol_shape() {
    let p = fixture();
    config_api(p.path());
    let path = p.path().join(".infra/pages/config.toml");
    fs::write(
        &path,
        fs::read_to_string(&path).expect("config").replace(
            "warnings_as_errors = true",
            "warnings_as_errors = true\ntimeout_seconds = 1",
        ),
    )
    .expect("timeout");
    let output = Command::new(env!("CARGO_BIN_EXE_cpp-infra-pages"))
        .env("CPP_INFRA_DOXYGEN", fake_doxygen(p.path(), "timeout"))
        .args([
            "--project",
            p.path().to_str().expect("p"),
            "--report",
            p.path().join("build/report.json").to_str().expect("report"),
            "api",
        ])
        .output()
        .expect("CLI");
    assert_eq!(output.status.code(), Some(1));
    for command in report(p.path())["commands"].as_array().expect("commands") {
        assert!(
            command.as_object().expect("object").keys().all(|key| [
                "program",
                "args",
                "cwd",
                "env",
                "exit_code",
                "log",
                "duration_ms"
            ]
            .contains(&key.as_str()))
        );
    }
}

#[test]
fn test_review_root_report_exclusions_are_shared_and_cannot_hide_inputs() {
    let p = fixture();
    let output = p.path().join("result.json");
    let invoke = || {
        Command::new(env!("CARGO_BIN_EXE_cpp-infra-pages"))
            .args([
                "--project",
                p.path().to_str().expect("p"),
                "--report",
                output.to_str().expect("report"),
                "build",
            ])
            .output()
            .expect("CLI")
    };
    assert!(invoke().status.success());
    let first: Value = serde_json::from_slice(&fs::read(&output).expect("report")).expect("json");
    assert_eq!(first["source_excluded_paths"], serde_json::json!(["result.json"]));
    assert!(invoke().status.success());
    let second: Value = serde_json::from_slice(&fs::read(&output).expect("second")).expect("json");
    assert_eq!(first["source_digest"], second["source_digest"]);
    let mut context = second;
    context["profile"] = serde_json::json!("docs");
    context["artifacts"] = serde_json::json!([]);
    context["source_excluded_paths"] = serde_json::json!(["README.md"]);
    let file = p.path().join("build/context.json");
    fs::write(&file, serde_json::to_vec(&context).expect("context")).expect("file");
    let bad = Command::new(env!("CARGO_BIN_EXE_cpp-infra-pages"))
        .args([
            "--project",
            p.path().to_str().expect("p"),
            "--run-context",
            file.to_str().expect("ctx"),
            "build",
        ])
        .output()
        .expect("CLI");
    assert_eq!(bad.status.code(), Some(2));
    let business = p.path().join("business.json");
    fs::write(&business, "{\"business\":true}").expect("business");
    let bad = Command::new(env!("CARGO_BIN_EXE_cpp-infra-pages"))
        .args([
            "--project",
            p.path().to_str().expect("p"),
            "--report",
            business.to_str().expect("business"),
            "build",
        ])
        .output()
        .expect("CLI");
    assert_eq!(bad.status.code(), Some(2));
    assert_eq!(fs::read_to_string(business).expect("preserved"), "{\"business\":true}");
}

#[cfg(unix)]
#[test]
fn test_review_input_changes_during_doxygen_reject_publication() {
    let p = fixture();
    config_api(p.path());
    let fake = fake_doxygen(p.path(), "mutate");
    let mut code = fs::read_to_string(&fake).expect("fake");
    code.push_str("\npathlib.Path(re.search(r'GENERATE_TAGFILE = \\\"([^\\\"]+)\\\"', text).group(1)).write_text('<tagfile><compound kind=\\\"file\\\"></compound></tagfile>')\npathlib.Path('README.md').write_text('# Changed during Doxygen')\n");
    fs::write(&fake, code).expect("mutating fake");
    let output = Command::new(env!("CARGO_BIN_EXE_cpp-infra-pages"))
        .env("CPP_INFRA_DOXYGEN", fake)
        .args(["--project", p.path().to_str().expect("p"), "api"])
        .output()
        .expect("API");
    assert_eq!(
        output.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_rereview_html_base_and_css_image_set_use_browser_resource_semantics() {
    let p = fixture();
    fs::write(
        p.path().join("images/page.html"),
        "<base href='/demo/'><img src='existing.png'>",
    )
    .expect("html");
    fs::write(p.path().join("images/existing.png"), "image").expect("local image");
    assert_eq!(
        run(p.path(), "build").status.code(),
        Some(1),
        "base must resolve the image from site root, where it is missing"
    );
    fs::write(
        p.path().join("images/page.html"),
        "<base href='/demo/assets/'><img src='existing.png'>",
    )
    .expect("valid base");
    assert!(run(p.path(), "build").status.success());
}
#[test]
fn test_rereview_css_image_set_strings_are_checked() {
    let p = fixture();
    fs::write(
        p.path().join("images/style.css"),
        ".x{background-image:image-set('missing.png' 1x)}",
    )
    .expect("css");
    assert_eq!(
        run(p.path(), "build").status.code(),
        Some(1),
        "image-set strings are URLs"
    );
    fs::write(
        p.path().join("images/style.css"),
        ".x{background-image:image-set('logo.svg' 1x)}",
    )
    .expect("valid image set");
    assert!(run(p.path(), "build").status.success());
}
#[cfg(unix)]
#[test]
fn test_rereview_report_symlink_alias_is_rejected_without_replacing_it() {
    use std::os::unix::fs::symlink;
    let p = fixture();
    let result = p.path().join("result.json");
    let invoke = |output: &Path| {
        Command::new(env!("CARGO_BIN_EXE_cpp-infra-pages"))
            .args([
                "--project",
                p.path().to_str().expect("p"),
                "--report",
                output.to_str().expect("report"),
                "build",
            ])
            .output()
            .expect("CLI")
    };
    assert!(invoke(&result).status.success());
    let alias = p.path().join("alias.json");
    symlink(&result, &alias).expect("alias");
    assert_eq!(invoke(&alias).status.code(), Some(2));
    assert!(
        fs::symlink_metadata(alias)
            .expect("kept alias")
            .file_type()
            .is_symlink()
    );
}

#[test]
fn test_rereview_meta_refresh_targets_follow_html_base() {
    let p = fixture();
    fs::write(
        p.path().join("images/page.html"),
        "<!doctype html><meta http-equiv=refresh content='0;url=missing.html'>",
    )
    .expect("refresh");
    assert_eq!(run(p.path(), "build").status.code(), Some(1));
    fs::write(
        p.path().join("images/page.html"),
        "<base href='/demo/assets/'><meta http-equiv=REFRESH content=\"0; URL='target.html'\"> ",
    )
    .expect("valid refresh");
    fs::write(p.path().join("images/target.html"), "<p>target</p>").expect("target");
    assert!(run(p.path(), "build").status.success());
}
#[cfg(unix)]
#[test]
fn test_policy_inputs_cannot_alias_excluded_build_files() {
    use std::os::unix::fs::symlink;
    let p = fixture();
    fs::create_dir_all(p.path().join(".infra/pages/policy")).expect("policy");
    fs::create_dir_all(p.path().join("build")).expect("build");
    fs::write(p.path().join("build/Doxyfile.in"), "EXTRACT_ALL=YES\n").expect("policy target");
    symlink(
        p.path().join("build/Doxyfile.in"),
        p.path().join(".infra/pages/policy/Doxyfile.in"),
    )
    .expect("policy alias");
    assert_eq!(run(p.path(), "plan").status.code(), Some(2));
}

#[test]
#[ignore = "requires Doxygen; run with PAGES_TEST_DOXYGEN and --include-ignored"]
fn test_generated_api_inputs_in_build_have_an_independent_cache_digest() {
    let binary = std::env::var_os("PAGES_TEST_DOXYGEN").unwrap_or_else(|| std::ffi::OsString::from("doxygen"));
    let p = fixture();
    config_api(p.path());
    let config = p.path().join(".infra/pages/config.toml");
    fs::write(
        &config,
        fs::read_to_string(&config)
            .expect("config")
            .replace("inputs = [\"include\"]", "inputs = [\"build/generated\"]"),
    )
    .expect("generated inputs");
    fs::create_dir_all(p.path().join("build/generated")).expect("generated dir");
    let header = p.path().join("build/generated/demo.hpp");
    fs::copy(p.path().join("include/demo.hpp"), &header).expect("generated header");
    let invoke = |context: Option<&Path>| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_cpp-infra-pages"));
        command.env("CPP_INFRA_DOXYGEN", &binary).args([
            "--project",
            p.path().to_str().expect("p"),
            "--report",
            p.path().join("build/report.json").to_str().expect("report"),
        ]);
        if let Some(context) = context {
            command.args(["--run-context", context.to_str().expect("context")]);
        }
        command.arg("api").output().expect("API")
    };
    let first_output = invoke(None);
    assert!(
        first_output.status.success(),
        "{}",
        String::from_utf8_lossy(&first_output.stderr)
    );
    let first = report(p.path());
    let mut context = first.clone();
    context["profile"] = serde_json::json!("docs");
    context["artifacts"] = serde_json::json!([]);
    let context_path = p.path().join("build/generated-context.json");
    fs::write(&context_path, serde_json::to_vec(&context).expect("context")).expect("context file");
    fs::write(
        &header,
        fs::read_to_string(&header)
            .expect("header")
            .replace("Sum.", "Changed generated API marker."),
    )
    .expect("change generated");
    let second_output = invoke(Some(&context_path));
    assert!(
        second_output.status.success(),
        "{}",
        String::from_utf8_lossy(&second_output.stderr)
    );
    let second = report(p.path());
    assert_eq!(first["source_digest"], second["source_digest"]);
    assert_ne!(
        first["data"]["api_fingerprint"], second["data"]["api_fingerprint"],
        "generated header contents must invalidate API cache independently"
    );
}

#[test]
fn test_input_directory_restores_shared_report_exclusion_metadata() {
    let p = fixture();
    let result = p.path().join("result.json");
    let first = Command::new(env!("CARGO_BIN_EXE_cpp-infra-pages"))
        .args([
            "--project",
            p.path().to_str().expect("p"),
            "--report",
            result.to_str().expect("report"),
            "build",
        ])
        .output()
        .expect("first CLI");
    assert!(first.status.success());
    let mut quality: Value = serde_json::from_slice(&fs::read(&result).expect("public report")).expect("JSON");
    let digest = quality["source_digest"].clone();
    quality["tool"]["name"] = serde_json::json!("cpp-infra-ci");
    quality["data"] = serde_json::json!({"phase":"quality","complete":true,"expected_task_ids":["test"],"tasks":[{"task_id":"test","status":"passed","required":true}]});
    let directory = p.path().join("build/evidence");
    fs::create_dir_all(&directory).expect("evidence");
    fs::write(
        directory.join("quality.json"),
        serde_json::to_vec(&quality).expect("quality"),
    )
    .expect("quality file");
    let output = Command::new(env!("CARGO_BIN_EXE_cpp-infra-pages"))
        .args([
            "--project",
            p.path().to_str().expect("p"),
            "--input-dir",
            directory.to_str().expect("input"),
            "--report",
            p.path().join("build/report.json").to_str().expect("report"),
            "build",
        ])
        .output()
        .expect("restored CLI");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(report(p.path())["source_digest"], digest);
    assert_eq!(
        report(p.path())["source_excluded_paths"],
        serde_json::json!(["result.json"])
    );
}

#[test]
fn test_exclusion_paths_cannot_use_directory_syntax_to_hide_business_json() {
    let p = fixture();
    assert!(run(p.path(), "build").status.success());
    let mut context = report(p.path());
    context["profile"] = serde_json::json!("docs");
    context["artifacts"] = serde_json::json!([]);
    context["source_excluded_paths"] = serde_json::json!(["business.json/"]);
    fs::write(p.path().join("business.json"), "{\"business\":true}").expect("business");
    let file = p.path().join("build/context.json");
    fs::write(&file, serde_json::to_vec(&context).expect("context")).expect("context file");
    let output = Command::new(env!("CARGO_BIN_EXE_cpp-infra-pages"))
        .args([
            "--project",
            p.path().to_str().expect("p"),
            "--run-context",
            file.to_str().expect("context"),
            "build",
        ])
        .output()
        .expect("CLI");
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn test_final_review_custom_project_and_pages_input_roles_are_protected() {
    for role in [
        "source_roots",
        "test_roots",
        "generated_roots",
        "third_party_roots",
        "readme",
        "api",
        "assets",
    ] {
        let p = fixture();
        let directory = p.path().join("custom_inputs");
        fs::create_dir(&directory).expect("custom input root");
        if role.ends_with("_roots") {
            fs::write(
                p.path().join(".infra/project.toml"),
                format!("schema = 1\nkind = \"library\"\n{role} = [\"custom_inputs\"]\n"),
            )
            .expect("project roots");
        }
        let config = p.path().join(".infra/pages/config.toml");
        let mut text = fs::read_to_string(&config).expect("config");
        if role == "readme" {
            assert!(run(p.path(), "build").status.success());
            let public_report = fs::read(p.path().join("build/report.json")).expect("valid public report");
            fs::write(directory.join("result.json"), public_report).expect("declared README input");
            text = text.replace("readme = \"README.md\"", "readme = \"custom_inputs/result.json\"");
        }
        if role == "api" {
            text = text.replace("enabled = false", "enabled = false\ninputs = [\"custom_inputs\"]");
        }
        if role == "assets" {
            text.push_str("\n[[assets]]\nsource = \"custom_inputs\"\noutput = \"custom-assets\"\n");
        }
        fs::write(&config, text).expect("role config");
        let output = directory.join("result.json");
        let before = fs::read(&output).ok();
        let result = Command::new(env!("CARGO_BIN_EXE_cpp-infra-pages"))
            .args([
                "--project",
                p.path().to_str().expect("project"),
                "--report",
                output.to_str().expect("report"),
                "plan",
            ])
            .output()
            .expect("CLI");
        assert_eq!(result.status.code(), Some(2), "custom role {role} must be protected");
        assert_eq!(fs::read(output).ok(), before, "protected role must not be overwritten");
        let allowed = p.path().join("unrelated-report.json");
        let output = Command::new(env!("CARGO_BIN_EXE_cpp-infra-pages"))
            .args([
                "--project",
                p.path().to_str().expect("project"),
                "--report",
                allowed.to_str().expect("report"),
                "plan",
            ])
            .output()
            .expect("allowed report");
        assert!(
            output.status.success(),
            "non-input report should remain allowed for {role}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
#[test]
fn test_final_review_drive_prefixed_exclusions_are_rejected_on_every_host() {
    let p = fixture();
    assert!(run(p.path(), "build").status.success());
    let initial = report(p.path());
    let path = p.path().join("build/drive-context.json");
    for drive in [
        "C:/business.json",
        "C:business.json",
        "z:/business.json",
        "z:business.json",
    ] {
        let mut context = initial.clone();
        context["profile"] = serde_json::json!("docs");
        context["artifacts"] = serde_json::json!([]);
        context["source_excluded_paths"] = serde_json::json!([drive]);
        fs::write(&path, serde_json::to_vec(&context).expect("context")).expect("context file");
        let output = Command::new(env!("CARGO_BIN_EXE_cpp-infra-pages"))
            .args([
                "--project",
                p.path().to_str().expect("project"),
                "--run-context",
                path.to_str().expect("context"),
                "build",
            ])
            .output()
            .expect("CLI");
        assert_eq!(output.status.code(), Some(2), "drive prefix {drive}");
    }
}
#[cfg(unix)]
#[test]
fn test_final_review_lock_wait_can_cancel_without_releasing_the_lock() {
    use std::fs::OpenOptions;
    use std::process::Stdio;
    use std::time::Duration;
    use std::time::Instant;

    use fs2::FileExt;
    use nix::sys::signal::Signal;
    use nix::sys::signal::kill;
    use nix::unistd::Pid;
    let p = fixture();
    assert!(run(p.path(), "build").status.success());
    let mut context = report(p.path());
    context["profile"] = serde_json::json!("docs");
    context["artifacts"] = serde_json::json!([]);
    let root = std::path::PathBuf::from(context["report_root"].as_str().expect("root"));
    let path = p.path().join("build/lock-context.json");
    fs::write(&path, serde_json::to_vec(&context).expect("context")).expect("context file");
    let held = OpenOptions::new()
        .read(true)
        .write(true)
        .open(root.join(".pages.lock"))
        .expect("lock");
    FileExt::lock_exclusive(&held).expect("hold lock");
    let before = fs::read_dir(root.join("pages")).expect("calls").count();
    let mut child = Command::new(env!("CARGO_BIN_EXE_cpp-infra-pages"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .args([
            "--project",
            p.path().to_str().expect("project"),
            "--run-context",
            path.to_str().expect("context"),
            "--report",
            p.path().join("build/lock-cancel-report.json").to_str().expect("report"),
            "build",
        ])
        .spawn()
        .expect("waiter");
    let started = Instant::now();
    while fs::read_dir(root.join("pages")).expect("calls").count() <= before {
        assert!(started.elapsed() < Duration::from_secs(3));
        std::thread::sleep(Duration::from_millis(10));
    }
    std::thread::sleep(Duration::from_millis(100));
    kill(Pid::from_raw(child.id() as i32), Signal::SIGINT).expect("cancel waiter");
    let deadline = Instant::now() + Duration::from_secs(1);
    let status = loop {
        if let Some(status) = child.try_wait().expect("wait status") {
            break Some(status);
        }
        if Instant::now() >= deadline {
            break None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    FileExt::unlock(&held).expect("release after observation");
    let exited_before_unlock = status.is_some();
    let status = status.unwrap_or_else(|| child.wait().expect("cleanup waiter"));
    assert!(
        exited_before_unlock,
        "SIGINT must terminate the waiter while the lock remains held"
    );
    assert_eq!(status.code(), Some(130));
    let report: Value =
        serde_json::from_slice(&fs::read(p.path().join("build/lock-cancel-report.json")).expect("cancelled report"))
            .expect("JSON");
    assert_eq!(report["status"], "cancelled");
}

#[test]
fn test_final_review_declared_asset_reports_cannot_hide_new_input_contents() {
    let p = fixture();
    fs::create_dir(p.path().join("documents")).expect("documents");
    let config = p.path().join(".infra/pages/config.toml");
    let mut text = fs::read_to_string(&config).expect("config");
    text.push_str("\n[[assets]]\nsource = \"documents\"\noutput = \"documents\"\n");
    fs::write(config, text).expect("assets config");
    assert!(run(p.path(), "build").status.success());
    let initial = report(p.path());
    let mut asset = initial.clone();
    asset["data"]["marker"] = serde_json::json!("FINAL_INPUT_ROLE_CHANGE");
    fs::write(
        p.path().join("documents/result.json"),
        serde_json::to_vec(&asset).expect("asset report"),
    )
    .expect("asset input");
    let mut context = initial;
    context["profile"] = serde_json::json!("docs");
    context["artifacts"] = serde_json::json!([]);
    context["source_excluded_paths"] = serde_json::json!(["documents/result.json"]);
    let file = p.path().join("build/asset-context.json");
    fs::write(&file, serde_json::to_vec(&context).expect("context")).expect("context file");
    let output = Command::new(env!("CARGO_BIN_EXE_cpp-infra-pages"))
        .args([
            "--project",
            p.path().to_str().expect("project"),
            "--run-context",
            file.to_str().expect("context"),
            "build",
        ])
        .output()
        .expect("CLI");
    assert_eq!(
        output.status.code(),
        Some(2),
        "a valid public-report-shaped asset is still a tracked input"
    );
}
#[test]
fn test_final_review_lock_wait_has_a_finite_budget() {
    use std::fs::OpenOptions;
    use std::process::Stdio;
    use std::time::Duration;
    use std::time::Instant;

    use fs2::FileExt;
    let p = fixture();
    let config = p.path().join(".infra/pages/config.toml");
    fs::write(
        &config,
        fs::read_to_string(&config)
            .expect("config")
            .replace("enabled = false", "enabled = false\ntimeout_seconds = 1"),
    )
    .expect("wait budget");
    assert!(run(p.path(), "build").status.success());
    let mut context = report(p.path());
    context["profile"] = serde_json::json!("docs");
    context["artifacts"] = serde_json::json!([]);
    let root = std::path::PathBuf::from(context["report_root"].as_str().expect("root"));
    let file = p.path().join("build/timeout-context.json");
    fs::write(&file, serde_json::to_vec(&context).expect("context")).expect("context file");
    let held = OpenOptions::new()
        .read(true)
        .write(true)
        .open(root.join(".pages.lock"))
        .expect("lock");
    FileExt::lock_exclusive(&held).expect("hold");
    let mut child = Command::new(env!("CARGO_BIN_EXE_cpp-infra-pages"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .args([
            "--project",
            p.path().to_str().expect("project"),
            "--run-context",
            file.to_str().expect("context"),
            "build",
        ])
        .spawn()
        .expect("waiter");
    let deadline = Instant::now() + Duration::from_secs(4);
    let status = loop {
        if let Some(status) = child.try_wait().expect("status") {
            break Some(status);
        }
        if Instant::now() >= deadline {
            break None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    FileExt::unlock(&held).expect("release after observation");
    let bounded = status.is_some();
    let status = status.unwrap_or_else(|| child.wait().expect("cleanup waiter"));
    assert!(bounded, "unavailable run locks must eventually time out");
    assert_eq!(status.code(), Some(3));
}

#[test]
fn test_closure_readme_and_asset_inputs_reject_common_digest_blind_zones() {
    let mut observed = Vec::new();
    for kind in ["readme", "asset"] {
        for zone in [
            "build",
            "target",
            ".git",
            ".infra/.tmp",
            ".infra/transactions",
            ".infra/.transactions",
        ] {
            let p = fixture();
            let directory = p.path().join(zone).join("generated");
            fs::create_dir_all(&directory).expect("untracked input directory");
            fs::write(directory.join("input.txt"), "# CLOSURE_UNTRACKED_INPUT").expect("content");
            let config = p.path().join(".infra/pages/config.toml");
            let mut text = fs::read_to_string(&config).expect("config");
            if kind == "readme" {
                text = text.replace(
                    "readme = \"README.md\"",
                    &format!("readme = \"{zone}/generated/input.txt\""),
                );
            } else {
                text.push_str(&format!(
                    "\n[[assets]]\nsource = \"{zone}/generated\"\noutput = \"extra-assets\"\n"
                ));
            }
            fs::write(config, text).expect("declared content input");
            observed.push((format!("{kind}:{zone}"), run(p.path(), "plan").status.code()));
        }
    }
    assert!(
        observed.iter().all(|(_, code)| *code == Some(2)),
        "every untracked ordinary content input must be refused: {observed:?}"
    );
}
#[test]
fn test_closure_asset_directory_cannot_contain_excluded_transaction_inputs() {
    let p = fixture();
    let config = p.path().join(".infra/pages/config.toml");
    let mut text = fs::read_to_string(&config).expect("config");
    text.push_str("\n[[assets]]\nsource = \".infra\"\noutput = \"infra-assets\"\n");
    fs::write(config, text).expect("asset root");
    fs::create_dir_all(p.path().join(".infra/transactions")).expect("transaction");
    fs::write(
        p.path().join(".infra/transactions/pending.txt"),
        "UNTRACKED_TRANSACTION_CONTENT",
    )
    .expect("transaction content");
    assert_eq!(run(p.path(), "plan").status.code(), Some(2));
}
#[test]
fn test_closure_public_passed_report_checks_freshness_after_execute() {
    use std::io::Read;
    use std::process::Stdio;
    let p = fixture();
    assert!(run(p.path(), "build").status.success());
    let mut context = report(p.path());
    context["profile"] = serde_json::json!("x".repeat(2 * 1024 * 1024));
    context["artifacts"] = serde_json::json!([]);
    let file = p.path().join("build/final-fresh-context.json");
    fs::write(&file, serde_json::to_vec(&context).expect("context")).expect("context file");
    let report_path = p.path().join("build/final-fresh-report.json");
    let mut child = Command::new(env!("CARGO_BIN_EXE_cpp-infra-pages"))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .args([
            "--project",
            p.path().to_str().expect("project"),
            "--run-context",
            file.to_str().expect("context"),
            "--report",
            report_path.to_str().expect("report"),
            "plan",
        ])
        .spawn()
        .expect("plan process");
    let mut first = [0];
    child
        .stdout
        .as_mut()
        .expect("stdout")
        .read_exact(&mut first)
        .expect("plan began printing after configuration validation");
    fs::write(p.path().join("README.md"), "# Changed before final passed report")
        .expect("mutation while plan stdout blocks");
    let output = child.wait_with_output().expect("finish plan");
    assert_eq!(
        output.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&fs::read(report_path).expect("report")).expect("JSON");
    assert_eq!(report["status"], "error");
}
