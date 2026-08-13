# Taskrail 团队功能验收清单

本清单用于让测试、研发、运维和产品团队按同一套标准验收 Taskrail，判断当前实现是否达到
[GitHub 仓库](https://github.com/Yuxin-Qiao/Taskrail)中的公开描述。

它是“执行模板”，不是对历史测试结果的复述。每次执行都必须填写版本、提交、平台、证据和
状态；不能因为仓库中已有单元测试、CI 记录或 `docs/ACCEPTANCE.md` 的历史记录，就把本次
黑盒验收直接标记为通过。

## 1. 验收基线

| 项目 | 本次基线 |
| --- | --- |
| 仓库 | `Yuxin-Qiao/Taskrail` |
| 基线分支 | `main` |
| 本次核对的 main 提交 | `7e2626722c058a3c34cc0571d96f46cc007efcf6`（2026-08-13） |
| 已发布版本 | `v0.1.7`，标签提交 `7f307066dd8437d415decd5f9bbf5183a9a76454` |
| 产品范围 | ARM64 macOS、ARM64 Linux |
| 主要参考 | [README](../README.md)、[中文 README](../README.zh-CN.md)、[中文文档索引](README.zh-CN.md)、[ChatGPT 集成指南](chatgpt.zh-CN.md)、[OpenAI 提交清单](OPENAI_SUBMISSION.md)、[部署说明](../deploy/README.md) |
| 本次执行日期 | ____________________ |
| 测试负责人 | ____________________ |
| 被测版本/提交 | ____________________ |

### 1.1 状态定义

- `待测`：尚未执行或证据不完整。
- `通过`：实际操作结果符合“通过标准”，并有可复核证据。
- `失败`：实际结果不符合标准；必须创建缺陷记录。
- `阻塞`：由于环境、权限、外部服务或缺少凭据无法执行；不能当作通过。
- `不适用`：该平台或部署形态不支持；必须写明理由和替代证据。
- `外部未验证`：仓库实现已具备或本地测试通过，但需要 ChatGPT 账号、生产部署、审核或真实用户操作才能完成。

### 1.2 发布判定

1. 所有适用的 `P0` 项必须通过；任一安全边界、数据完整性或核心用户路径的 `P0` 失败，整体不通过。
2. `P1` 项应全部通过；若有阻塞，必须在发布决策中明确责任人、风险和补测日期。
3. `P2` 和外部闸门单独报告，不得用“本地代码通过”替代账号级、生产级或审核级验证。
4. 单元测试、CI 绿色、静态代码检查只能作为证据之一；README 声明的用户行为必须至少有一次真实黑盒或等价隔离环境验证。
5. 任何真实破坏性操作、原生任务领养、远程写入或生产部署都必须使用专门的隔离环境和明确授权；本清单默认只要求证明“未授权时 fail-closed”。

## 2. 测试安全和统一记录规范

### 2.1 隔离环境

普通 CLI、调度、daemon、MCP 和 dashboard 测试使用临时 Registry，建议先执行：

```bash
export TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/taskrail-acceptance.XXXXXX")"
export TASKRAIL_REGISTRY="$TEST_ROOT/registry.sqlite3"
export XDG_DATA_HOME="$TEST_ROOT/data"
export XDG_RUNTIME_DIR="$TEST_ROOT/runtime"
mkdir -p "$XDG_RUNTIME_DIR"
chmod 700 "$XDG_RUNTIME_DIR"
taskrail --version
```

测试团队也可以将 `taskrail` 替换为：

```bash
cargo run --locked --package taskrail --
```

但每条命令都必须带同一个临时 Registry 配置。结束后删除 `TEST_ROOT`，不得污染真实的
`~/.local/share/taskrail/`、`~/Library`、launchd、cron、systemd、Homebrew 或应用自有配置。

### 2.2 每一项必须记录

```text
测试 ID：
执行人 / 日期：
平台 / 架构 / OS 版本：
被测版本 / commit：
前置条件和测试数据：
实际命令或操作：
预期结果：
实际结果：
证据路径或链接：
状态：待测 / 通过 / 失败 / 阻塞 / 不适用 / 外部未验证
缺陷号 / 后续动作：
```

证据至少应包含一种：命令输出、结构化 JSON、截图/录屏、日志、CI run 链接、发布资产链接、
网络请求记录或文件前后哈希。输出中若出现密钥、Cookie、Authorization、私有仓库内容或私人路径，
先脱敏再归档。

### 2.3 禁止直接使用真实数据的项目

下列测试只能使用 fixture、临时文件、模拟服务或专门测试主机：

- 原生任务 adoption、rollback、删除和 drift acknowledge；
- Mole clean、restic backup/forget/prune、rclone copy/sync、Homebrew upgrade/cleanup、Topgrade run；
- Shortcut run、任何远程写入或带真实权限的 GitHub 操作；
- 生产 HTTPS、公开 MCP endpoint、ChatGPT 应用审核和发布。

如果团队确实执行了真实操作，必须额外记录授权人、目标、执行前后状态、回滚方式和残余风险。

## 3. P0 核心用户路径

### CORE-01 安装、版本和 CLI 可用性

- **优先级/角色**：P0，测试/研发；ARM64 macOS 和 ARM64 Linux。
- **操作**：从 checkout 执行 `cargo install --locked --path crates/taskrail`，或使用发布包安装；执行 `taskrail --version`、`taskrail --help` 以及各一级子命令的 `--help`。
- **通过标准**：安装成功；版本与被测 commit/package 一致；帮助文本可发现 `add/register → schedule → run → history/logs`、daemon、MCP、GUI/TUI、集成、审批和状态相关入口；非法参数以非零退出，不静默忽略。
- **证据/状态**：输出或录屏：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞

### CORE-02 最小路径：add → list → run → runs → logs

- **操作**：

  ```bash
  taskrail add hello /bin/echo --arg "hello from Taskrail"
  taskrail list --json
  taskrail run hello
  taskrail runs --automation hello --limit 20
  taskrail logs <run-id>
  ```

- **通过标准**：定义被创建且 ownership 为 `managed`；命令按 argv 执行；运行记录有开始/结束时间、状态、退出码和自动化快照；日志能读到预期 stdout；重复查看不会改变记录。
- **证据/状态**：run ID：____________________；证据：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞

### CORE-03 YAML register、inspect 和 explain

- **操作**：使用 `examples/hello.yaml` 或等价临时 YAML 执行 `register`、`list`、`inspect hello`、`explain hello`。
- **通过标准**：YAML 中的 name、ownership、trigger、timeout、executable、args、cwd、env、shell 均按定义落库；`explain` 只解释计划，不启动进程；再次 register 的冲突行为明确且不会产生重复定义或部分写入。
- **证据/状态**：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞

### CORE-04 直接 argv 和 shell 边界

- **操作**：将带有 `;`, `&&`, `|`, `$()`, 反引号、重定向和换行的字符串作为普通参数传给 `/bin/echo`；另用 `shell: true` 或等价非法定义测试；准备一个“若被 shell 执行就会创建文件”的参数并监测文件系统。
- **通过标准**：字符串只作为一个 argv 参数传递；不会创建意外文件、执行第二个命令、读取未授权文件或展开环境变量；显式 shell 定义在 Registry 写入或运行前被拒绝；失败不会留下半成品自动化或运行记录。
- **证据/状态**：攻击字符串及文件前后状态：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞

### CORE-05 成功、失败、超时和日志边界

- **操作**：分别运行成功命令、非零退出命令、写入 stdout/stderr 的命令和超过 timeout 的命令。
- **通过标准**：成功/失败/超时状态可区分；退出码准确；stdout、stderr 均可通过 `logs` 读取且有大小边界；超时会终止或回收子进程；不会把一个运行的输出串到另一个运行；错误信息足够定位失败原因。
- **证据/状态**：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞

### CORE-06 删除保护和历史不可变

- **操作**：删除一个没有运行历史的 managed 自动化；再尝试删除有运行历史的 managed 自动化、observed 自动化和 adopted 自动化。
- **通过标准**：只有“托管且没有运行历史”的定义可删除；有历史的定义拒绝删除并保留 immutable run history；observed/adopted 定义不会被普通 `delete` 删除；拒绝操作不修改 Registry。
- **证据/状态**：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞

### CORE-07 暂停、恢复和运行取消

- **操作**：创建短周期任务，验证 `pause` 后不再调度，`resume` 后恢复；让一个长时间运行的命令执行 `cancel <run-id>`。
- **通过标准**：暂停只改变 Taskrail 控制状态，不改原生源；恢复后的 next run 正确；取消后的运行状态、结束时间和事件准确；进程被回收且不会被调度器重复拉起；取消不存在/已结束 run 返回明确错误。
- **证据/状态**：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞

### CORE-08 runs、events、metrics、inbox 和 doctor

- **操作**：制造一次成功、一次失败、一次暂停或 drift 事件，分别执行 `runs`、`events`、`metrics`、`inbox` 和 `doctor` 的全量及 `--limit` 版本。
- **通过标准**：各 read model 信息一致、排序稳定、数量受限；失败、漂移、暂停和 integration attention 能进入 inbox；doctor 能报告 ownership、permissions、drift、adoption 等检查结果；只读查询不改变状态。
- **证据/状态**：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞

### CORE-09 Registry 持久化和重启恢复

- **操作**：创建自动化并产生运行记录，停止/重启 CLI 或 daemon，再读取 `list`、`runs`、`logs`、`events`、`metrics`；人为中断或模拟 daemon 重启。
- **通过标准**：Registry、运行、日志、事件、审批和 adoption journal 重启后仍可读取；不产生重复 run；活跃运行的恢复状态符合策略；损坏或锁冲突时 fail-closed，不覆盖原库。
- **证据/状态**：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞

## 4. 调度器验收

### SCHED-01 interval 调度

- **操作**：创建 `--every-seconds` 为短间隔的无害命令，运行 daemon 多个周期；记录 run 数、间隔和 next run。
- **通过标准**：至少连续产生 3 次正确运行；不重复执行同一个调度 tick；pause 后无新运行，resume 后恢复；运行历史与日志完整。
- **证据/状态**：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞

### SCHED-02 cron、timezone、misfire 和 DST

- **操作**：在 fixture 时间或可控时钟环境创建五字段 cron，覆盖本地时区、明确时区、daemon 停机期间错过触发、夏令时重复/缺失时间；必要时使用项目测试 fixture。
- **通过标准**：cron 解析严格；next run 使用声明的时区；misfire 行为符合文档和 ADR；DST 不产生重复或永远不运行的任务；非法 cron 不落库。
- **证据/状态**：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞

### SCHED-03 overlap、超时和 run admission

- **操作**：让一次执行时间超过调度周期，观察是否重叠；同时触发 pause、cancel、daemon restart。
- **通过标准**：重叠策略稳定且可解释；不会因为并发检查导致重复启动、逃逸进程或错误历史；运行状态变更有事件记录；策略拒绝时不 spawn 新进程。
- **证据/状态**：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞

## 5. 原生任务发现、漂移和领养

### DISC-01 原生来源覆盖

- **操作**：在具备条件的 ARM64 主机或 fixture 中执行 `scan --json`，分别覆盖 `all`、`launchd`、`cron`、`systemd`、`homebrew`、`shortcuts`、`automator`、`keyboard-maestro`、`raycast`、`alfred`、`hazel`。
- **通过标准**：支持的 provider 被单独报告；结果包含安全摘要、source、ownership/execution 标记、状态和必要的调度信息；不存在的 provider 返回明确 unavailable/empty 语义，不伪造数据。
- **平台备注**：macOS app provider 在 Linux 上标记为不适用；Linux systemd/cron 在 macOS 上标记为不适用，不得因此判失败。
- **证据/状态**：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞 □ 不适用

### DISC-02 发现必须只读

- **操作**：扫描前后保存原生定义文件、plist、cron、systemd unit/timer、Homebrew service 和 app provider 元数据的哈希；同时比较 Registry 文件和事件。
- **通过标准**：发现不会创建、删除、启停、改写或领养原生任务；MCP/CLI 响应明确 `native_definitions_changed=false` 或等价事实；普通 discovery 不产生 adoption 或写入类事件。
- **证据/状态**：前后哈希：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞

### DISC-03 应用自有任务和 systemd timer 观察模式

- **操作**：准备 Shortcuts、Automator、Keyboard Maestro、Raycast、Alfred、Hazel 和 systemd timer fixture，读取扫描和 dashboard/MCP 结果。
- **通过标准**：应用自有定义不导入动作正文或原始输出；明确标记 `observe_only`；systemd timer 的安全调度摘要仍可展示，但不能被当成可任意修改的 managed command；只有 Shortcuts 走独立的 typed、fresh-UUID、approval-gated run path。
- **证据/状态**：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞

### DISC-04 provider 不可用时不得制造删除/消失告警

- **操作**：先让 provider 成功发现并记录一个源，再让该 provider 不可用或权限失败，执行后台 refresh、`status`、`overview`、`inbox` 和 fresh scan。
- **通过标准**：不可用 provider 不被解释为空列表；Registry 中既有观察项不被删除或标记为 confirmed missing；响应指出 provider unavailable；只有成功查询且确认不存在时才可产生 missing/drift attention。
- **证据/状态**：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞

### DISC-05 漂移、基线和 attention

- **操作**：在 fixture 中修改已观察源的安全属性，执行 `status`、`overview`、`inbox`；分别执行 `acknowledge-drift --dry-run` 和未授权/授权的 `--apply`。
- **通过标准**：漂移被记录并进入 attention；dry-run 不改变基线；确认漂移必须显式 apply；确认后保持所属自动化暂停，直至明确 resume；事件包含前后基线和操作者可识别信息。
- **证据/状态**：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞

### ADOPT-01 adoption dry-run 和前置检查

- **操作**：对专门的 user-level launchd/cron/systemd fixture 执行 `adopt <id> --dry-run`，检查输出和 Registry。
- **通过标准**：输出拟修改内容、来源、目标 ownership、冲突和回滚信息；不修改原生定义、Registry 或启动进程；不满足支持条件时明确拒绝。
- **证据/状态**：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞

### ADOPT-02 adoption apply、事务日志和验证

- **操作**：只在 disposable fixture 上执行 `adopt <id> --apply`，然后读取 `adoptions`、`adoption-inspect <tx-id>`、`list`、`events`。
- **通过标准**：写入前有明确事务；写入后执行验证；记录原始快照、目标快照、状态和 tx ID；Taskrail 只能领养声明支持的 user-level source；应用自有定义、任意 systemd/system service 或未知源不能被领养。
- **证据/状态**：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞

### ADOPT-03 验证失败和 rollback

- **操作**：让 adoption fixture 在写入后验证失败，或在事务中注入失败；执行 `rollback <tx-id>`。
- **通过标准**：失败关闭；原生源恢复到 adoption 前快照；Registry 不留下“已成功领养”的假状态；journal 保留失败原因；rollback 重复执行不会进一步破坏状态。
- **证据/状态**：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞

## 6. daemon、Unix socket、浏览器 dashboard 和 TUI

### RUNTIME-01 daemon 启动、安装、status 和 XDG 路径

- **操作**：在临时 `XDG_DATA_HOME`、`XDG_RUNTIME_DIR`、`XDG_CONFIG_HOME` 下运行前台 daemon；在 macOS/Linux 测试机分别执行 `daemon --install`、`status`、`daemon --uninstall`（安装测试使用专门用户或可回滚环境）。
- **通过标准**：Registry、socket、LaunchAgent/systemd user unit 位于文档声明的路径；Linux 无 user manager 时安装 fail-closed；headless Linux 在 lingering 后可运行；status 报告最近发现时间、provider 完整性、drift 和 confirmed missing counts。
- **证据/状态**：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞 □ 不适用

### RUNTIME-02 socket 权限、daemon restart 和本地 RPC

- **操作**：连接 daemon socket 执行 ping/status/lifecycle/run/log API；检查 socket 权限；重启 daemon 后重复请求；用其他用户尝试连接。
- **通过标准**：socket 仅当前用户可读写（Unix 模式为 `0600` 或等价保护）；RPC 可完成声明的本地控制面操作；daemon 重启不丢 Registry 或运行历史；其他用户不能读取或调用。
- **证据/状态**：权限、重启前后 JSON：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞

### RUNTIME-03 loopback dashboard、健康检查和端口回退

- **操作**：启动 daemon，访问默认 `127.0.0.1:10100`；预先占用 10100 再启动，验证回退范围和 `taskrail gui` 的发现逻辑；从非 loopback 地址尝试访问。
- **通过标准**：dashboard 只监听 loopback；10100 空闲时可用；被占用时在声明的有限范围内选择下一个 Taskrail 端口；`gui` 打开实际 Taskrail 端点，不打开占用 10100 的其他服务；健康和 API 路由返回正确状态。
- **证据/状态**：实际端口：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞

### RUNTIME-04 dashboard 功能和写入边界

- **操作**：使用浏览器打开 discovery、automations、runs、logs、integrations、inbox、approvals、metrics、audit events；通过 dashboard 创建/暂停/恢复/运行/审批等写操作；测试不同 Origin 和无同源请求。
- **通过标准**：展示数据与 CLI/RPC 一致；写操作通过与 CLI/TUI 相同的本地 RPC/policy boundary；非同源浏览器写请求被拒绝；dashboard 不能通过 HTTP 绕过审批、执行任意 shell 或访问 Registry 文件；MCP/Tunnel 不暴露这个浏览器 HTTP endpoint。
- **证据/状态**：截图/请求记录：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞

### RUNTIME-05 浏览器语言和本地存储

- **操作**：首次以 English、简体中文、日本語、한국어 浏览器语言打开；使用右上角 selector 切换语言；检查刷新、重新启动和 localStorage。
- **通过标准**：首次加载选择受支持语言；核心导航、错误、状态和操作按钮有对应翻译；手动选择只保存在浏览器 localStorage，不写入 Registry、服务器或 MCP 响应；刷新后保持选择。
- **证据/状态**：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞

### RUNTIME-06 TUI

- **操作**：执行 `taskrail tui`，准备 managed、observed、paused、failed run 和 attention fixture；测试终端 resize 和退出。
- **通过标准**：显示 name、ownership、runtime state、next run 和 attention items；不因单个长字段或缺失 provider 崩溃；退出不修改任务；运行、日志、事件、指标仍可由 CLI 查询。
- **证据/状态**：截图/录屏：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞

## 7. typed integrations 和审批边界

### INT-00 集成目录和 doctor

- **操作**：执行 `taskrail integrations`、各集成的 `detect` 和 `doctor`；通过 MCP `taskrail_list_integrations` 检查同一目录。
- **通过标准**：目录至少覆盖 Mole、restic、rclone、GitHub、Homebrew、mas、OSV-Scanner、Gitleaks、Trivy、Topgrade、Shortcuts；每个 descriptor 说明 actions/capabilities/risk；缺少本机可执行文件只报告 missing/unavailable，不伪造成功，不执行写操作。
- **证据/状态**：目录 JSON：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞

### INT-01 Mole

- **操作**：

  ```bash
  taskrail integration mole detect
  taskrail integration mole doctor
  taskrail integration mole version
  taskrail integration mole analyze
  taskrail integration mole status
  taskrail integration mole history --limit 20
  taskrail integration mole clean --dry-run
  ```

- **通过标准**：使用 typed argv 和 bounded parser；结果被规范化为 findings/metrics/changes/artifacts 等安全结构；`clean --dry-run` 不改系统；真实 clean 没有审批时不 spawn，有审批时仍匹配精确 typed plan、可过期且一次性消费；原始敏感输出不持久化。
- **证据/状态**：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞 □ 不适用

### INT-02 restic

- **操作**：执行 `detect`、`doctor`、`snapshots`、`check`；对 `backup`、`forget`、`prune` 只进行 plan/dry-run/未授权拒绝测试。
- **通过标准**：snapshot/check 为只读；backup/forget/prune 计划包含风险和参数；仓库地址和密码只通过环境变量引用，不接受或持久化明文 secret；无审批不连接或写入目标；有审批时一次性、精确 fingerprint、可审计。
- **证据/状态**：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞 □ 不适用

### INT-03 rclone

- **操作**：执行 `detect`、`doctor`、`list-remotes`、`check <source> <destination>`；执行 `sync --dry-run`、未授权 `copy/sync` 和审批 fixture。
- **通过标准**：remote 列表不暴露凭据；source/destination 必须显式；dry-run 只读；copy/sync 未审批被拒绝且不产生网络写入；输出有界并规范化；计划和实际执行匹配。
- **证据/状态**：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞 □ 不适用

### INT-04 GitHub 语义集成

- **操作**：用只读测试仓库执行 `taskrail integration github detect/doctor/issues/pulls/failed-runs/checks`；在无认证、仓库不存在和 API 错误时重复。
- **通过标准**：只调用固定的 GitHub 只读查询；不创建 issue、PR、评论、label、merge 或修改 workflow；结果 bounded、规范化，认证/网络/权限错误可诊断；不会把 GitHub token 写入 Registry、日志或响应。
- **证据/状态**：仓库和 query：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞

### INT-05 Homebrew

- **操作**：执行 `detect`、`doctor`、`outdated`、`bundle-check <file>`；对 `upgrade`、`cleanup` 执行 dry-run、未授权和 fixture 审批测试。
- **通过标准**：健康检查和过期清单只读；bundle 路径显式且错误可诊断；upgrade/cleanup 未审批不执行；不调用 sudo 或任意 shell；结果可进入 run/event/metric 模型。
- **证据/状态**：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞 □ 不适用

### INT-06 mas

- **操作**：执行 `detect`、`doctor`、`list`、`outdated`。
- **通过标准**：只读取 Mac App Store 可用性、已安装和过期列表；不安装、更新或删除应用；macOS 以外明确不适用，不伪造成功。
- **证据/状态**：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞 □ 不适用

### INT-07 OSV-Scanner、Gitleaks、Trivy

- **操作**：对无敏感数据 fixture、含已知测试漏洞 fixture、格式错误输出、可执行文件缺失分别执行 `detect`、`doctor`、`scan`。
- **通过标准**：输出统一为 findings 和 severity/count；Gitleaks 不返回 secret/match 原文，只返回 rule、location、severity、derived fingerprint 等安全字段；Trivy/OSV 可区分网络失败、工具缺失和扫描结果；不修改项目文件；解析失败 fail-closed。
- **证据/状态**：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞

### INT-08 Topgrade

- **操作**：执行 `detect`、`doctor`、`inspect`、`plan`；对 `run` 只做未授权拒绝和模拟执行。
- **通过标准**：inspect/plan 只读；run 属于需要审批的系统更新；无审批不 spawn、不调用 sudo；计划参数精确绑定且审计可追踪。
- **证据/状态**：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞 □ 不适用

### INT-09 Apple Shortcuts

- **操作**：执行 `shortcuts detect/doctor`；先用 fresh native scan 获取 UUID，再请求 run；分别测试缺少 `--confirm`、缺少审批、旧 UUID、重命名/删除 Shortcut 后的 UUID、正确审批和重复执行。
- **通过标准**：没有 `confirm=true` 直接拒绝；只能运行最近一次扫描返回的 UUID；执行前再次扫描；UUID 不存在或已过期时拒绝；审批绑定 exact plan fingerprint、一次性消费并过期；不返回动作主体或原始输出；Automator、Keyboard Maestro、Raycast、Alfred、Hazel 仍为 observe-only。
- **证据/状态**：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞 □ 不适用

### APPROVAL-01 审批生命周期

- **操作**：创建 typed write plan，执行 `approval-request`、`approvals`、`approval-decide --approve/--reject`、`approval-execute`；另测过期 TTL、错误 ID、拒绝后执行和重复执行。
- **通过标准**：审批本地持久化；包含 integration/action/risk/request/plan fingerprint/expiry/status；拒绝和过期不执行；批准只消费一次；执行后不能 replay；所有变化都有审计事件；审批不是 shell 授权。
- **证据/状态**：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞

### APPROVAL-02 计划绑定和 secret-safe persistence

- **操作**：申请审批后修改参数、目标、快捷指令 UUID 或环境变量引用，再尝试执行；向参数、日志和事件注入 `TOKEN`、`API_KEY`、密码样例。
- **通过标准**：任何 plan fingerprint 不一致都拒绝；secret 只允许安全的环境变量引用；Registry 快照、run、event、approval、MCP 响应不包含明文 secret；必要时只显示 `[REDACTED]` 或指纹。
- **证据/状态**：敏感词扫描输出：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞

### APPROVAL-03 typed scheduling

- **操作**：用 `schedule-integration` 或 MCP `taskrail_schedule_integration` 持久化 read-only/dry-run 集成；尝试创建 recurring write action。
- **通过标准**：自动化保存的是 typed integration step，不是任意 shell 字符串；执行时重新 plan/verify，不盲信创建时输出；周期性写操作被拒绝；运行结果进入正常 history/log/event/metric 模型。
- **证据/状态**：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞

## 8. MCP、Fleet 和 ChatGPT 集成

### MCP-01 stdio 初始化和协议基本面

- **操作**：启动 `taskrail mcp`，发送 initialize、initialized、`tools/list`、overview、status、list automations、scan native；检查 stderr 与 stdout。
- **通过标准**：stdout 只有 MCP 协议数据，诊断进入 stderr；initialize 成功；工具名、输入 schema、输出 schema、annotations 和版本信息完整；无效 JSON-RPC、未知 method、缺字段请求返回结构化错误，不使进程崩溃。
- **证据/状态**：协议记录：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞

### MCP-02 私有本地工具面

- **操作**：在本地临时 Registry 上覆盖 status、overview、fresh discovery、integrations、create/list/get automation、pause/resume、run/cancel、runs/logs、attention/events、adoption、drift、delete 和 approval 工具。
- **通过标准**：工具调用最终经过 daemon/RPC 和同一 policy boundary；typed argv、审批、ownership、历史保护仍生效；MCP 不直接打开 SQLite、不绕过 socket、不暴露原始 native definitions、环境值或 home 前缀。
- **证据/状态**：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞

### MCP-03 public read-only profile allowlist

- **操作**：使用 `TASKRAIL_MCP_PROFILE=public taskrail mcp` 或 `mcp-http --profile public-read-only`，核对 `tools/list`；直接构造对 create/delete/run/adopt/approve/cancel 等工具的请求。
- **通过标准**：公开 profile 只宣布并允许 19 个公开只读工具：

  `taskrail_status`、`taskrail_overview`、`taskrail_render_overview`、`taskrail_list_automations`、`taskrail_discover_local_automations`、`taskrail_scan_native`、`taskrail_list_integrations`、`taskrail_list_adoptions`、`taskrail_get_adoption`、`taskrail_github`、`taskrail_mas`、`taskrail_osv_scanner`、`taskrail_gitleaks`、`taskrail_trivy`、`taskrail_get_automation`、`taskrail_list_runs`、`taskrail_get_run_logs`、`taskrail_list_attention`、`taskrail_list_events`。

  写入、删除、领养、审批、取消、执行工具既不出现在 `tools/list`，直接调用也必须拒绝，且拒绝发生在 spawn/网络写入前。
- **证据/状态**：tools/list 和负向调用记录：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞

### MCP-04 MCP Apps 只读资源

- **操作**：先调用 `taskrail_overview`，再调用 `taskrail_render_overview`；Fleet 先调用 `taskrail_fleet_overview`，再调用 Fleet render 工具；检查 resource URI、版本和 widget 的网络请求。
- **通过标准**：resource 版本稳定且只绑定对应的只读 render tool；widget 展示 host、native task、Taskrail automation、run/attention；刷新/扫描按钮只调用 typed MCP 工具，不调用本地浏览器 HTTP API，不获得写权限。
- **证据/状态**：截图和网络记录：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞

### MCP-05 Fleet 配置、凭据和 host routing

- **操作**：复制 `examples/fleet.yaml` 到仓库外；验证示例默认 hosts disabled；配置一个 mock host、一个 disabled host、一个 offline host 和重复/相似 label；token 通过 `token_env` 注入。
- **通过标准**：配置文件只保存 endpoint metadata 和环境变量名，不保存 token；disabled/offline host 在 overview 中明确显示且不被隐式请求；每个操作都要求稳定 `host_id`，不能只靠 label 猜目标；路由错误不会访问另一台主机。
- **证据/状态**：请求路由记录：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞

### MCP-06 Fleet 只读默认和私有写入边界

- **操作**：`allow_writes: false` 时对 discovery、integration、adoption、approval、run 等 read/write 路由分别测试；再在专门 mock/private host 上测试 `allow_writes: true`。
- **通过标准**：默认只读；写请求在发起网络请求前拒绝；显式启用后仍由远端 Taskrail 的 policy、ownership 和 approval 最终约束；不同 host 的 Registry、审批和执行隔离。
- **证据/状态**：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞

### HTTP-01 public MCP HTTP 认证和协议

- **操作**：启动 `taskrail mcp-http --profile public-read-only --bind 127.0.0.1:8787`，测试 `/healthz`、`POST /mcp`、缺 token、错误 token、正确 token、MCP headers、错误 HTTP method 和超时。
- **通过标准**：health 与 MCP 路由行为符合文档；Bearer token 使用安全比较；没有凭据返回未授权；请求/响应内容不含 token；协议版本边界和错误码稳定；stdout/stderr 不泄露 secret。
- **证据/状态**：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞

### HTTP-02 Origin、body size、chunked、metrics 和 profile 隔离

- **操作**：测试允许/不允许 Origin、超大 JSON、chunked request、空 body、`/metrics` 认证与未认证、public/private profile 切换。
- **通过标准**：未允许 Origin 被拒；超大 body 被拒且不进入业务层；chunked 按公开 profile 约束处理；内部 metrics 需要认证且不含请求 body/Authorization；private profile 必须显式开启，不能因环境变量或默认值意外公开写工具。
- **证据/状态**：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞

### CHAT-01 ChatGPT doctor 和 Tunnel runtime

- **操作**：在专门测试主机执行 `taskrail integration chatgpt-doctor --profile <profile>`；如果配置了 Secure MCP Tunnel，运行 `chatgpt-connect` 或等价 runtime check。
- **通过标准**：doctor 能报告二进制、socket、profile、tunnel/control-plane 等检查；runtime 能连接并返回 Taskrail 数据；凭据只来自受保护的进程环境或 secret manager，不写入仓库、profile、Registry、日志或截图。
- **证据/状态**：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞

### CHAT-02 登录 ChatGPT 的交互式只读调用

- **操作**：在目标 ChatGPT Web/Desktop/Mobile 账号连接 MCP app，执行“概览主机、扫描本地任务、列出最近运行”，只使用读操作。
- **通过标准**：ChatGPT 能调用 connected app；返回正确 host label/OS/architecture、native discovery、automations、runs、attention；明确没有修改 host/Registry；工具调用链和最终答案可审计。
- **证据/状态**：截图/调用记录：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞

### CHAT-03 ChatGPT Scheduled future trigger

- **操作**：在目标账号创建一个未来时间的 Scheduled task，提示其调用指定 Taskrail automation；等待实际触发，记录 ChatGPT 调用、Taskrail run、日志和通知。
- **通过标准**：Scheduled 页面实际唤醒 ChatGPT；ChatGPT 实际调用 connected Taskrail app；Taskrail 目标主机实际收到 typed request 并记录 run；失败时能返回日志/attention；不能只凭创建成功或交互式调用推断通过。
- **状态**：□ 待测 □ 通过 □ 失败 □ 阻塞 □ 外部未验证
- **若未执行**：必须写“外部未验证”，原因/账号/补测时间：____________________

## 9. 可选 AI 执行器和 GitHub watcher

### AI-01 Codex doctor、codex-run 和模型目录兼容性

- **操作**：执行 `taskrail integration codex-doctor --cwd <git-repo>`；在只读 sandbox 运行一次 `codex-run`；准备含已知不支持 `audio` modality 的 model catalog，使用自动兼容和 `--model-catalog-json` 两种路径。
- **通过标准**：doctor 正确识别 Codex、Git repo 和权限；codex-run 能成功或给出可诊断的真实失败；兼容 catalog 是短期、权限 `0600` 的副本；全局 Codex 配置不改变；不向输出、Registry 或日志写 credential；sandbox 边界有效。
- **证据/状态**：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞 □ 不适用

### AI-02 Responses-compatible executor

- **操作**：用 fake Responses-compatible HTTP server 覆盖成功、非 2xx、malformed JSON、超时和 streaming/错误响应；测试默认 `store=false`、显式 `--store`、API key env 和 `--json`。
- **通过标准**：成功结果可解析；错误有清晰状态且不崩溃；默认不请求 provider-side storage；API key 不出现在命令输出、错误、Registry 或日志；超时回收请求；真实 API 测试必须使用专用低风险账号并单独标记外部成本。
- **证据/状态**：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞 □ 不适用

### GH-01 GitHub watcher CLI

- **操作**：对只读测试仓库执行：

  ```bash
  taskrail github-watch --repo <owner/repo> --query pulls
  taskrail github-watch --repo <owner/repo> --query issues
  taskrail github-watch --repo <owner/repo> --query failed-runs
  taskrail github-watch --repo <owner/repo> --query checks --pull-number <n>
  ```

  再开启 `--interval-seconds` 轮询，重复返回未变化快照。
- **通过标准**：只使用 read-only GitHub CLI 查询；pulls/issues/checks/failed-runs 结果可规范化；未变化快照去重，不产生无意义的 run/event；认证失败、仓库不存在和 rate limit 可诊断；绝不执行 GitHub 写入。
- **证据/状态**：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞

## 10. 安全、隐私和本地优先行为

### SEC-01 secret 和敏感路径脱敏

- **操作**：在 automation env、integration parameters、run output、event raw、approval request、MCP 请求中放入测试 token、密码、private path、home 前缀和 Authorization header；执行 list/get/run/logs/overview。
- **通过标准**：持久化快照、MCP 响应、事件和日志不含明文 secret；路径按声明隐藏 home 前缀；scanner 不输出匹配内容；approval ID 可显示但 token 不显示；真实密钥不得用于测试。
- **证据/状态**：全文扫描命令与结果：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞

### SEC-02 本地边界和网络最小化

- **操作**：监测 CLI、daemon、MCP、dashboard、Fleet 和 integration 的网络连接；尝试访问 Registry SQLite、跨用户 socket、非 loopback dashboard 和未知 host；检查普通发现是否产生网络写入。
- **通过标准**：Registry 只在本地使用；MCP 通过受限 socket 访问 daemon；dashboard 只 loopback；native discovery 不改变机器；public profile 只访问用户明确选择的只读外部服务；Fleet 只访问明确启用和指定 `host_id` 的 endpoint。
- **证据/状态**：网络/文件审计：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞

### SEC-03 失败关闭和错误恢复

- **操作**：对 Registry 锁冲突、磁盘只读、provider 超时、无效 JSON、缺失二进制、无权限 socket、错误审批、远端 5xx 和 daemon 重启注入故障。
- **通过标准**：不执行未确认的写操作；不删除已有观察项；不产生假成功；进程可重启；错误可诊断且不含敏感值；已有历史和审计记录不被覆盖。
- **证据/状态**：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞

## 11. CI、打包、发布和部署验收

### QA-01 Rust 质量门槛

- **操作**：在与 CI 一致的 toolchain 执行：

  ```bash
  cargo +1.88.0 fmt --all -- --check
  cargo +1.88.0 clippy --locked --workspace --all-targets --all-features -- -D warnings
  cargo +1.88.0 test --locked --workspace --all-features
  cargo +1.88.0 test --locked --workspace --doc
  cargo +1.88.0 build --locked --workspace
  git diff --check
  ```

- **通过标准**：全部成功；失败必须定位到被测 commit，不得用本机缓存或跳过测试掩盖。
- **证据/状态**：CI/local log：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞

### QA-02 Cargo package 和嵌入资源

- **操作**：

  ```bash
  cargo +1.88.0 package --locked --package taskrail
  cargo +1.88.0 package --locked --package taskrail --list | grep -F 'gui/index.html'
  cargo +1.88.0 package --locked --package taskrail --list | grep -F 'gui/app.js'
  cargo +1.88.0 package --locked --package taskrail --list | grep -F 'gui/styles.css'
  cargo +1.88.0 package --locked --package taskrail --list | grep -F 'gui/favicon.svg'
  cargo +1.88.0 package --locked --package taskrail --list | grep -F 'gui/mcp-app.html'
  cargo +1.88.0 package --locked --package taskrail --list | grep -F 'gui/mcp-fleet-app.html'
  ```

- **通过标准**：crate 可在干净环境打包；浏览器 dashboard、本机 MCP Apps 和 Fleet MCP Apps 资源全部进入包；从 package 产物启动时功能不依赖 checkout 路径。
- **证据/状态**：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞

### QA-03 ARM64 CI、MSRV 和安全工作流

- **操作**：检查并运行 `.github/workflows/ci.yml`、`codeql.yml`、`dependency-review.yml`、`security.yml`；核对 ARM64 Linux、ARM64 macOS、MSRV `1.88.0`、dependency review、CodeQL、cargo audit、cargo deny。
- **通过标准**：工作流 YAML 可解析；产品 build/test 使用 ARM64 runner；MSRV、package、audit、deny、CodeQL 和依赖审查按触发器执行；required contexts 与分支保护一致；没有把 x86_64/Windows 当成官方 release target。
- **证据/状态**：workflow run 链接：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞

### QA-04 Release 资产和版本一致性

- **操作**：对候选 tag 执行 release workflow；核对 package version/tag、Linux ARM64 与 macOS ARM64 archive、SHA-256、SPDX SBOM、签名/attestation、GitHub Release 上传和安装后 `--version`。
- **通过标准**：tag `vX.Y.Z` 与 crate version 一致；两个 ARM64 资产均能下载、校验和解包运行；checksum 校验成功；SBOM 与资产匹配；attestation 可在 GitHub/Sigstore 侧验证；release 不发布未经声明的 x86_64 或 Windows 官方资产。
- **证据/状态**：Release URL：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞

### QA-05 Docker Compose / 单主机公开部署

- **操作**：在 ARM64 Docker 主机执行：

  ```bash
  export TASKRAIL_MCP_BEARER_TOKEN="$(openssl rand -hex 32)"
  docker compose -f deploy/docker-compose.public.yml up -d --build
  docker compose -f deploy/docker-compose.public.yml --profile smoke run --rm taskrail-healthcheck
  ```

  检查 compose 是否使用 `expose` 而非直接 `ports`，再从 TLS/auth edge 访问 `/mcp`。
- **通过标准**：daemon 和 HTTP adapter 分离、非 root、只共享声明的 Registry volume/Unix socket；本机 smoke 通过；8787 不直接公开到 host；生产边界有稳定 HTTPS、OAuth/OIDC 或 MCP 认证、per-user host binding、secret manager、备份、日志和内部 metrics；该 sample 不被误宣称为多租户服务。
- **证据/状态**：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞

### QA-06 OpenAI app submission pack 和公开审核

- **操作**：执行 `python scripts/validate_openai_submission.py`；核对 public profile 工具数、五个正向测试、三个负向测试、logo、privacy/terms/support URL；在真实生产 HTTPS endpoint 上执行所有测试。
- **通过标准**：提交包校验通过；公开工具面与 public read-only allowlist 完全一致；公开 endpoint 不使用 localhost、私有地址或 tunnel-only 地址；用户认证和 per-user host binding 在边缘完成；所有正负向测试都在生产 endpoint 得到预期结果；OpenAI review 通过后还必须单独执行 Publish。
- **状态**：□ 待测 □ 通过 □ 失败 □ 阻塞 □ 外部未验证
- **外部记录**：endpoint / reviewer / submission / publish 状态：____________________

### QA-07 OSS 治理和文档一致性

- **操作**：检查 LICENSE、CONTRIBUTING、SECURITY、CODEOWNERS、issue templates、dependabot、PR template、README 双语链接、隐私/条款/支持页面；逐个打开文档链接。
- **通过标准**：许可证为 Apache-2.0；安全漏洞走私密渠道；bug template 要求版本、平台、复现和期望/实际行为且提醒不要上传 secret；文档不把历史提案或 removed components 当成当前能力；README、中文 README、MCP 指南、提交清单和发布资产描述一致。
- **证据/状态**：链接检查表：____________________；状态：□ 待测 □ 通过 □ 失败 □ 阻塞

## 12. GitHub 描述追踪矩阵

以下矩阵用于防止漏测。团队汇总报告时，应在右侧填入对应测试 ID 的最终状态和证据链接。

| GitHub 公开描述 | 对应验收项 | 最终状态/证据 |
| --- | --- | --- |
| 仅支持 ARM64 macOS/Linux；其他 target fail-closed | CORE-01、QA-03、QA-04 | ____________________ |
| `add/register → list → daemon → run → history/logs → tui` | CORE-02、CORE-03、CORE-07、CORE-08、CORE-09、SCHED-01、RUNTIME-06 | ____________________ |
| interval/cron 调度、misfire、overlap、restart recovery | SCHED-01、SCHED-02、SCHED-03、CORE-09 | ____________________ |
| launchd/cron/systemd/Homebrew 和 macOS app discovery | DISC-01 至 DISC-05 | ____________________ |
| adoption、rollback、删除保护、观察只读 | CORE-06、ADOPT-01 至 ADOPT-03 | ____________________ |
| dashboard、TUI、loopback、same-origin 和端口回退 | RUNTIME-01 至 RUNTIME-06 | ____________________ |
| typed semantic integrations | INT-00 至 INT-09 | ____________________ |
| approvals、typed scheduling、secret-safe persistence | APPROVAL-01 至 APPROVAL-03、SEC-01 | ____________________ |
| MCP private/public、MCP Apps、Fleet、HTTP | MCP-01 至 MCP-06、HTTP-01、HTTP-02 | ____________________ |
| ChatGPT Tunnel、交互式 app call、Scheduled | CHAT-01 至 CHAT-03 | ____________________ |
| Codex、Responses、GitHub watcher | AI-01、AI-02、GH-01 | ____________________ |
| ARM64 CI、package、release、Docker、OpenAI submission | QA-01 至 QA-07 | ____________________ |

## 13. 汇总和发布决策

### 13.1 统计

| 类别 | 总数 | 通过 | 失败 | 阻塞 | 外部未验证 | 不适用 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| P0 | ____ | ____ | ____ | ____ | ____ | ____ |
| P1 | ____ | ____ | ____ | ____ | ____ | ____ |
| P2/外部 | ____ | ____ | ____ | ____ | ____ | ____ |
| 合计 | ____ | ____ | ____ | ____ | ____ | ____ |

### 13.2 必须单独说明的外部闸门

- [ ] 未来 ChatGPT Scheduled trigger 已在目标账号真实触发并完成 Taskrail run。
- [ ] 稳定生产 HTTPS MCP endpoint 已部署，具备用户认证和 per-user host binding。
- [ ] OpenAI 应用审核已提交/通过；审核通过后 Publish 动作也已完成（如果目标是公开发布）。
- [ ] Docker Compose 在部署主机上通过 smoke；本地 Rust suite 不能替代该验证。
- [ ] 真实 destructive integration write 和 native adoption 已在专门隔离环境中按授权完成，或明确写为未执行。

### 13.3 最终结论

- [ ] **通过发布**：所有适用 P0 通过，无未接受的 P0 缺陷；P1 风险已关闭或有正式豁免；外部闸门已单独披露。
- [ ] **有条件通过**：核心和安全 P0 通过，但存在已批准的 P1/P2 或外部未验证项；发布说明明确限制、责任人和补测日期。
- [ ] **不通过**：存在任一 P0 失败、数据丢失、未授权执行/写入、secret 泄露、错误 host 路由或无法回滚的原生修改。

**验收负责人签字：** ____________________
**研发负责人签字：** ____________________
**运维/安全负责人签字：** ____________________
**产品决策日期：** ____________________
