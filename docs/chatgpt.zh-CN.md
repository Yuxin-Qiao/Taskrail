# ChatGPT 集成

[English](chatgpt.md)

Taskrail 的 ChatGPT 集成是一个提供类型化工具和可选只读 MCP Apps Widget 的 MCP 应用。ChatGPT 提供自然语言对话、
“Scheduled”页面和通知；Taskrail 提供本地守护进程、调度器、命令执行、运行历史、
日志和主机本地审计事件。

如果希望 ChatGPT 和 Fleet 响应中显示稳定、易识别的主机名称，可以设置
`TASKRAIL_HOST_LABEL`。未设置时，Taskrail 会回退到操作系统 hostname；只有在
系统也无法提供 hostname 时才显示 `unnamed-host`。

该连接可以按主机隔离，也可以通过显式的 fleet 网关把多台 Taskrail 主机呈现为一个 MCP
应用。每台 ARM64 macOS 或 Linux 机器仍然拥有自己的 Registry、策略、审批和执行边界。
单主机连接时设置稳定标签，让 ChatGPT 能区分不同主机：

~~~
export TASKRAIL_HOST_LABEL="macbook-pro"
~~~

多主机连接时，复制 `examples/fleet.yaml` 到被 `.gitignore` 忽略的本地路径，替换端点，
通过 `token_env` 注入令牌，然后启动网关：

~~~bash
mkdir -p ~/.config/taskrail
cp examples/fleet.yaml ~/.config/taskrail/fleet.yaml
taskrail mcp-fleet --config ~/.config/taskrail/fleet.yaml
~~~

仓库中的示例主机默认禁用。fleet 工具首先提供 `taskrail_fleet_overview`，可选地提供
`taskrail_fleet_render_overview` MCP Apps 视图，然后提供带明确 `host_id` 的发现、清单、原生集成、
领养、漂移、审计事件、运行历史、日志、审批和生命周期操作。
默认只读；只有可信私有端点才应显式设置 `allow_writes: true`，且远端 Taskrail 仍负责最终策略和审批。

## 启动本地后端

守护进程拥有 SQLite Registry，并在支持的 ARM64 macOS/Linux 主机上监听用户范围的
受限 Unix socket。
启动后默认每五分钟执行一次只读的原生任务发现；可以用
`--discovery-interval-seconds` 调整间隔。它会同步观察任务、记录漂移，并且只有在某个
provider 成功查询时才会把任务标记为消失；不可用的 provider 不会被当作空列表。

在 macOS 上安装 LaunchAgent：

~~~
taskrail daemon --install
taskrail status
~~~

在 Linux 上安装 systemd 用户单元。对于无头主机，先启用 lingering，使用户管理器在退出
登录后仍保持运行：

~~~
loginctl enable-linger "$USER"
taskrail daemon --install
taskrail status
~~~

该单元写入 `~/.config/systemd/user/taskrail.service`（或 `XDG_CONFIG_HOME` 指定的目录）。
Registry 使用 `XDG_DATA_HOME`，socket 在这些变量为绝对路径时使用 `XDG_RUNTIME_DIR`；
否则 Taskrail 回退到 `~/.local/share/taskrail/`。如果 systemd 用户管理器不可用，安装命令
会失败关闭。

如果自行管理 systemd 单元，仍可以显式以前台方式启动：

~~~
taskrail daemon --socket "${XDG_RUNTIME_DIR:-$HOME/.local/share}/taskrail/taskraild.sock"
~~~

MCP 进程有意设计为短生命周期，并通过 stdio 通信。Tunnel 客户端在 ChatGPT 需要调用
工具时启动它：

~~~
taskrail mcp --socket "${XDG_RUNTIME_DIR:-$HOME/.local/share}/taskrail/taskraild.sock"
~~~

不要把 MCP 进程的 stdout 重定向到人类可读的日志；stdout 是 MCP 协议流，诊断信息应写入
stderr。

默认的 `taskrail mcp` 配置是私有开发者连接使用的完整本地配置。它包含写入和执行工具，
但这些工具仍受类型化、直接 argv、策略和审批控制。不要将此配置暴露在公开 HTTP 端点上。

在配置 OpenAI 侧之前，可以检查本地前置条件，且不会打印任何凭据：

~~~
taskrail integration chatgpt-doctor
~~~

该命令报告守护进程/socket、MCP 适配器、Tunnel 客户端、Tunnel ID 和运行时密钥是否存在，
但不能证明 ChatGPT 可以访问 Tunnel；OpenAI Platform 配置完成后，最终检查应使用
`tunnel-client doctor`。

配置好 `CONTROL_PLANE_TUNNEL_ID` 和 `CONTROL_PLANE_API_KEY` 后，Taskrail 可以替你启动
受管理的 Tunnel 运行时：

~~~
taskrail integration chatgpt-connect
tunnel-client runtimes status taskrail-local --json
~~~

连接命令只把 `env:CONTROL_PLANE_API_KEY` 这一引用传给 `tunnel-client`，不会把密钥放入
配置参数、Registry 行、日志或 Git 文件。

