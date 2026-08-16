<div align="center">
  <img src="docs/assets/taskrail-topology.svg" alt="Taskrail 将 VibeCleaner、Mole、Homebrew、restic、rclone、本地任务和 ChatGPT 接入统一的自动化控制平面，负责调度、安全执行和审计历史" width="960" />

  <p>
    <a href="https://github.com/Yuxin-Qiao/Taskrail/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/Yuxin-Qiao/Taskrail/ci.yml?branch=main&style=flat-square&label=CI&logo=githubactions&logoColor=white" alt="CI 状态" /></a>
    <a href="https://github.com/Yuxin-Qiao/Taskrail/releases/latest"><img src="https://img.shields.io/github/v/release/Yuxin-Qiao/Taskrail?style=flat-square&label=%E7%89%88%E6%9C%AC&color=2563eb" alt="最新版本" /></a>
    <a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/Rust-1.88%2B-dea584?style=flat-square&logo=rust&logoColor=white" alt="使用 Rust 1.88+ 构建" /></a>
    <a href="LICENSE"><img src="https://img.shields.io/badge/%E8%AE%B8%E5%8F%AF%E8%AF%81-Apache--2.0-f97316?style=flat-square" alt="Apache 2.0 许可证" /></a>
  </p>

  <p>
    <a href="#实际支持的运行目标"><img src="https://img.shields.io/badge/macOS-Apple%20Silicon%20(aarch64)-000000?style=flat-square&logo=apple&logoColor=white" alt="Apple Silicon macOS" /></a>
    <a href="#实际支持的运行目标"><img src="https://img.shields.io/badge/Linux-ARM64%20(glibc)-FCC624?style=flat-square&logo=linux&logoColor=black" alt="ARM64 Linux" /></a>
    <a href="#实际支持的运行目标"><img src="https://img.shields.io/badge/%E6%9E%B6%E6%9E%84-aarch64-0091BD?style=flat-square&logo=arm&logoColor=white" alt="aarch64 架构" /></a>
    <a href="#实际支持的运行目标"><img src="https://img.shields.io/badge/Windows%20%2F%20x86__64-%E4%B8%8D%E6%94%AF%E6%8C%81-71717a?style=flat-square&logo=windows&logoColor=white" alt="Windows / x86_64 不支持" /></a>
  </p>
  <p><sub>核心 CLI/TUI 不需要 Node.js、Python、独立 SQLite 或 OpenSSL。VibeCleaner、Homebrew、Mole、restic、rclone、<code>gh</code>、扫描器、Codex 和 ChatGPT Tunnel 都是可选集成。</sub></p>

  <p><a href="#支持的平台与前置条件">平台与安装</a> · <a href="docs/chatgpt.zh-CN.md">ChatGPT 集成</a> · <a href="README.md">English</a></p>

  <p><sub>可接入</sub> · <a href="https://vibecleaner.app/">VibeCleaner</a> · <a href="https://github.com/tw93/Mole">Mole</a> · <a href="https://github.com/Homebrew/brew">Homebrew</a> · <a href="https://github.com/restic/restic">restic</a> · <a href="https://github.com/rclone/rclone">rclone</a></p>
</div>

<p align="center">
  <sub>发现</sub> &nbsp;→&nbsp; <sub>调度</sub> &nbsp;→&nbsp; <sub>执行</sub> &nbsp;→&nbsp; <sub>检查</sub>
</p>

## 支持的平台与前置条件

Taskrail 是一个运行在本机上的可执行程序。核心 CLI 不需要 Taskrail 服务端、托管账号、
数据库服务、Node.js、Python，也不需要单独安装 SQLite 或 OpenSSL。

### 实际支持的运行目标

官方二进制、CI 和发布验证只覆盖以下目标：

- <img src="https://img.shields.io/badge/macOS-Apple%20Silicon%20(M1--M4)-000000?style=flat-square&logo=apple&logoColor=white" alt="macOS Apple Silicon" /> `aarch64-apple-darwin` — **支持**（内置 LaunchAgent 守护进程监督）
- <img src="https://img.shields.io/badge/Linux-ARM64%20(glibc)-FCC624?style=flat-square&logo=linux&logoColor=black" alt="ARM64 Linux" /> `aarch64-unknown-linux-gnu` — **支持**（`systemd --user` 用户管理器监督）
- <img src="https://img.shields.io/badge/Windows%20%2F%20x86__64-%E4%B8%8D%E6%94%AF%E6%8C%81-71717a?style=flat-square&logo=windows&logoColor=white" alt="Windows / x86_64 不支持" /> `x86_64`、Windows、32 位 ARM、Linux `musl`/Alpine 等 — **不支持**（在编译期主动拒绝）

