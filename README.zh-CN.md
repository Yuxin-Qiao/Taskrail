<div align="center">
  <img src="docs/assets/taskrail-topology.svg" alt="Taskrail 将 Mole、Homebrew、restic、rclone、本地任务和 ChatGPT 接入统一的自动化控制平面，负责调度、安全执行和审计历史" width="960" />

  <p>
    <a href="https://github.com/Yuxin-Qiao/Taskrail/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/Yuxin-Qiao/Taskrail/ci.yml?branch=main&style=flat-square&label=CI" alt="CI 状态" /></a>
    <a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/built%20with-Rust-dea584?style=flat-square&logo=rust&logoColor=white" alt="使用 Rust 构建" /></a>
    <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-f97316?style=flat-square" alt="Apache 2.0 许可证" /></a>
  </p>

  <p><a href="#快速开始">快速开始</a> · <a href="docs/chatgpt.zh-CN.md">ChatGPT 集成</a> · <a href="README.md">English</a></p>

  <p><sub>可接入</sub> · <a href="https://github.com/tw93/Mole">Mole</a> · <a href="https://github.com/Homebrew/brew">Homebrew</a> · <a href="https://github.com/restic/restic">restic</a> · <a href="https://github.com/rclone/rclone">rclone</a></p>
</div>

<p align="center">
  <sub>发现</sub> &nbsp;→&nbsp; <sub>调度</sub> &nbsp;→&nbsp; <sub>执行</sub> &nbsp;→&nbsp; <sub>检查</sub>
</p>

## 支持的目标平台

官方二进制和 CI 只覆盖以下 ARM64 目标：

- Apple Silicon macOS：`aarch64-apple-darwin`；
- ARM64 Linux：`aarch64-unknown-linux-gnu`。

x86_64 和 Windows 不属于支持的发布目标。Rust crate 在其他目标上会直接失败，
不会生成未经验证的二进制文件。其他架构可以自行尝试源码，但不属于项目支持范围。

## 快速开始

在仓库检出目录中安装二进制文件：

~~~
cargo install --path crates/taskrail
~~~

无需编写配置文件即可添加命令：

~~~
taskrail add hello /bin/echo --arg "hello from Taskrail"
taskrail list
taskrail run hello
taskrail runs
taskrail logs <run-id>
# 只有没有任何运行历史的托管定义才能被删除
taskrail delete hello
~~~

最短使用路径是：

~~~
add → run → inspect
~~~

添加周期性任务：

~~~
taskrail add mole-cleanup mo --arg clean \
  --every-seconds 604800 --name "Mole cleanup"
~~~

使用当前平台的用户级服务保持调度器运行：

~~~
taskrail daemon --install
taskrail status
~~~

在 macOS 上，这会安装 LaunchAgent；在 Linux 上，会在
`~/.config/systemd/user/` 下安装 systemd 用户单元。Registry 默认存储在 Linux 的
`$XDG_DATA_HOME/taskrail/`（或 `~/.local/share/taskrail/`）；Unix 守护进程 socket
在可用时使用 `$XDG_RUNTIME_DIR/taskrail/`。对于无头 Linux 主机，请在安装前启用用户 lingering：

~~~
loginctl enable-linger "$USER"
taskrail daemon --install
~~~

守护进程默认每五分钟执行一次只读的原生调度器库存刷新。可以用
`--discovery-interval-seconds` 调整间隔；状态和 overview 会报告最近扫描时间、provider
完整性、漂移和已确认消失的任务数量。provider 不可用时不会被当作空 provider，因此不会
凭空产生删除告警。

## ChatGPT 定时任务

Taskrail 可以作为带类型化工具和可选只读 MCP Apps Widget 的 MCP 应用连接到 ChatGPT。ChatGPT Web、Desktop
和 Mobile 使用同一套 MCP 工具契约；ChatGPT 的“Scheduled”页面负责自然语言
调度和通知，Taskrail 则作为 ChatGPT 在选定 ARM64 macOS 或 Linux 主机上调用的
本地执行后端。