在 macOS 上进行无人值守重连时，将值保存在用户 launchd 环境中，而不是 shell 历史或仓库文件：

~~~
launchctl setenv CONTROL_PLANE_API_KEY '<runtime key>'
taskrail integration chatgpt-connect
~~~

Taskrail 只读取该值来启动短生命周期的 Tunnel 子进程，且从不打印它。Linux 用户服务应
使用用户服务管理器提供的环境机制，例如受保护的 systemd EnvironmentFile。

## 使用 Secure MCP Tunnel 的私有连接

Secure MCP Tunnel 保持 MCP 服务器私有，并由主机发起出站连接。在 OpenAI Platform 创建
Tunnel，并将它关联到使用该应用的 ChatGPT 工作区。然后使用最新版本的 tunnel-client
配置 stdio 配置：

~~~
export CONTROL_PLANE_API_KEY="<存放在仓库之外>"

tunnel-client init \
  --sample sample_mcp_stdio_local \
  --profile taskrail-local \
  --tunnel-id "<tunnel_id>" \
  --mcp-command "taskrail mcp --socket $HOME/.local/share/taskrail/taskraild.sock"

tunnel-client doctor --profile taskrail-local --explain
tunnel-client run --profile taskrail-local
~~~

使用应用时请保持 `tunnel-client run` 健康运行。Tunnel 运行时密钥和 Tunnel ID 都是部署
密钥；不要将它们提交到仓库，也不要放入自动化定义。

## 公开审核配置

OpenAI 公开应用审核需要稳定、生产托管的 HTTPS MCP 端点。本地 Secure MCP Tunnel 适合
开发连接，不适合作为公开提交端点。请将内置 HTTP 适配器置于 TLS 终止反向代理之后：

~~~
export TASKRAIL_MCP_BEARER_TOKEN="<从密钥管理器注入>"
taskrail mcp-http \
  --profile public-read-only \
  --bind 127.0.0.1:8787 \
  --socket "${XDG_RUNTIME_DIR:-$HOME/.local/share}/taskrail/taskraild.sock"
~~~

`taskrail mcp-http` 默认启动公开只读配置。它提供 `POST /mcp` 和 `GET /healthz`，要求
Bearer 认证、限制请求体大小并拒绝 chunked 请求。代理/托管层仍必须提供终端用户认证和
按用户的主机绑定。公开配置只提供状态、原生任务发现、清单、领养日志查看、只读 GitHub
观察、本地软件包/安全检查、运行历史/日志、待处理事项和审计事件；不提供自动化创建、
删除、暂停/恢复、执行、取消、原生领养、集成写入或审批操作。

如果私有的单主机 Fleet 目标需要接收明确的写入或运行请求，可显式启用私有配置：

~~~bash
export TASKRAIL_MCP_BEARER_TOKEN="<从密钥管理器注入>"
taskrail mcp-http \
  --profile private \
  --bind 127.0.0.1:8788 \
  --socket "${XDG_RUNTIME_DIR:-$HOME/.local/share}/taskrail/taskraild.sock"
~~~

私有配置绝不会默认启用：必须放在私有 TLS/认证边缘之后，每个端点只绑定一个获授权的主机，
不要把它暴露为共享公开中继。Fleet 中 `allow_writes: true` 的主机必须指向这种显式保护的私有
端点；公开只读端点拒绝 Fleet 写操作是预期行为。

公开端点在转发到用户守护进程前，必须自行增加用户认证和主机绑定。不要把只读配置变成
共享的未认证中继，也不要提交 localhost、私有网络或仅 Tunnel 可访问的 URL。其余门户
步骤和完整测试包请参阅 [OpenAI 提交检查清单](OPENAI_SUBMISSION.md)。

## 在 ChatGPT 中连接应用

在 ChatGPT 中：

1. 在“设置 → Security and login”中启用 Developer mode；
2. 打开 Plugins/Apps 开发者连接页面并创建应用；
3. 选择 Tunnel，选中 Taskrail Tunnel，并检查发现的工具；
4. 修改工具描述符或重新构建 Taskrail 后，刷新应用。

创建 Scheduled 任务前，应先完成应用连接。之后可以使用类似下面的提示：

~~~
每周日 09:00，在 MacBook 主机上运行名为“Mole cleanup”的 Taskrail 自动化。
如果运行失败，获取运行日志，并告诉我退出状态和下一步行动。
~~~

多主机环境请为每台机器使用独立 Tunnel/配置和主机标签，并在 Scheduled 任务中明确目标
主机。用户需要完整主机摘要时，始终先调用 taskrail_overview；它会在一个只读结果中
返回主机身份、守护进程状态、最新发现、Taskrail 清单、最近运行和待处理事项。轻量级连通
性检查使用 taskrail_status。当用户询问主机上现有的自动化任务时，优先使用
taskrail_discover_local_automations 执行新的原生扫描；ChatGPT 成功响应不代表另一台主机
的守护进程实际运行了任务。
守护进程状态还包含最近一次后台发现的时间、已完成查询的 provider、漂移数量和已确认
消失的任务数量。

