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
use serde_json::json;
use sha2::Digest;
use sha2::Sha256;
use tempfile::TempDir;
fn fixture() -> TempDir {
    let project = tempfile::tempdir().expect("project");
    fs::create_dir_all(project.path().join(".infra/pages")).expect("config directory");
    fs::write(project.path().join("README.md"), "# Protocol fixture").expect("README");
    fs::write(project.path().join(".infra/pages/config.toml"),"schema=1\ntitle='Protocol'\ndefault_language='en'\nbase_path='/'\n[languages.en]\nreadme='README.md'\nlabel='English'\n[api]\nenabled=false\n[reports]\nci_required=false\ncoverage_required=false\n").expect("config");
    project
}
fn call(project: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cpp-infra-pages"))
        .current_dir(project)
        .args([
            "--project",
            project.to_str().expect("project"),
            "--report",
            "build/report.json",
        ])
        .args(args)
        .output()
        .expect("CLI")
}
fn report(project: &Path) -> Value {
    serde_json::from_slice(&fs::read(project.join("build/report.json")).expect("report")).expect("JSON")
}
fn initial(project: &Path) -> Value {
    assert!(call(project, &["build"]).status.success());
    let mut context = report(project);
    context["profile"] = json!("docs");
    context["artifacts"] = json!([]);
    context
}
fn quality(context: &Value) -> Value {
    let mut quality = context.clone();
    quality.as_object_mut().expect("report").remove("profile");
    quality["tool"]["name"] = json!("cpp-infra-ci");
    quality["data"] = json!({"phase":"quality","complete":true,"incomplete_remote_coverage":false,"expected_task_ids":["check"],"tasks":[{"task_id":"check","required":true,"status":"passed","report":null}]});
    quality
}
fn evidence(project: &Path, context: &Value, evidence: &Value, kind: &str) -> Output {
    let root = std::path::PathBuf::from(context["report_root"].as_str().expect("root"));
    let path = root.join("evidence.json");
    let bytes = serde_json::to_vec(evidence).expect("evidence JSON");
    fs::write(&path, &bytes).expect("evidence");
    let mut context = context.clone();
    context["artifacts"] =
        json!([{"kind":kind,"path":path,"sha256":format!("{:x}",Sha256::digest(bytes)),"required":true}]);
    fs::write(
        project.join("build/context.json"),
        serde_json::to_vec(&context).expect("context JSON"),
    )
    .expect("context");
    call(project, &["--run-context", "build/context.json", "build"])
}
#[test]
fn test_common_report_fields_and_command_metadata_fail_closed() {
    let p = fixture();
    let context = initial(p.path());
    let mut valid = quality(&context);
    valid["tool"]["protocol"] = json!(1);
    valid["commands"] = json!([{"program":"checker","args":["--check"],"cwd":p.path(),"env":{"TOKEN":"[REDACTED]"},"exit_code":0,"log":"check.log","duration_ms":1,"timed_out":false,"cancelled":false,"stderr_log":"stderr.log","termination":"timeout"}]);
    valid["diagnostics"] =
        json!([{"code":"sample","severity":"note","message":"checked","project_relative_path":null}]);
    assert!(evidence(p.path(), &context, &valid, "quality-summary").status.success());
    let cases = vec![
        ("/schema", json!(2)),
        ("/duration_ms", json!(-1)),
        ("/source_revision", json!(3)),
        ("/tool/revision", json!(3)),
        ("/tool/protocol", json!(2)),
        ("/source_digest", Value::Null),
        ("/config_digest", json!("bad")),
        ("/external_versions", json!([])),
        ("/commands", Value::Null),
        ("/diagnostics", json!([{}])),
        (
            "/artifacts",
            json!([{"kind":"summary","path":"x","sha256":"bad","required":true}]),
        ),
        ("/commands/0/args", json!([1])),
        ("/commands/0/exit_code", json!("zero")),
        ("/commands/0/duration_ms", json!(-1)),
        ("/commands/0/env", json!({"BAD":true})),
        ("/commands/0/timed_out", json!("false")),
        ("/commands/0/cancelled", json!("false")),
        ("/commands/0/stderr_log", json!(0)),
        ("/commands/0/termination", json!("unknown")),
        ("/source_excluded_paths", json!([1])),
    ];
    for (path, value) in cases {
        let mut invalid = valid.clone();
        *invalid.pointer_mut(path).expect("field") = value;
        let output = evidence(p.path(), &context, &invalid, "quality-summary");
        assert_eq!(
            output.status.code(),
            Some(2),
            "{path}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert_eq!(
        evidence(p.path(), &context, &json!([]), "quality-summary")
            .status
            .code(),
        Some(2)
    );
}
#[test]
fn test_quality_task_structure_and_completion_gates() {
    let p = fixture();
    let context = initial(p.path());
    let valid = quality(&context);
    for (path, value, code) in [
        ("/data/phase", json!("final"), 2),
        ("/data/complete", json!("true"), 2),
        ("/data/incomplete_remote_coverage", json!("false"), 2),
        ("/data/tasks", Value::Null, 2),
        ("/data/expected_task_ids", Value::Null, 2),
        ("/data/tasks/0/task_id", json!(""), 2),
        ("/data/tasks/0/status", json!("unknown"), 2),
        ("/data/tasks/0/required", json!("true"), 2),
        ("/data/expected_task_ids", json!([1]), 2),
        ("/data/expected_task_ids", json!([""]), 2),
        ("/data/expected_task_ids", json!(["check", "check"]), 2),
        ("/data/complete", json!(false), 1),
        ("/data/incomplete_remote_coverage", json!(true), 1),
        ("/data/tasks/0/status", json!("failed"), 1),
        ("/data/tasks", json!([]), 1),
        ("/data/expected_task_ids", json!(["missing"]), 1),
        ("/status", json!("failed"), 1),
    ] {
        let mut invalid = valid.clone();
        *invalid.pointer_mut(path).expect("field") = value;
        assert_eq!(
            evidence(p.path(), &context, &invalid, "quality-summary").status.code(),
            Some(code),
            "{path}"
        );
    }
    let mut duplicate = valid.clone();
    let task = duplicate["data"]["tasks"][0].clone();
    duplicate["data"]["tasks"].as_array_mut().expect("tasks").push(task);
    assert_eq!(
        evidence(p.path(), &context, &duplicate, "quality-summary")
            .status
            .code(),
        Some(2)
    );
}
#[test]
fn test_context_protocol_and_json_boundaries() {
    let p = fixture();
    let context = initial(p.path());
    for (path, value) in [
        ("/schema", json!(2)),
        ("/run_id", json!("../escape")),
        ("/project", json!("/wrong-project")),
        ("/profile", json!("")),
        ("/task_id", Value::Null),
        ("/bundle_id", Value::Null),
        ("/bundle_id", json!("main-invalid")),
        ("/source_excluded_paths", json!("not-an-array")),
        ("/source_excluded_paths", json!([1])),
        ("/artifacts", json!([{"kind":"summary"}])),
    ] {
        let mut invalid = context.clone();
        *invalid.pointer_mut(path).expect("field") = value;
        fs::write(
            p.path().join("build/context.json"),
            serde_json::to_vec(&invalid).expect("JSON"),
        )
        .expect("context");
        let output = call(p.path(), &["--run-context", "build/context.json", "plan"]);
        assert_eq!(
            output.status.code(),
            Some(2),
            "{path}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    fs::write(p.path().join("build/context.json"), "not JSON").expect("malformed");
    assert_eq!(
        call(p.path(), &["--run-context", "build/context.json", "plan"])
            .status
            .code(),
        Some(2)
    );
    assert_eq!(call(p.path(), &[]).status.code(), Some(2));
    let version = call(p.path(), &["--version"]);
    assert!(version.status.success());
    assert!(String::from_utf8_lossy(&version.stdout).contains("cpp-infra-pages 0.1.0"));
}

#[test]
fn test_profileless_context_preserves_identity_for_pages_operations() {
    let p = fixture();
    let mut context = initial(p.path());
    context["profile"] = Value::Null;
    for operation in ["plan", "build", "artifact", "deploy"] {
        fs::write(
            p.path().join("build/context.json"),
            serde_json::to_vec(&context).expect("context JSON"),
        )
        .expect("context");
        let output = call(p.path(), &["--run-context", "build/context.json", operation]);
        assert!(
            output.status.success(),
            "{operation}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let actual = report(p.path());
        for field in [
            "run_id",
            "task_id",
            "bundle_id",
            "report_root",
            "source_digest",
            "config_digest",
        ] {
            assert_eq!(actual[field], context[field], "{operation}: {field}");
        }
        assert_eq!(actual["status"], "passed");
    }
    let api = call(p.path(), &["--run-context", "build/context.json", "api"]);
    assert_eq!(api.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&api.stderr).contains("API documentation is disabled"));
    for invalid in [
        None,
        Some(json!("")),
        Some(json!(false)),
        Some(json!(3)),
        Some(json!([])),
        Some(json!({})),
    ] {
        let mut invalid_context = context.clone();
        match invalid {
            Some(value) => invalid_context["profile"] = value,
            None => {
                invalid_context.as_object_mut().expect("context").remove("profile");
            }
        }
        fs::write(
            p.path().join("build/context.json"),
            serde_json::to_vec(&invalid_context).expect("context JSON"),
        )
        .expect("context");
        let output = call(p.path(), &["--run-context", "build/context.json", "plan"]);
        assert_eq!(output.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&output.stderr).contains("run-context profile missing"));
    }
}

#[test]
fn test_tracked_content_under_nested_build_and_infra_changes_old_identity() {
    for directory in ["docs/build", ".infra/docs"] {
        let p = fixture();
        let root = p.path().join(directory);
        fs::create_dir_all(&root).expect("tracked document root");
        fs::write(root.join("guide.md"), "# TRACKED_GUIDE_BEFORE").expect("guide");
        let config = p.path().join(".infra/pages/config.toml");
        let mut text = fs::read_to_string(&config)
            .expect("config")
            .replace("readme='README.md'", &format!("readme='{directory}/guide.md'"));
        text.push_str(&format!("\n[[assets]]\nsource='{directory}'\noutput='documents'\n"));
        fs::write(config, text).expect("tracked content config");
        let context = initial(p.path());
        let built = report(p.path());
        let site = std::path::PathBuf::from(built["report_root"].as_str().expect("root")).join(
            built["artifacts"]
                .as_array()
                .expect("artifacts")
                .iter()
                .find(|entry| entry["kind"] == "site")
                .expect("site artifact")["path"]
                .as_str()
                .expect("site path"),
        );
        assert!(
            fs::read_to_string(site.join("en/index.html"))
                .expect("HTML")
                .contains("TRACKED_GUIDE_BEFORE")
        );
        assert!(
            fs::read_to_string(site.join("documents/guide.md"))
                .expect("copied asset")
                .contains("TRACKED_GUIDE_BEFORE")
        );
        fs::write(root.join("guide.md"), "# TRACKED_GUIDE_AFTER").expect("guide change");
        assert!(call(p.path(), &["plan"]).status.success());
        let current = report(p.path());
        assert!(
            current["source_digest"] != context["source_digest"]
                || current["config_digest"] != context["config_digest"]
        );
        fs::write(
            p.path().join("build/context.json"),
            serde_json::to_vec(&context).expect("context"),
        )
        .expect("context file");
        assert_eq!(
            call(p.path(), &["--run-context", "build/context.json", "build"])
                .status
                .code(),
            Some(2)
        );
    }
}
#[test]
fn test_config_path_and_preprocessor_boundaries() {
    for (entry, replacement) in [
        ("readme='README.md'", "readme='missing.md'"),
        ("schema=1", "schema=2"),
        ("default_language='en'", "default_language='missing'"),
        ("enabled=false", "enabled=true\ninputs=[]"),
        ("enabled=false", "enabled=false\ninputs=['README.md']"),
        ("enabled=false", "enabled=false\ninputs=['../outside']"),
        ("enabled=false", "enabled=false\ndefines=['$(SECRET)']"),
        ("title='Protocol'", "title='Protocol'\nrepository='not-a-url'"),
        (
            "title='Protocol'",
            "title='Protocol'\nrepository='http://github.com/example/project'",
        ),
        (
            "title='Protocol'",
            "title='Protocol'\nrepository='https://github.com/example/project?query=yes'",
        ),
    ] {
        let p = fixture();
        let path = p.path().join(".infra/pages/config.toml");
        fs::write(
            &path,
            fs::read_to_string(&path).expect("config").replace(entry, replacement),
        )
        .expect("invalid config");
        assert_eq!(call(p.path(), &["plan"]).status.code(), Some(2), "{replacement}");
    }
    let p = fixture();
    let path = p.path().join(".infra/pages/config.toml");
    fs::write(
        &path,
        fs::read_to_string(&path)
            .expect("config")
            .replace("enabled=false", "enabled=false\ndefines=['FEATURE=1']"),
    )
    .expect("valid preprocessor config");
    assert!(call(p.path(), &["plan"]).status.success());
}
#[test]
fn test_git_source_revision_references_are_read_without_running_git() {
    for layout in ["loose", "packed", "worktree", "unborn", "invalid"] {
        let p = fixture();
        let metadata = p.path().join("build/git-meta");
        fs::create_dir_all(&metadata).expect("metadata");
        let git = p.path().join(".git");
        if layout == "worktree" {
            fs::write(&git, "gitdir: build/git-meta\n").expect("worktree indirection");
            fs::write(metadata.join("commondir"), "../common\n").expect("common metadata");
            fs::create_dir_all(p.path().join("build/common/refs/heads")).expect("refs");
            fs::write(
                p.path().join("build/common/refs/heads/main"),
                "0123456789abcdef0123456789abcdef01234567\n",
            )
            .expect("revision");
        } else {
            fs::create_dir(&git).expect("git directory");
        }
        let directory = if layout == "worktree" { metadata } else { git };
        fs::write(
            directory.join("HEAD"),
            if layout == "invalid" {
                "bad HEAD\n"
            } else {
                "ref: refs/heads/main\n"
            },
        )
        .expect("HEAD");
        if layout == "loose" {
            fs::create_dir_all(directory.join("refs/heads")).expect("refs");
            fs::write(
                directory.join("refs/heads/main"),
                "0123456789abcdef0123456789abcdef01234567\n",
            )
            .expect("revision");
        }
        if layout == "packed" {
            fs::write(
                directory.join("packed-refs"),
                "# packed refs\n0123456789abcdef0123456789abcdef01234567 refs/heads/main\n",
            )
            .expect("packed refs");
        }
        let result = call(p.path(), &["plan"]);
        assert_eq!(
            result.status.code(),
            Some(if layout == "invalid" { 2 } else { 0 }),
            "{layout}"
        );
        if layout != "invalid" {
            assert_eq!(
                report(p.path())["source_revision"],
                if layout == "unborn" {
                    Value::Null
                } else {
                    json!("0123456789abcdef0123456789abcdef01234567")
                }
            );
        }
    }
}
#[test]
fn test_root_report_role_configuration_errors_and_output_io_are_explicit() {
    for declaration in [
        "schema=1\nsource_roots=3\n",
        "schema=1\nsource_roots=[3]\n",
        "not valid toml",
    ] {
        let p = fixture();
        fs::write(p.path().join(".infra/project.toml"), declaration).expect("project config");
        let output = Command::new(env!("CARGO_BIN_EXE_cpp-infra-pages"))
            .current_dir(p.path())
            .args([
                "--project",
                p.path().to_str().expect("project"),
                "--report",
                "result.json",
                "plan",
            ])
            .output()
            .expect("CLI");
        assert_eq!(output.status.code(), Some(2));
    }
    let p = fixture();
    fs::create_dir_all(p.path().join("build/report-directory")).expect("directory output");
    let output = Command::new(env!("CARGO_BIN_EXE_cpp-infra-pages"))
        .current_dir(p.path())
        .args([
            "--project",
            p.path().to_str().expect("project"),
            "--report",
            "build/report-directory",
            "build",
        ])
        .output()
        .expect("CLI");
    assert_eq!(output.status.code(), Some(3));
    assert!(p.path().join("build/report-directory").is_dir());
}

#[test]
fn test_markdown_urls_and_source_link_requirements() {
    let p = fixture();
    fs::create_dir(p.path().join("assets")).expect("assets");
    fs::write(
        p.path().join("assets/icon with space.svg"),
        "<svg xmlns='http://www.w3.org/2000/svg'></svg>",
    )
    .expect("icon");
    let config = p.path().join(".infra/pages/config.toml");
    let mut text = fs::read_to_string(&config).expect("config");
    text.push_str("\n[[assets]]\nsource='assets'\noutput='assets'\n");
    fs::write(&config, &text).expect("assets config");
    fs::write(p.path().join("README.md"),"# URLs\n![Icon](assets/icon%20with%20space.svg)\n[mail](mailto:maintainer@example.test)\n[section](#urls)\n[external](https://example.test/path?q=1)\n").expect("README");
    assert!(call(p.path(), &["build"]).status.success());
    for (reference, code) in [("%FF", 1), ("%XZ", 1), ("../../../escape.html", 2)] {
        fs::write(p.path().join("README.md"), format!("[bad]({reference})")).expect("bad URL");
        assert_eq!(call(p.path(), &["build"]).status.code(), Some(code), "{reference}");
    }
    fs::write(p.path().join("module.hpp"), "void public_api();").expect("source");
    fs::write(p.path().join("README.md"), "[source](module.hpp)").expect("source link");
    assert_eq!(call(p.path(), &["build"]).status.code(), Some(2));
    fs::write(
        config,
        format!("repository='https://github.com/example/project'\n{text}"),
    )
    .expect("repository");
    assert_eq!(
        call(p.path(), &["build"]).status.code(),
        Some(2),
        "source link requires a real revision"
    );
}
#[test]
fn test_static_urls_reject_invalid_hosts_and_paths_outside_the_base() {
    for content in [
        "<img src='http://['>",
        "<base href='http://['><img src='image.svg'>",
        "<a href='/outside/missing.html'>bad</a>",
    ] {
        let p = fixture();
        let config = p.path().join(".infra/pages/config.toml");
        let text = fs::read_to_string(&config)
            .expect("config")
            .replace("base_path='/'", "base_path='/documentation/'");
        fs::write(
            config,
            format!("{text}\n[[assets]]\nsource='assets'\noutput='assets'\n"),
        )
        .expect("assets config");
        fs::create_dir(p.path().join("assets")).expect("assets");
        fs::write(p.path().join("assets/page.html"), content).expect("HTML");
        assert_eq!(call(p.path(), &["build"]).status.code(), Some(1), "{content}");
    }
}
#[test]
fn test_multiple_shared_report_exclusions_remain_sorted_and_reusable() {
    let p = fixture();
    let mut context = initial(p.path());
    context["source_excluded_paths"] = json!(["second.json", "first.json"]);
    fs::write(
        p.path().join("build/context.json"),
        serde_json::to_vec(&context).expect("context"),
    )
    .expect("context file");
    assert!(
        call(p.path(), &["--run-context", "build/context.json", "plan"])
            .status
            .success()
    );
    let result = report(p.path());
    assert_eq!(result["source_excluded_paths"], json!(["first.json", "second.json"]));
    for name in ["first.json", "second.json"] {
        fs::write(p.path().join(name), serde_json::to_vec(&result).expect("report")).expect("shared report");
    }
    assert!(
        call(p.path(), &["--run-context", "build/context.json", "plan"])
            .status
            .success()
    );
}
#[cfg(unix)]
#[test]
fn test_non_utf8_project_filenames_are_explicit_input_errors() {
    use std::os::unix::ffi::OsStringExt;
    let p = fixture();
    fs::write(
        p.path().join(std::ffi::OsString::from_vec(vec![b'x', 0xff])),
        "invalid filename",
    )
    .expect("file");
    assert_eq!(call(p.path(), &["plan"]).status.code(), Some(2));
}
#[cfg(unix)]
fn fake_backend(project: &Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let directory = project.join("build/backend-bin");
    fs::create_dir_all(&directory).expect("backend bin");
    let program = directory.join("doxygen");
    fs::write(&program,r#"#!/usr/bin/env python3
import os,pathlib,re,sys
mode=os.environ.get('PAGES_BACKEND_MODE','success')
if '--version' in sys.argv:
    if mode=='version-failure': sys.exit(2)
    if mode!='version-empty': print('1.9.8-fixture-'+mode)
    sys.exit(0)
text=pathlib.Path(sys.argv[1]).read_text()
assert '"FEATURE=1"' in text
output=pathlib.Path(re.search(r'OUTPUT_DIRECTORY = "([^"]+)"',text).group(1))/'html'
output.mkdir(parents=True)
(output/'index.html').write_text('' if mode=='empty-index' else '<a href="demo_8hpp.html">API</a><link rel="stylesheet" href="doxygen.css">')
if mode!='no-documents': (output/'demo_8hpp.html').write_text('<h1>Fresh API fixture</h1>')
(output/'doxygen.css').write_text("a{background:url('../tab.svg')}b{background:url('mag.svg')}")
(output/'tab.svg').write_text('<svg xmlns="http://www.w3.org/2000/svg"></svg>')
(output/'search').mkdir()
(output/'search/mag.svg').write_text('<svg xmlns="http://www.w3.org/2000/svg"></svg>')
if mode!='no-entities': pathlib.Path(re.search(r'GENERATE_TAGFILE = "([^"]+)"',text).group(1)).write_text('<tagfile><compound kind="file"><name>demo.hpp</name></compound></tagfile>')
"#).expect("backend");
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).expect("executable");
    program
}
#[cfg(unix)]
#[test]
fn test_backend_contract_success_path_discovery_cache_and_output_failures() {
    for mode in [
        "success",
        "version-empty",
        "version-failure",
        "empty-index",
        "no-documents",
        "no-entities",
    ] {
        let p = fixture();
        let program = fake_backend(p.path());
        let config = p.path().join(".infra/pages/config.toml");
        fs::write(
            &config,
            fs::read_to_string(&config).expect("config").replace(
                "enabled=false",
                "enabled=true\ninputs=['include']\ndefines=['FEATURE=1']",
            ),
        )
        .expect("API config");
        fs::create_dir(p.path().join("include")).expect("include");
        fs::write(p.path().join("include/first.hpp"), "void first();").expect("first header");
        fs::write(p.path().join("include/second.hpp"), "void second();").expect("second header");
        let invoke = |context: bool| {
            let mut command = Command::new(env!("CARGO_BIN_EXE_cpp-infra-pages"));
            command
                .current_dir(p.path())
                .env_remove("CPP_INFRA_DOXYGEN")
                .env("PAGES_BACKEND_MODE", mode)
                .env(
                    "PATH",
                    format!(
                        "{}:{}",
                        program.parent().expect("bin").display(),
                        std::env::var("PATH").expect("PATH")
                    ),
                )
                .args([
                    "--project",
                    p.path().to_str().expect("project"),
                    "--report",
                    "build/report.json",
                ]);
            if context {
                command.args(["--run-context", "build/context.json"]);
            }
            command.arg("api").output().expect("API CLI")
        };
        let first = invoke(false);
        assert_eq!(
            first.status.code(),
            Some(match mode {
                "success" => 0,
                "version-empty" => 2,
                _ => 1,
            }),
            "{mode}: {}",
            String::from_utf8_lossy(&first.stderr)
        );
        if mode != "success" {
            continue;
        }
        let public = report(p.path());
        let api = std::path::PathBuf::from(public["report_root"].as_str().expect("root")).join(
            public["artifacts"]
                .as_array()
                .expect("artifacts")
                .iter()
                .find(|entry| entry["kind"] == "api")
                .expect("API artifact")["path"]
                .as_str()
                .expect("API path"),
        );
        let mut context = public.clone();
        context["profile"] = json!("docs");
        context["artifacts"] = json!([]);
        fs::write(
            p.path().join("build/context.json"),
            serde_json::to_vec(&context).expect("context"),
        )
        .expect("context file");
        assert!(invoke(true).status.success());
        assert_eq!(
            report(p.path())["commands"].as_array().expect("commands").len(),
            1,
            "an unchanged API cache only probes the backend version"
        );
        fs::write(api.join("demo_8hpp.html"), "tampered API cache").expect("cache corruption");
        assert!(invoke(true).status.success());
        assert_eq!(
            report(p.path())["commands"].as_array().expect("commands").len(),
            2,
            "cache corruption requires regeneration"
        );
    }
}

#[cfg(unix)]
#[test]
fn test_report_outputs_in_build_and_target_cannot_overlap_api_inputs() {
    let mut observations = Vec::new();
    for root in ["build/generated", "target/generated"] {
        for collision in [
            "existing",
            "new-json",
            "nested-new",
            "directory",
            "ancestor",
            "initialization-error",
        ] {
            let p = fixture();
            let program = fake_backend(p.path());
            let directory = p.path().join(root);
            fs::create_dir_all(&directory).expect("generated input");
            let header = directory.join("demo.hpp");
            let original = b"/** @file demo.hpp */\n/** Public function. */\nint demo();\n";
            fs::write(&header, original).expect("API header");
            let config = p.path().join(".infra/pages/config.toml");
            fs::write(
                &config,
                fs::read_to_string(&config).expect("config").replace(
                    "enabled=false",
                    &format!("enabled=true\ninputs=['{root}']\ndefines=['FEATURE=1']"),
                ),
            )
            .expect("API config");
            let output = match collision {
                "existing" | "initialization-error" => header.clone(),
                "new-json" => directory.join("new-report.json"),
                "nested-new" => directory.join("future/report.data"),
                "directory" => directory.clone(),
                "ancestor" => directory.parent().expect("parent").to_path_buf(),
                _ => unreachable!(),
            };
            let existed = output.exists();
            let mut command = Command::new(env!("CARGO_BIN_EXE_cpp-infra-pages"));
            command.current_dir(p.path()).env("CPP_INFRA_DOXYGEN", program).args([
                "--project",
                p.path().to_str().expect("project"),
                "--report",
                output.to_str().expect("report"),
            ]);
            if collision == "initialization-error" {
                fs::write(p.path().join("build/invalid-context.json"), "not JSON").expect("invalid context");
                command.args(["--run-context", "build/invalid-context.json"]);
            }
            let result = command.arg("api").output().expect("CLI");
            observations.push((
                format!("{root}:{collision}"),
                result.status.code(),
                fs::read(&header).expect("header bytes") == original,
                existed || !output.exists(),
            ));
        }
    }
    assert!(
        observations
            .iter()
            .all(|(_, code, preserved, no_new_input)| *code == Some(2) && *preserved && *no_new_input),
        "all output/input collisions must be rejected without mutations: {observations:?}"
    );
}
#[cfg(unix)]
#[test]
fn test_report_output_aliases_and_invalid_content_fallbacks_preserve_inputs() {
    use std::os::unix::fs::symlink;
    let mut observations = Vec::new();
    for role in ["output-alias", "input-alias", "readme", "asset"] {
        let p = fixture();
        let directory = p.path().join("build/generated");
        fs::create_dir_all(&directory).expect("input directory");
        let file = directory.join("owned.data");
        let original = b"PROJECT_INPUT_MUST_SURVIVE";
        fs::write(&file, original).expect("owned input");
        let config = p.path().join(".infra/pages/config.toml");
        let mut text = fs::read_to_string(&config).expect("config");
        let mut output = file.clone();
        match role {
            "output-alias" => {
                text = text.replace("enabled=false", "enabled=false\ninputs=['build/generated']");
                symlink(&directory, p.path().join("report-alias")).expect("output alias");
                output = p.path().join("report-alias/owned.data");
            }
            "input-alias" => {
                symlink(&directory, p.path().join("generated-alias")).expect("input alias");
                text = text.replace("enabled=false", "enabled=false\ninputs=['generated-alias']");
            }
            "readme" => text = text.replace("readme='README.md'", "readme='build/generated/owned.data'"),
            "asset" => text.push_str("\n[[assets]]\nsource='build/generated'\noutput='generated-assets'\n"),
            _ => unreachable!(),
        };
        fs::write(config, text).expect("input roles");
        fs::write(p.path().join("build/invalid-context.json"), "not JSON").expect("invalid context");
        let result = Command::new(env!("CARGO_BIN_EXE_cpp-infra-pages"))
            .current_dir(p.path())
            .args([
                "--project",
                p.path().to_str().expect("project"),
                "--report",
                output.to_str().expect("report"),
                "--run-context",
                "build/invalid-context.json",
                "plan",
            ])
            .output()
            .expect("CLI");
        observations.push((
            role,
            result.status.code(),
            fs::read(file).expect("input bytes") == original,
        ));
    }
    assert!(
        observations
            .iter()
            .all(|(_, code, preserved)| *code == Some(2) && *preserved),
        "fallback must validate canonical input roles too: {observations:?}"
    );
}
#[test]
fn test_final_report_guard_revalidates_roles_after_execution_changes_config() {
    use std::io::Read;
    use std::process::Stdio;
    let p = fixture();
    let mut context = initial(p.path());
    context["profile"] = json!("x".repeat(2 * 1024 * 1024));
    fs::write(
        p.path().join("build/context.json"),
        serde_json::to_vec(&context).expect("context"),
    )
    .expect("context file");
    let report_path = p.path().join("build/newly-owned.json");
    let mut child = Command::new(env!("CARGO_BIN_EXE_cpp-infra-pages"))
        .current_dir(p.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .args([
            "--project",
            p.path().to_str().expect("project"),
            "--report",
            report_path.to_str().expect("report"),
            "--run-context",
            "build/context.json",
            "plan",
        ])
        .spawn()
        .expect("plan");
    let mut marker = [0];
    child
        .stdout
        .as_mut()
        .expect("stdout")
        .read_exact(&mut marker)
        .expect("execute output began");
    let config = p.path().join(".infra/pages/config.toml");
    fs::write(
        &config,
        fs::read_to_string(&config)
            .expect("config")
            .replace("enabled=false", "enabled=false\ninputs=['build']"),
    )
    .expect("new input role");
    let output = child.wait_with_output().expect("finish");
    assert_eq!(output.status.code(), Some(2));
    assert!(
        !report_path.exists(),
        "an error report must not overwrite a newly declared input subtree"
    );
}