Intel/AMD `x86_64`、Windows、32 位 ARM、Linux `musl`/Alpine，以及其他架构或操作系统，
都不是支持的发布目标。Rust crate 会在不支持的目标上主动编译失败，不会生成未经验证的二进制文件。
当前没有原生桌面 App；本地界面是 CLI、TUI 和由 daemon 提供的 loopback 浏览器控制台。

### 选择安装方式

#### 方式 A：下载发布包（不需要 Rust）

从 [GitHub Releases](https://github.com/Yuxin-Qiao/Taskrail/releases) 下载与主机匹配的压缩包：

- Apple Silicon macOS：`taskrail-<version>-aarch64-apple-darwin.tar.gz`；
- ARM64 Linux：`taskrail-<version>-aarch64-unknown-linux-gnu.tar.gz`。

请先用对应的 `.sha256` 文件校验，再解压并把二进制放入 `PATH`（将 `<target>` 替换为上面列出的完整 Rust target）：

~~~bash
tar -xzf taskrail-<version>-<target>.tar.gz
mkdir -p "$HOME/.local/bin"
install -m 0755 taskrail "$HOME/.local/bin/taskrail"
export PATH="$HOME/.local/bin:$PATH"
taskrail --version
~~~

如果 `~/.local/bin` 尚未在 PATH 中，请把这条 PATH 设置加入 shell profile。

#### 方式 B：从源码构建

源码安装需要 [Rustup](https://rustup.rs/)、Cargo、Rust `1.88.0` 或更高版本，以及本机 ARM64 的 C 编译器/链接器。
macOS 如果尚未安装 Apple Command Line Tools，请执行下面的命令；Debian/Ubuntu ARM64
请安装系统构建工具：

~~~bash
# macOS：仅在尚未安装 Command Line Tools 时执行
xcode-select --install

# Debian/Ubuntu ARM64
sudo apt-get update
sudo apt-get install build-essential

rustup toolchain install 1.88.0
cargo +1.88.0 install --locked --path crates/taskrail
taskrail --version
~~~

当前 crate 不需要安装 Node.js、Python、独立 SQLite 服务或 OpenSSL。仓库通过
`rust-toolchain.toml` 固定 Rust `1.88.0`；如果 Rustup 已经选中这个版本，也可以省略
命令中的 `+1.88.0`。

### 各平台的服务和界面前置条件

| 能力 | Apple Silicon macOS | ARM64 Linux |
| --- | --- | --- |
| 核心 CLI、TUI、前台 daemon、本地 Registry | 不需要额外软件包 | 不需要额外软件包；使用基于 glibc 的发行版 |
| `taskrail daemon --install` | 使用系统自带 `launchctl` 安装用户级 LaunchAgent | 安装 systemd 用户单元；必须有 `systemctl --user` |
| 无头机器上的后台服务 | LaunchAgent 在用户登录会话中运行 | 如果要在退出登录后继续运行，先执行 `loginctl enable-linger "$USER"` |
| `taskrail gui` | 使用系统自带的 `open` | 使用 `xdg-open`；请安装发行版的 `xdg-utils`，也可以手动打开命令输出的 loopback URL |
| 浏览器控制台 | 同一台主机上的现代浏览器 | 同一台主机上的现代浏览器；控制台始终只监听 loopback |

浏览器和 `taskrail gui` 都是可选的；没有图形桌面时仍可使用 CLI 和 `taskrail tui`。
Linux 容器或极简发行版可以运行前台 CLI，但 `daemon --install` 需要 systemd 用户管理器；
发布目标仍是 GNU libc ARM64，不是 Alpine/musl。

### 可选集成：只安装你需要的工具

核心命令（`add`、`register`、`list`、`run`、`daemon`、`tui`、本地控制台和本地 MCP）不需要下表工具。
Taskrail 不会替你安装这些外部工具。缺少某个工具只会让对应集成不可用；可以用
`taskrail integrations` 或具体集成的 `doctor` 命令检查。

| 能力 | 外部命令或设置 | 平台和说明 |
| --- | --- | --- |
| Mole 清理/分析/状态 | `mo`（Mole） | 仅 macOS；需要单独安装 Mole |
| VibeCleaner 开发者缓存扫描 | `vibecleaner` headless CLI 或兼容 wrapper | 只读扫描；不会驱动公开 GUI DMG |
| Homebrew 清单/服务 | `brew`（Homebrew） | macOS 或 Linux；可选 |
| 备份和仓库检查 | `restic` | macOS 或 Linux；仓库操作还需要配置仓库/密码环境变量引用 |
| 复制和同步 | `rclone` | macOS 或 Linux；需要单独配置 remote |
| GitHub 观察 | `gh`（GitHub CLI） | macOS 或 Linux；目标数据需要认证时先登录 `gh` |
| Mac App Store 清单 | `mas` | 仅 macOS；可选 |
| Apple Shortcuts | `shortcuts` | macOS 自带，不需额外安装；运行 Shortcut 需要审批 |
| Automator、Keyboard Maestro、Raycast、Alfred、Hazel 发现 | 对应的 macOS App | 仅 macOS；应用自有定义只观察，不会导入为任意命令 |
| 安全扫描 | `osv-scanner`、`gitleaks`、`trivy` | macOS 或 Linux；按需分别安装 |
| 系统更新计划 | `topgrade` | macOS 或 Linux；执行需要审批 |
| Codex 执行器 | `codex` CLI | 可选；只有使用 `taskrail codex-run` 时需要 |
| Responses 执行器 | 网络访问和 `OPENAI_API_KEY` 等 API key | 可选；不需要额外 CLI |
| ChatGPT MCP/Tunnel 连接 | `tunnel-client`、OpenAI Secure MCP Tunnel 和本地凭据 | 可选；参阅 [ChatGPT 集成指南](docs/chatgpt.zh-CN.md) |

因此，只安装 Taskrail 就足够完成下面的第一次运行：

~~~bash
taskrail add hello /bin/echo --arg "hello from Taskrail"
taskrail run hello
~~~

容器部署是另一条可选路径。`[deploy/](deploy/)` 下的示例需要 ARM64 Docker 主机和 Docker Compose；
本地 CLI/TUI 使用和 Rust 测试套件都不需要 Docker。该示例是单主机、公开只读 MCP 部署，仍需要
HTTPS/认证边缘，不是通用的托管服务。

## 快速开始

如果从仓库检出目录安装，请使用上面的源码安装命令：

~~~
cargo +1.88.0 install --locked --path crates/taskrail
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
taskrail add weekly-hello /bin/echo --arg "weekly Taskrail run" \
  --every-seconds 604800 --name "Weekly hello"
~~~

在 macOS 上，如果已经单独安装 Mole，也可以用同样的方式调用 `mo clean`；参见下面的可选集成表。

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

守护进程默认每五分钟执行一次只读的本地来源库存刷新。可以用
`--discovery-interval-seconds` 调整间隔；状态和 overview 会报告最近扫描时间、provider
完整性、漂移和已确认消失的任务数量。provider 不可用时不会被当作空 provider，因此不会
凭空产生删除告警。

## ChatGPT 定时任务

Taskrail 可以作为带类型化工具和可选只读 MCP Apps Widget 的 MCP 应用连接到 ChatGPT。当前已验证
连接后的 ChatGPT 客户端可以交互式调用该应用；目标账号中未来 Scheduled 触发仍需实际观察。
ChatGPT Web、Desktop 和 Mobile 使用同一套 MCP 工具契约；ChatGPT 的“Scheduled”页面负责自然语言
调度和通知，Taskrail 则作为 ChatGPT 在选定 ARM64 macOS 或 Linux 主机上调用的本地执行后端。

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
在支持的账号中可以创建类似下面的 Scheduled 任务；首次触发成功前，不应把它当成已验证的完整工作流：

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
- launchd、cron、systemd 用户服务/定时器、Homebrew 服务，以及受支持的 macOS 应用自动化发现
  （Shortcuts、Automator、Keyboard Maestro、Raycast、Alfred 和 Hazel）；应用自有定义保持只读观察；
  发现阶段保持只读观察，Shortcuts 另有类型化、需审批的运行路径；
- 对受支持的用户级原生任务进行显式领养，并记录可回滚的变更；
- 删除没有运行历史的未使用托管定义，同时保留不可变的运行历史；
- 可选的 Codex 和 Responses 兼容 AI 执行器；
- VibeCleaner（只读开发者缓存扫描）、Mole、restic、rclone、GitHub、Homebrew、mas、OSV-Scanner、Gitleaks、Trivy、
  Topgrade 和 Apple Shortcuts 的类型化语义集成；
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
taskrail integration vibecleaner detect
taskrail integration vibecleaner doctor
taskrail integration vibecleaner scan "$HOME/Projects" --min-size-mb 500
taskrail integration restic snapshots
taskrail integration rclone sync ./data remote:backup --dry-run
taskrail integration github pulls Yuxin-Qiao/Taskrail
taskrail integration homebrew outdated
taskrail integration gitleaks scan .
taskrail integration topgrade plan
taskrail integration shortcuts doctor

# 将只读原生集成持久化为周期性自动化
taskrail schedule-integration homebrew-outdated homebrew outdated \
  --every-seconds 86400 --name "Daily Homebrew inventory"
~~~

这些操作使用类型化 argv 计划、有界解析、归一化语义结果、Run/Event/Metric 记录
和适配器验证。写操作和破坏性操作必须绑定到持久化且会过期的审批请求：

VibeCleaner 在这里明确只提供扫描。公开 App 是本地 GUI，Taskrail 不会尝试点击
界面或自动删除文件；当本机存在 headless `vibecleaner` wrapper（或其文档化的
Python CLI 源码）时，适配器会调用 `--cli ... --json`，保留上游 `safe`/`verify`
风险区别，只记录可回收字节，不触碰被扫描目录。使用文档化源码 CLI 时设置
`TASKRAIL_VIBECLEANER_SCRIPT` 指向 Python 源码路径，也可以设置
`TASKRAIL_VIBECLEANER_PYTHON` 指定解释器；未设置时适配器会在 `PATH` 中查找
`vibecleaner` wrapper。

~~~
taskrail approval-request restic-prune
taskrail approval-request shortcuts-run <shortcut-uuid> --confirm
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
界面：已验证的交互式调用可以使用 Taskrail MCP 适配器，而 Taskrail 负责本地发现、
类型化 Automation 定义、执行、审批、历史和日志。ChatGPT 的 Scheduled 任务与
Taskrail 的本地计划是有意分开的两层：前者是仍需实测的外部触发，后者运行持久化的
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

当前软件包版本为 0.1.7，可用于本地命令自动化和私有 ChatGPT 交互式应用控制；未来 Scheduled
触发仍是尚未验证的账号级工作流门槛。
稳定的核心路径是：

~~~
add/register → list → daemon → run → history/logs → tui
~~~

当前实现和剩余发布门槛如下：

| 领域 | 状态 |
| --- | --- |
| Registry、调度器、运行、日志、事件 | 🟢 核心 |
| CLI 和 TUI | 🟢 核心 |
| launchd / cron / systemd / Homebrew 以及受支持的 macOS 应用发现和后台监督 | 🔵 集成；Shortcuts 已具备类型化、需审批的运行能力 |
| 用户级原生任务领养 | 🔵 集成（cron/launchd/systemd） |
| Codex CLI 和 Responses 执行器 | 🟣 可选集成 |
| 原生语义集成 | 🟢 VibeCleaner（扫描） / Mole / restic / rclone / GitHub / Homebrew / mas / 安全扫描器 / Topgrade / Shortcuts |
| 私有 ChatGPT MCP/Tunnel 与 ChatGPT 交互式只读调用 | 🟢 已验证；尚未观察到未来 Scheduled 触发 |
| ChatGPT MCP Apps 本机与 Fleet 只读视图 | 🟢 已实现（私有 MCP） |
| 多主机 fleet 网关和显式主机路由 | 🟢 已实现（私有配置） |
| 公开 ChatGPT 应用托管、审核和发布 | 🟡 外部门槛 |
| ARM64 CLI 发布 | 🟢 [v0.1.7 已发布](https://github.com/Yuxin-Qiao/Taskrail/releases/tag/v0.1.7) |
| Homebrew formula | 🟡 未来 |

## 文档

- [中文文档索引](docs/README.zh-CN.md)
- [English README](README.md)
- [中文 ChatGPT 集成指南](docs/chatgpt.zh-CN.md)
- [团队功能验收清单](docs/ACCEPTANCE_TEAM.zh-CN.md)
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
