# cpp-infra-pages

[![CI](https://github.com/qubit-ltd/cpp-infra-pages/actions/workflows/ci.yml/badge.svg)](https://github.com/qubit-ltd/cpp-infra-pages/actions/workflows/ci.yml)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.94%2B-orange.svg)](Cargo.toml)
[![Protocol](https://img.shields.io/badge/protocol-1-blue.svg)](src/report.rs)
[![Version](https://img.shields.io/badge/version-0.1.0-blue.svg)](Cargo.toml)
[![English](https://img.shields.io/badge/README-English-blue.svg)](README.md)

为 C++ 项目把 README、Doxygen API 文档和经过身份校验的质量报告组装成可移植静态站点。维护者可以直接发布 CI 审阅过的同一份文档，避免误用上次构建的绿色徽章，或把不同源码版本的报告拼在一起。

## 安装

使用 Rust 1.94 或更新版本从当前源码构建：

```text
cargo build --locked --release
./target/release/cpp-infra-pages --version --json
```

启用 API 文档后需要安装 Doxygen。真实集成测试已使用 Doxygen 1.9.8 验证；可通过 `CPP_INFRA_DOXYGEN` 指定可执行文件，否则从 PATH 查找 `doxygen`。包括 `deploy` 在内，工具均不发送网络请求。

## 快速开始

假设目标项目已有 `README.md`，先复制配置示例，再生成单语站点：

```text
mkdir -p /path/to/project/.infra/pages
cp conf/pages/config.example.toml /path/to/project/.infra/pages/config.toml
./target/release/cpp-infra-pages --project /path/to/project --report /path/to/project/build/pages-report.json build
```

检查 `build/pages-report.json`：成功报告的 `status` 为 `passed`，其中 `site` 产物的 `path` 相对于 `report_root`。用本地 HTTP 服务打开该目录的 `index.html`。默认 `base_path = "/"` 适合服务根路径；发布到仓库路径时改为 `"/repository/"`。

需要中文页面时增加 `[languages.zh_CN]`，填写 `readme` 和 `label`。需要 API 页面时启用 `[api]`，用 `inputs` 声明自有头文件目录，并为文件和成员编写文档。`api` 和 `build` 都会在 Doxygen 失败或没有生成已文档化的 API 实体时失败；文档警告默认视为错误。`api.timeout_seconds` 也限制运行锁的等待时间（最少一秒）；其他调用持锁时仍能响应取消。

## 命令与输入

- `plan` 只验证本地配置并输出计划，不调用 Doxygen。
- `api` 生成 API HTML；`build` 组装完整站点。
- `artifact` 构建站点并生成可重复的 `pages.tar.gz` 和 `artifact.json`。
- `deploy` 另生成 `deploy-handoff.json`，实际认证上传和部署交给 GitHub Actions。

全局参数包括 `--project`、`--report`、`--run-context`、`--input-dir` 和 `--offline`。公共库提供具体错误类型 `PagesError` 与 CLI 入口 `run()`；当前通过可执行文件使用站点生成能力。

覆盖率公共报告和 CI 的 `quality-summary` 可通过运行上下文的 `artifacts` 清单或显式 `--input-dir` 传入。工具检查协议、bundle、源码和配置摘要，以及必需任务是否完成。覆盖率 HTML 的摘要校验通过后才会复制，并根据行覆盖率生成 `coverage-badge.json`。需要强制这些输入时，在 `[reports]` 设置 `coverage_required` 和 `ci_required`。放行前还会严格检查公共报告结构、生产工具身份、任务 required 布尔类型及 ID 唯一性。输入目录应只包含报告及 JSON 证据；工具不会自动搜索旧构建目录。

用 `[[assets]]` 的 `source` 与 `output` 声明静态资源。普通 README 和资源内容不能放在根 `.git`、`build`、`target` 或 `.infra` 受控事务目录中，资源子树也会检查。`build` 中的 API 生成输入仍通过独立内容摘要支持。执行结束后，写成功公共报告前还会最终检查输入新鲜度。README 相对图片链接转换为对应资源路径，已声明语言的 README 链接转换为语言页面。源码链接需要顶层可选配置 `repository = "https://github.com/owner/repository"`，并要求存在 Git 源码 revision。页面会转义标签和元数据，清洗 README 中的 HTML。缺失站内目标、路径越界、符号链接、文件映射覆盖和大小写折叠冲突均会失败；外部 HTTP 链接只检查语法，不联网探测。

公共 `report_root` 固定为 `build/infra-runs/<run-id>`，每次调用在其下独占不可覆盖的 `pages/<unique-call-id>/` 目录；工件和日志引用仍相对公共根目录。组装失败会保留之前调用的成功站点；复用 API 时，会将校验后的缓存复制到本次私有目录；源码或配置改变后，旧运行上下文会被拒绝。复用 API 还要求 Doxygen 版本、内置模板、独立计算的输入目录及产物目录摘要一致。API 生成输入可位于 build；项目策略则在读取前和 fresh 时校验，不能通过别名指向被排除的构建或缓存目录。归档只包含排序后的普通文件，并固定时间戳和权限。`conf/manifest.json` 将受管理策略和项目自有配置示例分开声明。

C++ 项目可以默认追踪 cpp-infra 的 `main`。运行上下文通过 `runtime_snapshot` 和 `runtime_snapshot_digest` 固定本次实际选择的快照；Pages 校验 bundle、版本、revision、协议与可执行文件摘要，不要求它与项目持久锁文件相等。直接运行开发源码时，bundle 标记为 `development`。

项目根目录的报告 JSON 通过经过安全校验的 `source_excluded_paths` 排除在源码摘要之外，上下文与质量证据共享同一列表。已配置的源码、测试、生成、资源和配置输入不能排除或覆盖，现有非报告 JSON 也会保留。发布产物前会重新核验源码与配置摘要。Windows 取消调用 `taskkill /PID <pid> /T /F`，Unix 则终止子进程组。

## 测试

```text
cargo +nightly fmt --all -- --config-path .infra/style/rustfmt.toml --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
PAGES_TEST_DOXYGEN=/path/to/doxygen cargo test --locked -- --include-ignored
```

默认测试覆盖真实 Markdown 解析、资源与链接、摘要、归档和进程错误。另有两个明确标记为忽略的环境集成测试，需要 Doxygen，验证真实生成、文档警告失败、缓存复用及生成头文件变化后的缓存失效；最后一条命令会运行这两个测试。Unix 进程测试夹具还需要 Python 3。跨工具端到端验收由协调项目负责；本仓库 CI 只验证当前 crate。

## 许可证

采用 [Apache-2.0](LICENSE) 许可证。

## 贡献

请限定修改范围，对可观察行为提供回归证据，并在发起审阅前运行以上检查。

## 作者

Haixing Hu（<starfish.hu@gmail.com>）。项目入口见[仓库](https://github.com/qubit-ltd/cpp-infra-pages)。