在 Taskrail 守护进程运行后启动本地 MCP 适配器：

~~~
taskrail daemon --install       # 按平台安装 LaunchAgent/systemd
taskrail mcp                    # 当前主机的 MCP stdio 适配器
taskrail integration chatgpt-doctor
~~~

适配器提供状态查询、最新的原生任务发现、自动化创建、暂停和恢复、立即运行、
运行历史、日志、取消、待处理事项和审计事件。守护进程还会在后台维护只读的原生任务
观察镜像；稳定的状态调用会携带安全的监督摘要，而 overview 仍会执行最新扫描。
命令始终以直接 argv 传递；ChatGPT 不能通过该接口把自由文本变成 shell 管道。

对于私有的 ARM64 macOS 或 Linux 主机，请通过 OpenAI Secure MCP Tunnel
连接 `taskrail mcp`，然后将该 Tunnel 添加为 ChatGPT 开发者模式应用。连接应用后，
可以创建类似下面的 Scheduled 任务：

~~~
每周日 09:00，在 MacBook 主机上运行名为“Mole cleanup”的 Taskrail 自动化。
如果运行失败，请检查运行日志，并告诉我需要处理什么问题。
~~~

如果要通过一个 ChatGPT 应用管理多台主机，请复制并修改 fleet 配置；端点使用 HTTPS，
令牌只通过环境变量提供，不要写入 YAML：

~~~bash
mkdir -p ~/.config/taskrail
cp examples/fleet.yaml ~/.config/taskrail/fleet.yaml
# 编辑 endpoint、label 和 token_env；不要提交 ~/.config/taskrail/fleet.yaml
taskrail mcp-fleet --config ~/.config/taskrail/fleet.yaml
~~~

仓库中的示例主机默认处于禁用状态，并使用占位端点；只有你编辑本地副本并显式启用后，
fleet 才会发起出站请求。fleet 工具要求每次主机操作都明确提供 `host_id`；默认只读，远端主机仍负责自己的策略、
审批和执行。参阅 [ChatGPT 集成指南](docs/chatgpt.zh-CN.md)了解 Tunnel、权限和多主机设置。

对于公开部署，使用强制只读的 HTTP 配置：

~~~bash
export TASKRAIL_MCP_BEARER_TOKEN="<从密钥管理器注入>"
taskrail mcp-http --profile public-read-only --bind 127.0.0.1:8787
~~~

此端点默认使用公开只读配置，不提供创建、删除、执行、领养和审批工具。请将其
置于带有终端用户认证和按用户主机绑定的生产 HTTPS 代理之后；本地 Tunnel 只能
用于开发连接。参阅[OpenAI 提交检查清单](docs/OPENAI_SUBMISSION.md)和[单主机
部署示例](deploy/README.md)。

如果私有的单主机 Fleet 目标需要明确的写入或运行请求，可使用
`taskrail mcp-http --profile private`，并配置独立的 Bearer 密钥及私有
TLS/认证边缘。不要把该配置暴露为共享公开中继；Fleet 的 `allow_writes: true`
只应指向这种显式保护的端点。

如需在本地通过 stdio 检查公开只读配置：

~~~
TASKRAIL_MCP_PROFILE=public taskrail mcp
~~~

打开实时终端面板：

~~~
taskrail tui
~~~

守护进程本身也会在 loopback 上提供主浏览器控制台，默认监听
`127.0.0.1:10100`，可以这样打开：

~~~bash
taskrail gui
~~~

控制台展示原生发现、自动化、运行、日志、集成、待处理事项、审批、指标和审计事件。
写操作仍然复用 CLI/TUI 的本地 RPC 与策略边界；它只绑定本机 loopback，写请求必须来自
同源浏览器，并且不会通过 ChatGPT MCP 或 Tunnel 暴露。需要更换端口时使用
`taskrail daemon --http-bind 127.0.0.1:10100` 指定本地地址。如果 `10100` 已被其他本地服务占用，
Taskrail 会自动回退到后续 loopback 端口，`taskrail gui` 会发现真正的 Taskrail 地址，不会打开其他服务。