使用 fleet 网关时，先调用 `taskrail_fleet_overview` 查看所有配置主机的在线状态；需要交互式多主机视图时再调用
`taskrail_fleet_render_overview`，然后在每次
主机操作中传入稳定的 `host_id`。不要只根据显示名称猜测目标；fleet 配置是本地文件，令牌
只能通过 `token_env` 引用，远端主机仍负责最终策略、审批和执行。

## 工具范围

适配器提供面向具体操作的工具，而不是通用 shell 端点：

- taskrail_status — 检查守护进程连通性并识别主机；
- taskrail_overview — 返回合并了身份、发现、Taskrail 自动化、最近运行和待处理事项的安全主机摘要；
- taskrail_render_overview — 在 taskrail_overview 之后，将同一份只读摘要渲染为 ChatGPT 内的
  MCP Apps 控制面视图；其中包含已发现的原生任务清单、Taskrail 自动化和待处理事项。刷新和
  原生扫描按钮只调用类型化只读工具，不会暴露本地浏览器 HTTP API；
- taskrail_fleet_render_overview — 在 taskrail_fleet_overview 之后，将配置主机状态渲染为只读的
  多主机 MCP Apps 控制面视图；不会绕过每台主机的路由、策略、审批或执行边界；
- taskrail_list_automations / taskrail_get_automation — 查看本地清单；
- taskrail_discover_local_automations — 新扫描 launchd、cron、systemd 和 Homebrew 服务；
- taskrail_scan_native — 执行只读的 launchd、cron、systemd 或 Homebrew 扫描；
- taskrail_list_integrations — 查看内置集成目录、可执行文件检测和本机 doctor 状态；
- taskrail_schedule_integration — 将类型化只读或 dry-run 集成持久化为本地自动化；拒绝周期性写操作；
- taskrail_list_adoptions / taskrail_get_adoption — 查看原生领养日志状态；
- taskrail_adopt_automation / taskrail_rollback_adoption — 预检/应用或恢复原生调度器领养事务；
- taskrail_acknowledge_drift — 接受新的外部基线，同时保持所属自动化暂停；
- taskrail_create_automation — 创建直接 argv 的手动、interval 或 cron 任务；
- taskrail_delete_automation — 仅删除没有运行历史的托管自动化；观察到的/已领养定义受保护；
- taskrail_pause_automation / taskrail_resume_automation — 更改托管运行状态；
- taskrail_run_automation / taskrail_cancel_run — 显式开始或停止运行；
- taskrail_list_runs / taskrail_get_run_logs — 查看运行结果；
- taskrail_list_attention / taskrail_list_events — 查看失败、漂移和近期活动；
- taskrail_mole — 使用类型化 Mole 操作进行检测、分析、状态、历史和清理 dry-run；真实清理具有破坏性，须先获得 Taskrail 策略要求的显式、会过期的审批；
- taskrail_restic / taskrail_rclone — 使用类型化快照、仓库、传输和同步操作；备份、复制和真实同步受策略控制；
- taskrail_github / taskrail_homebrew — 使用固定的只读 GitHub 观察和类型化 Homebrew 健康/维护操作；
- taskrail_mas、taskrail_osv_scanner、taskrail_gitleaks 和 taskrail_trivy — 检查本地软件包和安全发现，不暴露密钥或匹配内容；
- taskrail_topgrade — 检查或规划更新；执行需要审批；
- taskrail_list_approvals、taskrail_request_approval、taskrail_approve、taskrail_reject 和 taskrail_execute_approved — 查看和操作持久化、绑定计划的审批流；审批只能使用一次，且不是 shell 授权。

Fleet 网关提供对应的 `taskrail_fleet_` 主机定向操作，包括原生领养、漂移确认、类型化集成
调度和审批生命周期。每个 Fleet 操作都必须明确提供 `host_id`；除非该主机显式配置
`allow_writes: true`，否则写入操作会在发起网络请求前被拒绝。

适配器不接受任意 shell 字符串、不暴露 SQLite 文件，也不会修改被观察的原生任务。原生
领养仍然是显式的本地操作。

公开审核时，只有只读子集会通过 TASKRAIL_MCP_PROFILE=public 宣布并强制执行。上面的
完整工具范围用于私有、用户拥有的连接；分离两种范围可以避免公开端点变成通用的本地命令
执行器。

## 该集成不代表什么

ChatGPT 的 Scheduled 页面是 ChatGPT 提示词的调度器，Taskrail 守护进程是本地
Automation 的调度器。因此，在 09:00 调用 Taskrail 的 Scheduled 任务是一个两阶段工作流：
ChatGPT 被唤醒并调用本地应用；Taskrail 再根据持久化的类型化定义运行选定的本地 Automation，
并记录结果。已连接的应用不会自动导入或控制 ChatGPT 自己的 Scheduled 任务列表；该列表
仍由 ChatGPT 的 Scheduled 页面管理。

如需无人值守运行，请保持 Taskrail 守护进程和 Tunnel 客户端运行，并通过返回的运行状态
和日志确认失败。
