# cpp-infra-pages

[![CI](https://github.com/qubit-ltd/cpp-infra-pages/actions/workflows/ci.yml/badge.svg)](https://github.com/qubit-ltd/cpp-infra-pages/actions/workflows/ci.yml)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.94%2B-orange.svg)](Cargo.toml)
[![Protocol](https://img.shields.io/badge/protocol-1-blue.svg)](src/report.rs)
[![Version](https://img.shields.io/badge/version-0.1.0-blue.svg)](Cargo.toml)
[![中文](https://img.shields.io/badge/README-简体中文-red.svg)](README.zh_CN.md)

Build a portable documentation site for a C++ project from its README files, Doxygen API pages, and verified quality reports. C++ maintainers can publish the same documentation reviewed in CI without copying a previous run's coverage badge or mixing source revisions.

## Installation

Build from this checkout with Rust 1.94 or later:

```text
cargo build --locked --release
./target/release/cpp-infra-pages --version --json
```

Doxygen is required when API generation is enabled. The real integration test is verified with Doxygen 1.9.8. Set `CPP_INFRA_DOXYGEN` to an executable path to select an installed version; otherwise `doxygen` is discovered through PATH. Pages performs no network requests, including in `deploy`.

## Quick start

For a project that already has `README.md`, copy the configuration example and build a single-language site:

```text
mkdir -p /path/to/project/.infra/pages
cp conf/pages/config.example.toml /path/to/project/.infra/pages/config.toml
./target/release/cpp-infra-pages --project /path/to/project --report /path/to/project/build/pages-report.json build
```

Read `build/pages-report.json`: a successful run has `status: "passed"` and a `site` artifact whose `path` is relative to `report_root`. Open that directory's `index.html` through a local HTTP server. The default `/` base path works at a server root; set `base_path = "/repository/"` for a project hosted under a repository prefix.

To add a translation, declare `[languages.zh_CN]` with its `readme` and `label`. To publish API pages, enable `[api]`, declare header directories in `inputs`, and document the headers and members. Both `api` and `build` fail if Doxygen fails or produces no documented API entities. Warning enforcement defaults to enabled. The `api.timeout_seconds` budget also bounds waiting for the run lock (with a minimum of one second); cancellation remains responsive while another invocation holds it.

## Operations and inputs

- `plan` reads and validates configuration without invoking Doxygen.
- `api` generates API HTML; `build` assembles the complete site.
- `artifact` builds the site and a reproducible `pages.tar.gz` with `artifact.json`.
- `deploy` additionally writes `deploy-handoff.json`; GitHub Actions performs authenticated upload and deployment.

All commands accept `--project`, `--report`, `--run-context`, `--input-dir`, and `--offline`. The public library exposes the typed `PagesError` and CLI entry point `run()`; site operations are currently consumed through the executable.

Use run-context `artifacts` or an explicitly selected `--input-dir` to supply public coverage reports and CI's `quality-summary`. Pages validates schema, bundle, source/config digests and required task completion. Coverage HTML is copied only after validating its artifact digest; coverage line percentages produce `coverage-badge.json`. Set `coverage_required` and `ci_required` in `[reports]` when those inputs are mandatory. Public report structure, producer identity, boolean task requirements and unique task IDs are validated before any gate can pass. Input directories should contain reports and their JSON evidence only. Pages never scans old build directories implicitly.

Declare assets with `[[assets]]`, `source`, and `output`. Ordinary README and asset content must remain outside root `.git`, `build`, `target` and controlled `.infra` transaction directories; asset subtrees are checked too. Generated API inputs under `build` retain their independent content digest. Successful public reports perform a final freshness check after execution. Relative README images map to those assets, and links to declared language READMEs map to language pages. Source links require the optional top-level `repository = "https://github.com/owner/repository"` and a Git source revision. Generated pages escape labels and metadata and sanitize README HTML. Missing local targets, traversal, symlinks, overlapping files and case-insensitive output collisions fail the build. External HTTP links receive syntax validation without network checks.

Public `report_root` stays at `build/infra-runs/<run-id>`; each invocation owns an immutable `pages/<unique-call-id>/` directory beneath it. Artifact and log references remain relative to the public root. A failed staging build preserves previous calls and their successful sites. API reuse copies a validated cached tree into the new call directory. Contexts reject changed sources/configuration. API reuse also requires matching Doxygen version, bundled template, independently hashed input trees and API directory hashes. Generated API inputs may be under build; project policy inputs are checked before reading and during freshness checks and cannot alias excluded build/cache directories. Archives contain regular files in stable order with normalized timestamps and permissions. `conf/manifest.json` separates managed policy from the project-owned configuration example.

C++ projects may track cpp-infra `main`. A run context can pin the selected runtime snapshot using `runtime_snapshot` and `runtime_snapshot_digest`; Pages checks its bundle, version, revision, protocol and executable hash. It does not require equality with a persistent project lock. Direct development runs identify their bundle as `development`.

Project-root report JSON is excluded from source identity through validated `source_excluded_paths`, shared by contexts and evidence reports. Configured source, test, generated, asset and configuration inputs cannot be excluded or overwritten; existing non-report JSON is preserved. Source and config digests are checked again before publishing generated outputs. Windows cancellation invokes `taskkill /PID <pid> /T /F`; Unix cancellation targets the process group.

## Testing

```text
cargo +nightly fmt --all -- --config-path .infra/style/rustfmt.toml --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
PAGES_TEST_DOXYGEN=/path/to/doxygen cargo test --locked -- --include-ignored
```

Default tests cover real Markdown rendering, assets, links, digests, archives and process failures. Two explicitly ignored integration tests require Doxygen and verify real generation, warning failures, cache reuse and generated-header cache invalidation. They are included by the final command. Unix process fixtures also require Python 3. Cross-tool end-to-end verification belongs to the coordinating project; CI validates this crate only.

## License

[Apache-2.0](LICENSE).

## Contributing

Keep changes scoped and include regression evidence for observable behavior. Run the checks above before requesting review.

## Author

Haixing Hu (<starfish.hu@gmail.com>). See the [project repository](https://github.com/qubit-ltd/cpp-infra-pages).