浏览器控制台支持 English、简体中文、日本語和한국어。首次打开时会根据浏览器语言自动选择；
也可以使用右上角的语言选择器切换。选择只保存在浏览器本地存储中。

ChatGPT MCP 应用可以通过带版本的 MCP Apps resource 在对话内渲染同一份受约束的摘要；可选的
Fleet 网关还提供只读多主机视图。两个 Widget 都只调用类型化 MCP 工具，不会访问本地浏览器 HTTP 接口。

需要更多字段的定义可以使用 YAML：

~~~
taskrail register examples/hello.yaml
taskrail explain hello
taskrail run hello
~~~

命令执行器使用直接 argv，不会把字符串转换成 shell 命令。

## Taskrail 管理什么

Taskrail 可以管理你已经在使用的命令和脚本：

- 一次性命令，以及按间隔或 cron 运行的周期性任务；
- 本地运行历史、标准输出、标准错误和运维事件；
- launchd、cron、systemd 用户服务和 Homebrew 服务发现；
- 对受支持的用户级原生任务进行显式领养，并记录可回滚的变更；
- 删除没有运行历史的未使用托管定义，同时保留不可变的运行历史；
- 可选的 Codex 和 Responses 兼容 AI 执行器；
- Mole、restic、rclone、GitHub、Homebrew、mas、OSV-Scanner、Gitleaks、Trivy
  和 Topgrade 的类型化语义集成；
- 支持只读和 dry-run 调度的持久化、类型化集成自动化；
- 从这些集成中归一化发现结果、指标、变更、产物、运行历史和待处理事项。

Taskrail 会先观察原生任务，再等待显式领养命令。发现过程不会修改主机；当前领养
仅限受支持的用户级来源，并且始终需要显式命令。

## 原生集成

Taskrail 为原生工具提供统一的类型化语义层。例如：

~~~
taskrail integration mole detect
taskrail integration mole doctor
taskrail integration mole analyze
taskrail integration mole status
taskrail integration mole history --limit 20
taskrail integration mole clean --dry-run
taskrail integration restic snapshots
taskrail integration rclone sync ./data remote:backup --dry-run
taskrail integration github pulls Yuxin-Qiao/Taskrail
taskrail integration homebrew outdated
taskrail integration gitleaks scan .
taskrail integration topgrade plan

# 将只读原生集成持久化为周期性自动化
taskrail schedule-integration homebrew-outdated homebrew outdated \
  --every-seconds 86400 --name "Daily Homebrew inventory"
~~~

这些操作使用类型化 argv 计划、有界解析、归一化语义结果、Run/Event/Metric 记录
和适配器验证。写操作和破坏性操作必须绑定到持久化且会过期的审批请求：

~~~
taskrail approval-request restic-prune
taskrail approvals
taskrail approval-decide <approval-id> --approve
taskrail approval-execute <approval-id>
~~~

准确的类型化审批请求子命令请查看 taskrail approval-request --help。获批请求
只能使用一次，并且必须匹配精确的类型化计划指纹。没有审批时，策略边界只会记录
请求，不会启动进程。使用 taskrail integrations 查看完整的内置适配器目录。

## TUI 是主视图

TUI 面向小型、始终可用的本地工具，而不是浏览器控制台。它显示每个自动化的名称、
归属、运行状态、下次运行时间和待处理事项；运行、日志、事件和指标仍可通过 CLI
查看。

~~~
NAME              OWNERSHIP   STATE       NEXT RUN
Mole cleanup      managed     enabled     2026-08-18T...
GitHub watch      observed    paused      manual

Needs attention
failed run        run_failure  high
~~~

## AI 是执行器，而不是产品本身

简单工作应保持为命令：

~~~
每周日 → mo clean
~~~

当任务需要解释和判断时，AI 执行器才更有用：

~~~
每两小时 → 检查 GitHub 状态 → 总结需要关注的内容
~~~

