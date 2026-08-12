# Taskrail 文档

Taskrail 是面向开发者的本地自动化管理器。产品核心路径很小：

~~~
add/register → schedule → run → history/logs → 浏览器控制台
~~~

返回[中文 README](../README.zh-CN.md)或阅读 [English README](../README.md)。

## 当前产品

- Rust crate 和 CLI 的名称都是 `taskrail`；
- 本地 Registry 存储自动化、运行、日志、事件和指标；
- 守护进程负责计算 interval 和 cron 触发器；
- 守护进程默认在 `http://127.0.0.1:10100` 上提供主要的 loopback 浏览器控制台；`taskrail gui`
  可以打开它，`taskrail tui` 是终端备用入口；
- 如果 `10100` 已被占用，守护进程会在有限的 loopback 端口范围内尝试到 `10110`，`taskrail gui` 会发现真正的
  Taskrail 地址，不会误打开其他本地服务；
- 浏览器控制台只是守护进程本地 RPC handler 的薄客户端，不是公开服务，也不会通过 Tunnel 暴露；
- 浏览器控制台支持 English、简体中文、日本語和한국어，首次打开会自动识别浏览器语言，手动切换只保存
  在浏览器本地存储中；
- ChatGPT MCP 应用可以通过带版本的 MCP Apps resource 在对话内渲染只读主机摘要和多主机 Fleet 总览；Widget 只调用
  类型化 MCP 工具，不会访问本地浏览器 HTTP 接口；
- Rust CLI、守护进程、TUI、浏览器控制台和本地 MCP 适配器只支持 ARM64 macOS 和 ARM64 Linux；
  控制平面使用受限 Unix socket，浏览器控制台只使用 loopback HTTP；
- 已连接的 ChatGPT 客户端可以通过 OpenAI Secure MCP Tunnel 调用本地 MCP 适配器；
  未来 Scheduled 触发的实测属于独立的外部验证门槛，公开应用审核和托管部署同样如此；
- `taskrail mcp-fleet` 可以把明确配置的多台主机聚合为一个 MCP 应用；端点和令牌环境变量名
  保留在本机，默认只读，写入路由必须显式启用；私有主机定向能力包括只读 Fleet 控制面、原生领养、
  漂移确认、类型化集成和持久化审批；
- 公开只读 `taskrail mcp-http` 适配器可部署在 TLS/认证边缘之后；`deploy/` 下的容器示例
  仅支持单主机，不是托管的多租户服务；
- 原生调度器发现和类型化语义集成位于核心管理器边缘；
- 命令使用直接 argv，拒绝 shell 字符串；
- 语义集成覆盖 Mole、restic、rclone、GitHub、Homebrew、mas、OSV-Scanner、Gitleaks、
  Trivy 和 Topgrade；支持写入的操作必须经过持久化审批并在策略边界内失败关闭。

## 文档

- [安全策略](../SECURITY.md) — 权威安全边界。
- [贡献指南](../CONTRIBUTING.md) — 开发工作流。
- [架构决策](adr/) — 仍描述当前实现的专题决策。
- [研究报告](../deep-research-report.md) — 历史产品和架构研究，其中包含已从当前 MVP 移除的提案。
- [中文 ChatGPT 集成指南](chatgpt.zh-CN.md) — 将 ChatGPT Scheduled 任务连接到 ARM64 macOS 或 Linux 主机上的 Taskrail。
- [ChatGPT integration](chatgpt.md) — English version。
- [Fleet 示例](../examples/fleet.yaml) — 多主机端点元数据模板；启用主机前请复制到仓库之外。
- [OpenAI 提交检查清单](OPENAI_SUBMISSION.md) — 公开审核配置、元数据、测试用例、策略页面和外部发布门槛。
- [OpenAI release notes](OPENAI_RELEASE_NOTES.md) — 可直接用于初始提交的英文摘要。
- [隐私政策](PRIVACY.md)、[服务条款](TERMS.md)和[支持页面](SUPPORT.md) — 公开应用审核所需的策略页面。
- [验收检查清单](ACCEPTANCE.md) — 可复现的发布门槛命令和证据要求。
- [原生集成架构](adr/0031-native-integration-semantic-layer.md) — 共享计划、策略、解析和验证边界。

已移除的 Codex App Server、特权辅助进程和通用远程策略引擎文档不属于当前产品契约。
当前支持的 ChatGPT 集成边界是 `chatgpt.zh-CN.md` 中描述的 MCP 适配器和本地审批记录。