当前仓库包含可选的 Codex CLI 和 Responses 兼容执行器。ChatGPT 是自然语言控制
界面：它的 Scheduled 任务调用 Taskrail MCP 适配器，而 Taskrail 负责本地发现、
类型化 Automation 定义、执行、审批、历史和日志。ChatGPT 的 Scheduled 任务与
Taskrail 的本地计划是有意分开的两层：前者唤醒已连接的应用，后者运行持久化的
本地 Automation。

如果 Codex 安装使用其他工具生成的模型目录，Taskrail 在发现已知不支持的
audio 模态时，会自动创建短期、权限为 0600 的兼容副本；不会修改全局 Codex
配置。也可以显式指定目录：

~~~
taskrail codex-run --cwd . --model-catalog-json /path/to/catalog.json \
  --prompt "inspect the repository"
~~~

## 本地优先行为

- Registry 使用本地 SQLite；
- 运行、日志和事件都记录在本地；
- 命令以 argv 执行，不接受任意 shell 字符串；
- 环境变量值会在持久化的自动化快照中脱敏；
- 已存在的原生任务在显式领养前始终只读观察；
- 守护进程会后台刷新原生任务观察，并把漂移或已确认消失的任务标记为待处理；provider
  不可用时不会因为扫描不到而从 Registry 删除任务；
- ChatGPT MCP 适配器通过受限的本地 Unix socket 访问守护进程，不直接暴露 Registry；
- 审批请求在本地持久化，会过期、绑定计划并且只能消费一次，且不包含密钥值。

## 当前状态

当前软件包版本为 0.1.6，可用于本地命令自动化和私有 ChatGPT Scheduled 任务控制。
稳定的核心路径是：

~~~
add/register → list → daemon → run → history/logs → tui
~~~

当前实现和剩余发布门槛如下：

| 领域 | 状态 |
| --- | --- |
| Registry、调度器、运行、日志、事件 | 🟢 核心 |
| CLI 和 TUI | 🟢 核心 |
| launchd / cron / systemd / Homebrew 发现和后台监督 | 🔵 集成 |
| 用户级原生任务领养 | 🔵 集成（cron/launchd/systemd） |
| Codex CLI 和 Responses 执行器 | 🟣 可选集成 |
| 原生语义集成 | 🟢 Mole / restic / rclone / GitHub / Homebrew / mas / 安全扫描器 / Topgrade |
| 私有 ChatGPT MCP/Tunnel 和 Scheduled 任务控制 | 🟢 本地 runtime/MCP 已验证，等待解锁后复核 ChatGPT UI |
| ChatGPT MCP Apps 本机与 Fleet 只读视图 | 🟢 已实现（私有 MCP） |
| 多主机 fleet 网关和显式主机路由 | 🟢 已实现（私有配置） |
| 公开 ChatGPT 应用托管、审核和发布 | 🟡 外部门槛 |
| ARM64 CLI 发布 | 🟢 [v0.1.6 已发布](https://github.com/Yuxin-Qiao/Taskrail/releases/tag/v0.1.6) |
| Homebrew formula | 🟡 未来 |

## 文档

- [中文文档索引](docs/README.zh-CN.md)
- [English README](README.md)
- [中文 ChatGPT 集成指南](docs/chatgpt.zh-CN.md)
- [ChatGPT integration](docs/chatgpt.md)
- [贡献指南](CONTRIBUTING.md)
- [安全策略](SECURITY.md)
- [验收检查清单](docs/ACCEPTANCE.md)
- [架构决策](docs/adr/)
- [研究笔记](deep-research-report.md)
- [示例自动化](examples/hello.yaml)

ADR 保留历史决策；使用核心 CLI 和 TUI 不需要逐篇阅读 ADR。

## 参与贡献

请从核心用户路径开始，并将集成保持在边缘。提交修改前运行：

~~~
cargo fmt --all -- --check
cargo +1.88.0 clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo +1.88.0 test --locked --workspace --all-features
~~~

## 许可证

Apache-2.0，见 [LICENSE](LICENSE)。
