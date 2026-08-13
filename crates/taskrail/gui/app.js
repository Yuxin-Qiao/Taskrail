const navItems = [
  ["dashboard", "⌂", "nav.dashboard"],
  ["automations", "◆", "nav.automations"],
  ["discovery", "⌁", "nav.discovery"],
  ["runs", "↻", "nav.runs"],
  ["inbox", "!", "nav.inbox"],
  ["integrations", "◇", "nav.integrations"],
  ["approvals", "✓", "nav.approvals"],
  ["metrics", "∿", "nav.metrics"],
  ["events", "≡", "nav.events"],
];

const localeChoices = [
  ["en", "English"],
  ["zh-CN", "简体中文"],
  ["ja", "日本語"],
  ["ko", "한국어"],
];

const translations = {
  en: {
    "app.title": "Taskrail Dashboard",
    language: "Language",
    "brand.web": "web",
    "nav.dashboard": "Dashboard",
    "nav.automations": "Automations",
    "nav.discovery": "Discovery",
    "nav.runs": "Runs",
    "nav.inbox": "Inbox",
    "nav.integrations": "Integrations",
    "nav.approvals": "Approvals",
    "nav.metrics": "Metrics",
    "nav.events": "Events",
    "sidebar.local": "Local control plane",
    "sidebar.unavailable": "daemon unavailable",
    "sidebar.healthz": "healthz ↗",
    "page.dashboard.title": "Local Automation Manager",
    "page.dashboard.subtitle": "Daemon-hosted dashboard · local HTTP management API",
    "status.connected": "Connected · {platform} {architecture}",
    "status.unavailable": "Daemon unavailable · start taskrail daemon",
    "common.refresh": "Refresh",
    "common.localHost": "local host",
    "common.scanNote": "A scan never changes the native definition.",
    "common.error": "Error",
    "card.automations": "Automations",
    "card.recentRuns": "Recent runs",
    "card.needsAttention": "Needs attention",
    "card.pendingApprovals": "Pending approvals",
    "detail.managedObserved": "{managed} managed · {observed} observed",
    "detail.succeeded": "{count} succeeded in current window",
    "detail.failuresVisible": "Failures and drift stay visible",
    "detail.typedPlans": "Typed plans only · no shell access",
    "count.items": "{count} item(s)",
    "count.nativeSources": "{count} native source(s)",
    "page.automations.title": "Automations",
    "page.automations.subtitle": "Managed commands and observed native jobs.",
    "page.discovery.title": "Native discovery",
    "page.discovery.subtitle": "Read-only inventory of schedulers and supported macOS application sources.",
    "page.runs.title": "Runs",
    "page.runs.subtitle": "Immutable run records and bounded stdout/stderr logs.",
    "page.inbox.title": "Inbox",
    "page.inbox.subtitle": "Failures, drift, missing sources, and recovery items.",
    "page.integrations.title": "Integrations",
    "page.integrations.subtitle": "Typed semantic adapters detected on this host.",
    "page.approvals.title": "Approvals",
    "page.approvals.subtitle": "Plan-bound, expiring, one-time requests for native writes.",
    "page.metrics.title": "Metrics",
    "page.metrics.subtitle": "Recorded provider and operational measurements.",
    "page.events.title": "Events",
    "page.events.subtitle": "Audit history for runs, adoption, discovery, and approvals.",
    "section.registry": "Registry",
    "section.automationOverview": "Automation overview",
    "section.needsAttention": "Needs attention",
    "table.name": "Name",
    "table.ownership": "Ownership",
    "table.state": "State",
    "table.nextRun": "Next run",
    "table.severity": "Severity",
    "table.kind": "Kind",
    "table.title": "Title",
    "table.status": "Status",
    "table.automation": "Automation",
    "table.started": "Started",
    "table.exit": "Exit",
    "table.nativeId": "Native ID",
    "table.provider": "Provider",
    "table.path": "Path",
    "table.integration": "Integration",
    "table.detection": "Detection",
    "table.doctor": "Doctor",
    "table.capabilities": "Capabilities",
    "table.action": "Action",
    "table.risk": "Risk",
    "table.expires": "Expires",
    "table.key": "Key",
    "table.value": "Value",
    "table.source": "Source",
    "table.recorded": "Recorded",
    "table.seq": "Seq",
    "table.type": "Type",
    "table.occurred": "Occurred",
    "table.payloadKeys": "Payload keys",
    "button.run": "Run",
    "button.pause": "Pause",
    "button.resume": "Resume",
    "button.logs": "Logs",
    "button.cancel": "Cancel",
    "button.scan": "Scan now",
    "button.approve": "Approve",
    "button.reject": "Reject",
    "empty.automations": "No automations registered.",
    "empty.inbox": "Inbox is clear.",
    "empty.runs": "No runs recorded.",
    "empty.discovery": "No native scan has run yet.",
    "empty.integrations": "No integrations registered.",
    "empty.integrationStatus": "Integration status unavailable.",
    "empty.approvals": "Approval queue is clear.",
    "empty.metrics": "No metrics recorded.",
    "empty.events": "No events recorded.",
    "log.stdout": "STDOUT",
    "log.stderr": "STDERR",
    "log.empty": "(empty)",
    "value.observed": "observed",
    "value.managed": "managed",
    "value.enabled": "enabled",
    "value.paused": "paused",
    "value.needs_attention": "needs attention",
    "value.succeeded": "succeeded",
    "value.running": "running",
    "value.failed": "failed",
    "value.cancelled": "cancelled",
    "value.pending": "pending",
    "value.approved": "approved",
    "value.rejected": "rejected",
    "value.available": "available",
    "value.missing": "missing",
    "value.ready": "ready",
    "value.unavailable": "unavailable",
    "value.unknown": "unknown",
    "value.manual": "manual",
    "value.read": "read",
    "value.destructive": "destructive",
    "value.system_write": "system write",
    "value.network_write": "network write",
    "integrationAction.doctor": "doctor",
    "integrationAction.issues": "issues",
    "integrationAction.pulls": "pulls",
    "integrationAction.failed-runs": "failed runs",
    "integrationAction.checks": "checks",
    "integrationAction.scan": "scan",
    "integrationAction.outdated": "outdated",
    "integrationAction.bundle-check": "bundle check",
    "integrationAction.upgrade": "upgrade",
    "integrationAction.cleanup": "cleanup",
    "integrationAction.detect": "detect",
    "integrationAction.version": "version",
    "integrationAction.analyze": "analyze",
    "integrationAction.status": "status",
    "integrationAction.history": "history",
    "integrationAction.clean": "clean",
    "integrationAction.list-remotes": "list remotes",
    "integrationAction.snapshots": "snapshots",
    "integrationAction.backup": "backup",
    "integrationAction.check": "check",
    "integrationAction.forget": "forget",
    "integrationAction.prune": "prune",
    "integrationAction.inspect": "inspect",
    "integrationAction.plan": "plan",
    "integrationAction.run": "run",
  },
  "zh-CN": {
    "app.title": "Taskrail 控制台",
    language: "语言",
    "brand.web": "网页",
    "nav.dashboard": "概览",
    "nav.automations": "自动化",
    "nav.discovery": "发现",
    "nav.runs": "运行记录",
    "nav.inbox": "待处理",
    "nav.integrations": "集成",
    "nav.approvals": "审批",
    "nav.metrics": "指标",
    "nav.events": "事件",
    "sidebar.local": "本地控制平面",
    "sidebar.unavailable": "守护进程不可用",
    "sidebar.healthz": "健康检查 ↗",
    "page.dashboard.title": "本地自动化管理器",
    "page.dashboard.subtitle": "守护进程托管的控制台 · 本地 HTTP 管理 API",
    "status.connected": "已连接 · {platform} {architecture}",
    "status.unavailable": "守护进程不可用 · 请启动 taskrail daemon",
    "common.refresh": "刷新",
    "common.localHost": "本地主机",
    "common.scanNote": "扫描不会修改原生任务定义。",
    "common.error": "错误",
    "card.automations": "自动化",
    "card.recentRuns": "最近运行",
    "card.needsAttention": "需要处理",
    "card.pendingApprovals": "待审批",
    "detail.managedObserved": "{managed} 个托管 · {observed} 个观察",
    "detail.succeeded": "当前窗口成功 {count} 次",
    "detail.failuresVisible": "失败和漂移会持续显示",
    "detail.typedPlans": "仅支持类型化计划 · 无 shell 访问",
    "count.items": "{count} 项",
    "count.nativeSources": "{count} 个原生来源",
    "page.automations.title": "自动化",
    "page.automations.subtitle": "托管命令与观察到的原生任务。",
    "page.discovery.title": "原生发现",
    "page.discovery.subtitle": "只读查看调度器和受支持的 macOS 应用自动化来源。",
    "page.runs.title": "运行记录",
    "page.runs.subtitle": "不可变的运行记录，以及有界的 stdout/stderr 日志。",
    "page.inbox.title": "待处理",
    "page.inbox.subtitle": "失败、漂移、缺失来源和恢复事项。",
    "page.integrations.title": "集成",
    "page.integrations.subtitle": "在本机检测到的类型化语义适配器。",
    "page.approvals.title": "审批",
    "page.approvals.subtitle": "绑定计划、会过期且只能使用一次的原生写入请求。",
    "page.metrics.title": "指标",
    "page.metrics.subtitle": "已记录的 provider 和运行指标。",
    "page.events.title": "事件",
    "page.events.subtitle": "运行、领养、发现和审批的审计历史。",
    "section.registry": "注册表",
    "section.automationOverview": "自动化概览",
    "section.needsAttention": "需要处理",
    "table.name": "名称",
    "table.ownership": "归属",
    "table.state": "状态",
    "table.nextRun": "下次运行",
    "table.severity": "严重程度",
    "table.kind": "类型",
    "table.title": "标题",
    "table.status": "状态",
    "table.automation": "自动化",
    "table.started": "开始时间",
    "table.exit": "退出码",
    "table.nativeId": "原生 ID",
    "table.provider": "Provider",
    "table.path": "路径",
    "table.integration": "集成",
    "table.detection": "检测",
    "table.doctor": "诊断",
    "table.capabilities": "能力",
    "table.action": "操作",
    "table.risk": "风险",
    "table.expires": "过期时间",
    "table.key": "键",
    "table.value": "值",
    "table.source": "来源",
    "table.recorded": "记录时间",
    "table.seq": "序号",
    "table.type": "类型",
    "table.occurred": "发生时间",
    "table.payloadKeys": "载荷键",
    "button.run": "运行",
    "button.pause": "暂停",
    "button.resume": "恢复",
    "button.logs": "日志",
    "button.cancel": "取消",
    "button.scan": "立即扫描",
    "button.approve": "批准",
    "button.reject": "拒绝",
    "empty.automations": "尚未注册自动化。",
    "empty.inbox": "待处理列表为空。",
    "empty.runs": "暂无运行记录。",
    "empty.discovery": "尚未执行原生扫描。",
    "empty.integrations": "尚未注册集成。",
    "empty.integrationStatus": "集成状态不可用。",
    "empty.approvals": "审批队列为空。",
    "empty.metrics": "暂无指标记录。",
    "empty.events": "暂无事件记录。",
    "log.stdout": "标准输出",
    "log.stderr": "标准错误",
    "log.empty": "（空）",
    "value.observed": "观察",
    "value.managed": "托管",
    "value.enabled": "启用",
    "value.paused": "已暂停",
    "value.needs_attention": "需要处理",
    "value.succeeded": "成功",
    "value.running": "运行中",
    "value.failed": "失败",
    "value.cancelled": "已取消",
    "value.pending": "待处理",
    "value.approved": "已批准",
    "value.rejected": "已拒绝",
    "value.available": "可用",
    "value.missing": "缺失",
    "value.ready": "就绪",
    "value.unavailable": "不可用",
    "value.unknown": "未知",
    "value.manual": "手动",
    "value.read": "读取",
    "value.destructive": "破坏性",
    "value.system_write": "系统写入",
    "value.network_write": "网络写入",
    "integrationAction.doctor": "诊断",
    "integrationAction.issues": "问题",
    "integrationAction.pulls": "拉取请求",
    "integrationAction.failed-runs": "失败运行",
    "integrationAction.checks": "检查",
    "integrationAction.scan": "扫描",
    "integrationAction.outdated": "过期项",
    "integrationAction.bundle-check": "包检查",
    "integrationAction.upgrade": "升级",
    "integrationAction.cleanup": "清理",
    "integrationAction.detect": "检测",
    "integrationAction.version": "版本",
    "integrationAction.analyze": "分析",
    "integrationAction.status": "状态",
    "integrationAction.history": "历史",
    "integrationAction.clean": "清理",
    "integrationAction.list-remotes": "远端列表",
    "integrationAction.snapshots": "快照",
    "integrationAction.backup": "备份",
    "integrationAction.check": "检查",
    "integrationAction.forget": "忘记",
    "integrationAction.prune": "清理无用数据",
    "integrationAction.inspect": "检查详情",
    "integrationAction.plan": "计划",
    "integrationAction.run": "运行",
  },
  ja: {
    "app.title": "Taskrail ダッシュボード",
    language: "言語",
    "brand.web": "ウェブ",
    "nav.dashboard": "ダッシュボード",
    "nav.automations": "自動化",
    "nav.discovery": "検出",
    "nav.runs": "実行履歴",
    "nav.inbox": "受信トレイ",
    "nav.integrations": "連携",
    "nav.approvals": "承認",
    "nav.metrics": "メトリクス",
    "nav.events": "イベント",
    "sidebar.local": "ローカル制御プレーン",
    "sidebar.unavailable": "デーモン未接続",
    "sidebar.healthz": "ヘルスチェック ↗",
    "page.dashboard.title": "ローカル自動化マネージャー",
    "page.dashboard.subtitle": "デーモン提供のダッシュボード · ローカル HTTP 管理 API",
    "status.connected": "接続済み · {platform} {architecture}",
    "status.unavailable": "デーモンを利用できません · taskrail daemon を起動してください",
    "common.refresh": "更新",
    "common.localHost": "ローカルホスト",
    "common.scanNote": "スキャンでネイティブ定義が変更されることはありません。",
    "common.error": "エラー",
    "card.automations": "自動化",
    "card.recentRuns": "最近の実行",
    "card.needsAttention": "要対応",
    "card.pendingApprovals": "保留中の承認",
    "detail.managedObserved": "管理 {managed} · 監視 {observed}",
    "detail.succeeded": "現在の期間で成功 {count} 件",
    "detail.failuresVisible": "失敗とドリフトを表示",
    "detail.typedPlans": "型付きプランのみ · シェルアクセスなし",
    "count.items": "{count} 件",
    "count.nativeSources": "ネイティブソース {count} 件",
    "page.automations.title": "自動化",
    "page.automations.subtitle": "管理対象コマンドと検出されたネイティブジョブ。",
    "page.discovery.title": "ネイティブ検出",
    "page.discovery.subtitle": "スケジューラと対応する macOS 自動化ソースの読み取り専用一覧。",
    "page.runs.title": "実行履歴",
    "page.runs.subtitle": "変更できない実行記録と、制限付き stdout/stderr ログ。",
    "page.inbox.title": "受信トレイ",
    "page.inbox.subtitle": "失敗、ドリフト、不足ソース、復旧項目。",
    "page.integrations.title": "連携",
    "page.integrations.subtitle": "このホストで検出された型付きセマンティックアダプター。",
    "page.approvals.title": "承認",
    "page.approvals.subtitle": "プランに紐づき、期限付きで一度だけ使えるネイティブ書き込み要求。",
    "page.metrics.title": "メトリクス",
    "page.metrics.subtitle": "記録された provider と運用メトリクス。",
    "page.events.title": "イベント",
    "page.events.subtitle": "実行、採用、検出、承認の監査履歴。",
    "section.registry": "レジストリ",
    "section.automationOverview": "自動化の概要",
    "section.needsAttention": "要対応",
    "table.name": "名前",
    "table.ownership": "所有",
    "table.state": "状態",
    "table.nextRun": "次回実行",
    "table.severity": "重大度",
    "table.kind": "種類",
    "table.title": "タイトル",
    "table.status": "ステータス",
    "table.automation": "自動化",
    "table.started": "開始",
    "table.exit": "終了コード",
    "table.nativeId": "ネイティブ ID",
    "table.provider": "Provider",
    "table.path": "パス",
    "table.integration": "連携",
    "table.detection": "検出",
    "table.doctor": "診断",
    "table.capabilities": "機能",
    "table.action": "操作",
    "table.risk": "リスク",
    "table.expires": "有効期限",
    "table.key": "キー",
    "table.value": "値",
    "table.source": "ソース",
    "table.recorded": "記録日時",
    "table.seq": "番号",
    "table.type": "タイプ",
    "table.occurred": "発生日時",
    "table.payloadKeys": "ペイロードキー",
    "button.run": "実行",
    "button.pause": "一時停止",
    "button.resume": "再開",
    "button.logs": "ログ",
    "button.cancel": "キャンセル",
    "button.scan": "今すぐスキャン",
    "button.approve": "承認",
    "button.reject": "拒否",
    "empty.automations": "登録された自動化はありません。",
    "empty.inbox": "対応が必要な項目はありません。",
    "empty.runs": "実行記録はありません。",
    "empty.discovery": "ネイティブスキャンはまだ実行されていません。",
    "empty.integrations": "登録された連携はありません。",
    "empty.integrationStatus": "連携ステータスを取得できません。",
    "empty.approvals": "承認キューは空です。",
    "empty.metrics": "メトリクスはありません。",
    "empty.events": "イベントはありません。",
    "log.stdout": "標準出力",
    "log.stderr": "標準エラー",
    "log.empty": "（空）",
    "value.observed": "監視",
    "value.managed": "管理対象",
    "value.enabled": "有効",
    "value.paused": "一時停止",
    "value.needs_attention": "要対応",
    "value.succeeded": "成功",
    "value.running": "実行中",
    "value.failed": "失敗",
    "value.cancelled": "キャンセル済み",
    "value.pending": "保留中",
    "value.approved": "承認済み",
    "value.rejected": "拒否済み",
    "value.available": "利用可能",
    "value.missing": "不足",
    "value.ready": "準備完了",
    "value.unavailable": "利用不可",
    "value.unknown": "不明",
    "value.manual": "手動",
    "value.read": "読み取り",
    "value.destructive": "破壊的",
    "value.system_write": "システム書き込み",
    "value.network_write": "ネットワーク書き込み",
    "integrationAction.doctor": "診断",
    "integrationAction.issues": "問題",
    "integrationAction.pulls": "プルリクエスト",
    "integrationAction.failed-runs": "失敗実行",
    "integrationAction.checks": "チェック",
    "integrationAction.scan": "スキャン",
    "integrationAction.outdated": "古い項目",
    "integrationAction.bundle-check": "バンドルチェック",
    "integrationAction.upgrade": "アップグレード",
    "integrationAction.cleanup": "クリーンアップ",
    "integrationAction.detect": "検出",
    "integrationAction.version": "バージョン",
    "integrationAction.analyze": "分析",
    "integrationAction.status": "ステータス",
    "integrationAction.history": "履歴",
    "integrationAction.clean": "クリーン",
    "integrationAction.list-remotes": "リモート一覧",
    "integrationAction.snapshots": "スナップショット",
    "integrationAction.backup": "バックアップ",
    "integrationAction.check": "チェック",
    "integrationAction.forget": "忘却",
    "integrationAction.prune": "整理",
    "integrationAction.inspect": "検査",
    "integrationAction.plan": "計画",
    "integrationAction.run": "実行",
  },
  ko: {
    "app.title": "Taskrail 대시보드",
    language: "언어",
    "brand.web": "웹",
    "nav.dashboard": "대시보드",
    "nav.automations": "자동화",
    "nav.discovery": "탐색",
    "nav.runs": "실행 기록",
    "nav.inbox": "받은 편지함",
    "nav.integrations": "통합",
    "nav.approvals": "승인",
    "nav.metrics": "메트릭",
    "nav.events": "이벤트",
    "sidebar.local": "로컬 제어 플레인",
    "sidebar.unavailable": "데몬을 사용할 수 없음",
    "sidebar.healthz": "상태 확인 ↗",
    "page.dashboard.title": "로컬 자동화 관리자",
    "page.dashboard.subtitle": "데몬이 제공하는 대시보드 · 로컬 HTTP 관리 API",
    "status.connected": "연결됨 · {platform} {architecture}",
    "status.unavailable": "데몬을 사용할 수 없음 · taskrail daemon을 시작하세요",
    "common.refresh": "새로 고침",
    "common.localHost": "로컬 호스트",
    "common.scanNote": "탐색은 네이티브 정의를 변경하지 않습니다.",
    "common.error": "오류",
    "card.automations": "자동화",
    "card.recentRuns": "최근 실행",
    "card.needsAttention": "확인 필요",
    "card.pendingApprovals": "대기 중인 승인",
    "detail.managedObserved": "관리 {managed} · 관찰 {observed}",
    "detail.succeeded": "현재 기간 성공 {count}회",
    "detail.failuresVisible": "실패와 드리프트를 계속 표시합니다",
    "detail.typedPlans": "타입이 지정된 계획만 · 셸 접근 없음",
    "count.items": "{count}개",
    "count.nativeSources": "네이티브 소스 {count}개",
    "page.automations.title": "자동화",
    "page.automations.subtitle": "관리 명령과 관찰된 네이티브 작업입니다.",
    "page.discovery.title": "네이티브 탐색",
    "page.discovery.subtitle": "스케줄러와 지원되는 macOS 자동화 소스를 읽기 전용으로 조회합니다.",
    "page.runs.title": "실행 기록",
    "page.runs.subtitle": "변경할 수 없는 실행 기록과 제한된 stdout/stderr 로그입니다.",
    "page.inbox.title": "받은 편지함",
    "page.inbox.subtitle": "실패, 드리프트, 누락된 소스와 복구 항목입니다.",
    "page.integrations.title": "통합",
    "page.integrations.subtitle": "이 호스트에서 감지된 타입 기반 시맨틱 어댑터입니다.",
    "page.approvals.title": "승인",
    "page.approvals.subtitle": "계획에 연결되고 만료되며 한 번만 사용할 수 있는 네이티브 쓰기 요청입니다.",
    "page.metrics.title": "메트릭",
    "page.metrics.subtitle": "기록된 provider 및 운영 측정값입니다.",
    "page.events.title": "이벤트",
    "page.events.subtitle": "실행, 도입, 탐색, 승인에 대한 감사 기록입니다.",
    "section.registry": "레지스트리",
    "section.automationOverview": "자동화 개요",
    "section.needsAttention": "확인 필요",
    "table.name": "이름",
    "table.ownership": "소유",
    "table.state": "상태",
    "table.nextRun": "다음 실행",
    "table.severity": "심각도",
    "table.kind": "종류",
    "table.title": "제목",
    "table.status": "상태",
    "table.automation": "자동화",
    "table.started": "시작",
    "table.exit": "종료 코드",
    "table.nativeId": "네이티브 ID",
    "table.provider": "Provider",
    "table.path": "경로",
    "table.integration": "통합",
    "table.detection": "탐지",
    "table.doctor": "진단",
    "table.capabilities": "기능",
    "table.action": "작업",
    "table.risk": "위험",
    "table.expires": "만료",
    "table.key": "키",
    "table.value": "값",
    "table.source": "소스",
    "table.recorded": "기록",
    "table.seq": "순번",
    "table.type": "유형",
    "table.occurred": "발생",
    "table.payloadKeys": "페이로드 키",
    "button.run": "실행",
    "button.pause": "일시 중지",
    "button.resume": "재개",
    "button.logs": "로그",
    "button.cancel": "취소",
    "button.scan": "지금 탐색",
    "button.approve": "승인",
    "button.reject": "거부",
    "empty.automations": "등록된 자동화가 없습니다.",
    "empty.inbox": "확인할 항목이 없습니다.",
    "empty.runs": "기록된 실행이 없습니다.",
    "empty.discovery": "아직 네이티브 탐색을 실행하지 않았습니다.",
    "empty.integrations": "등록된 통합이 없습니다.",
    "empty.integrationStatus": "통합 상태를 사용할 수 없습니다.",
    "empty.approvals": "승인 대기열이 비어 있습니다.",
    "empty.metrics": "기록된 메트릭이 없습니다.",
    "empty.events": "기록된 이벤트가 없습니다.",
    "log.stdout": "표준 출력",
    "log.stderr": "표준 오류",
    "log.empty": "(비어 있음)",
    "value.observed": "관찰",
    "value.managed": "관리",
    "value.enabled": "활성화",
    "value.paused": "일시 중지됨",
    "value.needs_attention": "확인 필요",
    "value.succeeded": "성공",
    "value.running": "실행 중",
    "value.failed": "실패",
    "value.cancelled": "취소됨",
    "value.pending": "대기 중",
    "value.approved": "승인됨",
    "value.rejected": "거부됨",
    "value.available": "사용 가능",
    "value.missing": "누락",
    "value.ready": "준비됨",
    "value.unavailable": "사용 불가",
    "value.unknown": "알 수 없음",
    "value.manual": "수동",
    "value.read": "읽기",
    "value.destructive": "파괴적",
    "value.system_write": "시스템 쓰기",
    "value.network_write": "네트워크 쓰기",
    "integrationAction.doctor": "진단",
    "integrationAction.issues": "문제",
    "integrationAction.pulls": "풀 리퀘스트",
    "integrationAction.failed-runs": "실패한 실행",
    "integrationAction.checks": "검사",
    "integrationAction.scan": "탐색",
    "integrationAction.outdated": "오래된 항목",
    "integrationAction.bundle-check": "번들 검사",
    "integrationAction.upgrade": "업그레이드",
    "integrationAction.cleanup": "정리",
    "integrationAction.detect": "탐지",
    "integrationAction.version": "버전",
    "integrationAction.analyze": "분석",
    "integrationAction.status": "상태",
    "integrationAction.history": "기록",
    "integrationAction.clean": "정리",
    "integrationAction.list-remotes": "원격 목록",
    "integrationAction.snapshots": "스냅샷",
    "integrationAction.backup": "백업",
    "integrationAction.check": "검사",
    "integrationAction.forget": "삭제",
    "integrationAction.prune": "정리",
    "integrationAction.inspect": "검사",
    "integrationAction.plan": "계획",
    "integrationAction.run": "실행",
  },
};

const pageMeta = {
  dashboard: ["page.dashboard.title", "page.dashboard.subtitle"],
  automations: ["page.automations.title", "page.automations.subtitle"],
  discovery: ["page.discovery.title", "page.discovery.subtitle"],
  runs: ["page.runs.title", "page.runs.subtitle"],
  inbox: ["page.inbox.title", "page.inbox.subtitle"],
  integrations: ["page.integrations.title", "page.integrations.subtitle"],
  approvals: ["page.approvals.title", "page.approvals.subtitle"],
  metrics: ["page.metrics.title", "page.metrics.subtitle"],
  events: ["page.events.title", "page.events.subtitle"],
};

const state = {
  page: (location.hash.slice(1) || "dashboard").split("/")[0],
  status: null,
  automations: [],
  discovery: [],
  integrations: null,
  approvals: [],
  runs: [],
  inbox: [],
  metrics: [],
  events: [],
  error: null,
  busy: new Set(),
};

if (!pageMeta[state.page]) state.page = "dashboard";

const localeStorageKey = "taskrail.locale";
const app = document.querySelector("#app");
const escapeHtml = value => String(value ?? "")
  .replaceAll("&", "&amp;").replaceAll("<", "&lt;")
  .replaceAll(">", "&gt;").replaceAll('"', "&quot;");
const label = value => String(value ?? "").replaceAll("_", " ");
const wait = milliseconds => new Promise(resolve => setTimeout(resolve, milliseconds));

function normalizeLocale(value) {
  const candidate = String(value || "").toLowerCase();
  if (candidate.startsWith("zh")) return "zh-CN";
  if (candidate.startsWith("ja")) return "ja";
  if (candidate.startsWith("ko")) return "ko";
  return "en";
}

function detectLocale() {
  try {
    const stored = localStorage.getItem(localeStorageKey);
    if (stored) return normalizeLocale(stored);
  } catch (_) {
    // Private browsing can make localStorage unavailable; browser language still works.
  }
  const candidates = navigator.languages?.length ? navigator.languages : [navigator.language];
  const preferred = candidates.find(language => /^(en|zh|ja|ko)/i.test(language));
  return normalizeLocale(preferred || candidates[0]);
}

let locale = detectLocale();

function t(key, variables = {}) {
  const template = translations[locale]?.[key] || translations.en[key] || key;
  return template.replace(/\{(\w+)\}/g, (_, name) => String(variables[name] ?? `{${name}}`));
}

function valueLabel(value) {
  const key = `value.${String(value ?? "")}`;
  return translations[locale]?.[key] || translations.en[key] || label(value);
}

function capabilityLabel(value) {
  const key = `integrationAction.${String(value ?? "")}`;
  return translations[locale]?.[key] || translations.en[key] || label(value);
}

function pill(value, kind = "") {
  return `<span class="pill ${kind}">${escapeHtml(valueLabel(value))}</span>`;
}

function empty(message) {
  return `<div class="empty">${escapeHtml(message)}</div>`;
}

function setLocale(nextLocale) {
  locale = normalizeLocale(nextLocale);
  try { localStorage.setItem(localeStorageKey, locale); } catch (_) { /* best effort */ }
  document.documentElement.lang = locale;
  document.title = t("app.title");
  render();
}

async function request(path, options = {}) {
  const method = (options.method || "GET").toUpperCase();
  const attempts = method === "GET" ? 3 : 1;
  let lastError;
  for (let attempt = 0; attempt < attempts; attempt += 1) {
    try {
      const response = await fetch(path, {
        ...options,
        headers: { Accept: "application/json", ...(options.headers || {}) },
      });
      const body = await response.json().catch(() => ({}));
      if (!response.ok) throw new Error(body.error || `${response.status} ${response.statusText}`);
      return body;
    } catch (error) {
      lastError = error instanceof Error ? error : new Error(String(error));
      if (attempt + 1 < attempts) await wait(150 * (attempt + 1));
    }
  }
  throw lastError;
}

async function refresh(options = {}) {
  state.error = null;
  try {
    const includeIntegrations = options.includeIntegrations === true || state.page === "integrations" || state.integrations === null;
    const [status, automations, integrations, approvals, runs, inbox, metrics, events] = await Promise.all([
      request("/api/status"), request("/api/automations"), includeIntegrations ? request("/api/integrations") : Promise.resolve(state.integrations),
      request("/api/approvals?limit=100"), request("/api/runs?limit=100"),
      request("/api/inbox?limit=100"), request("/api/metrics"), request("/api/events?limit=100"),
    ]);
    state.status = status;
    state.automations = automations;
    state.integrations = integrations;
    state.approvals = approvals;
    state.runs = runs;
    state.inbox = inbox;
    state.metrics = metrics;
    state.events = events;
  } catch (error) {
    state.error = error instanceof Error ? error.message : String(error);
  }
  render();
}

async function act(key, path, options = {}) {
  state.busy.add(key); render();
  try { await request(path, { method: "POST", ...options }); await refresh(); }
  catch (error) { state.error = error instanceof Error ? error.message : String(error); render(); }
  finally { state.busy.delete(key); render(); }
}

function automationRows() {
  if (!state.automations.length) return empty(t("empty.automations"));
  return `<div class="table-wrap"><table><thead><tr><th>${t("table.name")}</th><th>${t("table.ownership")}</th><th>${t("table.state")}</th><th>${t("table.nextRun")}</th><th></th></tr></thead><tbody>${state.automations.map(item => {
    const id = escapeHtml(item.id);
    const paused = item.runtime_state === "paused";
    const attention = item.runtime_state === "needs_attention";
    const runKey = `run:${item.id}`;
    const pauseKey = `${paused ? "resume" : "pause"}:${item.id}`;
    return `<tr><td><strong>${escapeHtml(item.name)}</strong><br><code>${id}</code></td><td>${pill(item.ownership)}</td><td>${pill(item.runtime_state, attention ? "bad" : paused ? "warn" : "ok")}</td><td class="mono">${escapeHtml(item.next_run_at || t("value.manual"))}</td><td><div class="actions"><button class="mini" data-action="run" data-id="${id}" ${item.ownership === "observed" || state.busy.has(runKey) ? "disabled" : ""}>${t("button.run")}</button><button class="mini" data-action="${paused ? "resume" : "pause"}" data-id="${id}" ${item.ownership === "observed" || attention || state.busy.has(pauseKey) ? "disabled" : ""}>${t(paused ? "button.resume" : "button.pause")}</button></div></td></tr>`;
  }).join("")}</tbody></table></div>`;
}

function inboxRows() {
  if (!state.inbox.length) return empty(t("empty.inbox"));
  return `<div class="table-wrap"><table><thead><tr><th>${t("table.severity")}</th><th>${t("table.kind")}</th><th>${t("table.title")}</th><th>${t("table.status")}</th></tr></thead><tbody>${state.inbox.map(item => `<tr><td>${pill(item.severity, item.severity === "critical" || item.severity === "high" ? "bad" : "warn")}</td><td>${escapeHtml(item.kind)}</td><td><strong>${escapeHtml(item.title)}</strong><br><code>${escapeHtml(item.id)}</code></td><td>${pill(item.status)}</td></tr>`).join("")}</tbody></table></div>`;
}

function runRows() {
  if (!state.runs.length) return empty(t("empty.runs"));
  return `<div class="table-wrap"><table><thead><tr><th>${t("table.automation")}</th><th>${t("table.status")}</th><th>${t("table.started")}</th><th>${t("table.exit")}</th><th></th></tr></thead><tbody>${state.runs.map(run => `<tr><td><strong>${escapeHtml(run.automation_id)}</strong><br><code>${escapeHtml(run.id)}</code></td><td>${pill(run.status, run.status === "succeeded" ? "ok" : run.status === "running" ? "warn" : "bad")}</td><td class="mono">${escapeHtml(run.started_at)}</td><td>${escapeHtml(run.exit_code ?? "—")}</td><td><div class="actions"><button class="mini" data-action="logs" data-id="${escapeHtml(run.id)}">${t("button.logs")}</button>${run.status === "running" ? `<button class="mini danger" data-action="cancel" data-id="${escapeHtml(run.id)}">${t("button.cancel")}</button>` : ""}</div></td></tr>`).join("")}</tbody></table></div>`;
}

function pageBody() {
  switch (state.page) {
    case "automations": return `<h2 class="section-title">${t("page.automations.title")}</h2><p class="section-subtitle">${t("page.automations.subtitle")}</p><section class="panel"><div class="panel-head"><h2>${t("section.registry")}</h2><span class="muted">${t("count.items", { count: state.automations.length })}</span></div>${automationRows()}</section>`;
    case "discovery": return `<h2 class="section-title">${t("page.discovery.title")}</h2><p class="section-subtitle">${t("page.discovery.subtitle")}</p><div class="toolbar"><button class="button primary" data-action="discover">${t("button.scan")}</button><span class="muted">${t("common.scanNote")}</span></div><section class="panel">${state.discovery.length ? `<div class="table-wrap"><table><thead><tr><th>${t("table.nativeId")}</th><th>${t("table.provider")}</th><th>${t("table.kind")}</th><th>${t("table.state")}</th><th>${t("table.path")}</th></tr></thead><tbody>${state.discovery.map(item => `<tr><td><code>${escapeHtml(item.native_id)}</code></td><td>${escapeHtml(item.provider)}</td><td>${escapeHtml(item.kind)}${item.execution === "observe_only" || !item.command ? " · observe-only" : ""}</td><td>${pill(item.enabled ? "enabled" : "paused", item.enabled ? "ok" : "warn")}</td><td class="mono">${escapeHtml(item.path || "—")}</td></tr>`).join("")}</tbody></table></div>` : empty(t("empty.discovery"))}</section>`;
    case "runs": return `<h2 class="section-title">${t("page.runs.title")}</h2><p class="section-subtitle">${t("page.runs.subtitle")}</p><section class="panel">${runRows()}</section><div id="log-detail"></div>`;
    case "inbox": return `<h2 class="section-title">${t("page.inbox.title")}</h2><p class="section-subtitle">${t("page.inbox.subtitle")}</p><section class="panel">${inboxRows()}</section>`;
    case "integrations": return `<h2 class="section-title">${t("page.integrations.title")}</h2><p class="section-subtitle">${t("page.integrations.subtitle")}</p><section class="panel">${integrationBody()}</section>`;
    case "approvals": return `<h2 class="section-title">${t("page.approvals.title")}</h2><p class="section-subtitle">${t("page.approvals.subtitle")}</p><section class="panel">${approvalRows()}</section>`;
    case "metrics": return `<h2 class="section-title">${t("page.metrics.title")}</h2><p class="section-subtitle">${t("page.metrics.subtitle")}</p><section class="panel">${metricRows()}</section>`;
    case "events": return `<h2 class="section-title">${t("page.events.title")}</h2><p class="section-subtitle">${t("page.events.subtitle")}</p><section class="panel">${eventRows()}</section>`;
    default: return dashboardBody();
  }
}

function dashboardBody() {
  const status = state.status || {};
  const discovery = status.native_discovery || {};
  return `<div class="cards"><div class="card"><div class="card-label">${t("card.automations")}</div><div class="card-value">${state.automations.length}</div><div class="card-detail">${t("detail.managedObserved", { managed: status.managed_count || 0, observed: status.observed_count || 0 })}</div></div><div class="card"><div class="card-label">${t("card.recentRuns")}</div><div class="card-value">${state.runs.length}</div><div class="card-detail">${t("detail.succeeded", { count: state.runs.filter(run => run.status === "succeeded").length })}</div></div><div class="card"><div class="card-label">${t("card.needsAttention")}</div><div class="card-value">${state.inbox.length}</div><div class="card-detail">${t("detail.failuresVisible")}</div></div><div class="card"><div class="card-label">${t("card.pendingApprovals")}</div><div class="card-value">${state.approvals.filter(item => item.status === "pending").length}</div><div class="card-detail">${t("detail.typedPlans")}</div></div></div><div class="grid-2"><section class="panel"><div class="panel-head"><h2>${t("section.automationOverview")}</h2><span class="muted">${escapeHtml(status.host?.label || t("common.localHost"))}</span></div>${automationRows()}</section><section class="panel"><div class="panel-head"><h2>${t("section.needsAttention")}</h2><span class="muted">${t("count.nativeSources", { count: discovery.source_count || 0 })}</span></div>${inboxRows()}</section></div>`;
}

function integrationBody() {
  if (!state.integrations) return empty(t("empty.integrationStatus"));
  const descriptors = state.integrations.descriptors || [];
  const detection = state.integrations.detection || [];
  const doctor = state.integrations.doctor || [];
  if (!descriptors.length) return empty(t("empty.integrations"));
  return `<div class="table-wrap"><table><thead><tr><th>${t("table.integration")}</th><th>${t("table.detection")}</th><th>${t("table.doctor")}</th><th>${t("table.capabilities")}</th></tr></thead><tbody>${descriptors.map(item => { const d = detection.find(row => row.integration === item.id); const health = doctor.find(row => row.integration === item.id); return `<tr><td><strong>${escapeHtml(item.display_name)}</strong><br><code>${escapeHtml(item.id)}</code></td><td>${pill(d?.status || "unknown", d?.status === "available" ? "ok" : "warn")}</td><td>${pill(health?.status || "unknown", health?.status === "ready" ? "ok" : "warn")}</td><td class="muted">${escapeHtml((item.capabilities || []).map(cap => `${capabilityLabel(cap.action)} (${valueLabel(cap.risk)})`).join(", "))}</td></tr>`; }).join("")}</tbody></table></div>`;
}

function approvalRows() {
  if (!state.approvals.length) return empty(t("empty.approvals"));
  return `<div class="table-wrap"><table><thead><tr><th>${t("table.action")}</th><th>${t("table.risk")}</th><th>${t("table.status")}</th><th>${t("table.expires")}</th><th></th></tr></thead><tbody>${state.approvals.map(item => `<tr><td><strong>${escapeHtml(item.integration)} · ${escapeHtml(item.action)}</strong><br><span class="muted">${escapeHtml(item.reason)}</span></td><td>${pill(item.risk, item.risk === "destructive" || item.risk === "system_write" ? "bad" : "warn")}</td><td>${pill(item.status, item.status === "pending" ? "warn" : item.status === "approved" ? "ok" : "")}</td><td class="mono">${escapeHtml(item.expires_at)}</td><td>${item.status === "pending" ? `<div class="actions"><button class="mini" data-action="approve" data-id="${escapeHtml(item.id)}">${t("button.approve")}</button><button class="mini danger" data-action="reject" data-id="${escapeHtml(item.id)}">${t("button.reject")}</button></div>` : ""}</td></tr>`).join("")}</tbody></table></div>`;
}

function metricRows() {
  if (!state.metrics.length) return empty(t("empty.metrics"));
  return `<div class="table-wrap"><table><thead><tr><th>${t("table.key")}</th><th>${t("table.value")}</th><th>${t("table.source")}</th><th>${t("table.recorded")}</th></tr></thead><tbody>${state.metrics.map(item => `<tr><td>${escapeHtml(item.key)}</td><td><strong>${escapeHtml(item.value)} ${escapeHtml(item.unit)}</strong></td><td>${escapeHtml(item.source)}</td><td class="mono">${escapeHtml(item.recorded_at)}</td></tr>`).join("")}</tbody></table></div>`;
}

function eventRows() {
  if (!state.events.length) return empty(t("empty.events"));
  return `<div class="table-wrap"><table><thead><tr><th>${t("table.seq")}</th><th>${t("table.type")}</th><th>${t("table.occurred")}</th><th>${t("table.payloadKeys")}</th></tr></thead><tbody>${state.events.map(item => `<tr><td class="mono">#${escapeHtml(item.seq)}</td><td><strong>${escapeHtml(item.event_type)}</strong></td><td class="mono">${escapeHtml(item.occurred_at)}</td><td class="muted">${escapeHtml(Object.keys(item.payload || {}).join(", ") || "—")}</td></tr>`).join("")}</tbody></table></div>`;
}

function localePicker() {
  return `<label class="locale-control"><span class="sr-only">${escapeHtml(t("language"))}</span><select data-locale aria-label="${escapeHtml(t("language"))}">${localeChoices.map(([id, name]) => `<option value="${id}" ${id === locale ? "selected" : ""}>${name}</option>`).join("")}</select></label>`;
}

function render() {
  document.documentElement.lang = locale;
  document.title = t("app.title");
  const status = state.status;
  const connected = Boolean(status);
  const [titleKey, subtitleKey] = pageMeta[state.page] || pageMeta.dashboard;
  const title = t(titleKey);
  app.innerHTML = `<div class="app"><aside class="sidebar" id="sidebar"><div class="brand"><span class="brand-mark">T</span><span class="brand-name">Taskrail</span><span class="brand-version">${t("brand.web")}</span></div><nav class="nav">${navItems.map(([id, icon, textKey]) => `<button class="${state.page === id ? "active" : ""}" data-page="${id}"><span class="nav-icon">${icon}</span>${t(textKey)}</button>`).join("")}</nav><div class="sidebar-foot"><span>${t("sidebar.local")}</span><span class="mono">${escapeHtml(status?.host?.label || t("sidebar.unavailable"))}</span><a href="/healthz" target="_blank" rel="noreferrer">${t("sidebar.healthz")}</a></div></aside><main class="main"><header class="topbar"><button class="button mobile-menu" data-action="menu">☰</button><div><h1>${escapeHtml(title)}</h1><p>${t(subtitleKey)}</p><div class="status"><span class="status-dot ${connected ? "ok" : ""}"></span>${connected ? escapeHtml(t("status.connected", { platform: status.host?.platform || "local", architecture: status.host?.architecture || "" })) : t("status.unavailable")}</div></div><div class="topbar-actions">${localePicker()}<button class="button" data-action="refresh">${t("common.refresh")}</button></div></header>${state.error ? `<div class="notice">${escapeHtml(`${t("common.error")}: ${state.error}`)}</div>` : ""}${pageBody()}</main></div>`;
  bindEvents();
}

function bindEvents() {
  document.querySelector("[data-locale]")?.addEventListener("change", event => setLocale(event.target.value));
  document.querySelectorAll("[data-page]").forEach(button => button.addEventListener("click", () => { location.hash = button.dataset.page; }));
  document.querySelectorAll("[data-action]").forEach(button => button.addEventListener("click", () => {
    const action = button.dataset.action;
    const id = button.dataset.id;
    if (action === "refresh") return refresh();
    if (action === "menu") return document.querySelector("#sidebar")?.classList.toggle("open");
    if (action === "discover") return request("/api/discovery?source=all").then(rows => { state.discovery = rows; render(); }).catch(error => { state.error = error.message; render(); });
    if (action === "logs") return request(`/api/runs/${encodeURIComponent(id)}/logs`).then(logs => { const target = document.querySelector("#log-detail"); if (target) target.innerHTML = `<section class="panel" style="margin-top:14px"><div class="panel-head"><h2>${escapeHtml(logs.automation_id)} · ${escapeHtml(valueLabel(logs.status))}</h2></div><div class="panel-body stack"><div><div class="muted">${t("log.stdout")}</div><div class="log">${escapeHtml(logs.stdout || t("log.empty"))}</div></div><div><div class="muted">${t("log.stderr")}</div><div class="log">${escapeHtml(logs.stderr || t("log.empty"))}</div></div></div></section>`; }).catch(error => { state.error = error.message; render(); });
    const path = action === "run" ? `/api/automations/${encodeURIComponent(id)}/run` : action === "pause" ? `/api/automations/${encodeURIComponent(id)}/pause` : action === "resume" ? `/api/automations/${encodeURIComponent(id)}/resume` : action === "cancel" ? `/api/runs/${encodeURIComponent(id)}/cancel` : action === "approve" ? `/api/approvals/${encodeURIComponent(id)}/approve` : action === "reject" ? `/api/approvals/${encodeURIComponent(id)}/reject` : null;
    if (path) return act(`${action}:${id}`, path);
  }));
}

window.addEventListener("hashchange", () => { state.page = (location.hash.slice(1) || "dashboard").split("/")[0]; if (!pageMeta[state.page]) state.page = "dashboard"; refresh({ includeIntegrations: state.page === "integrations" }); });
document.addEventListener("visibilitychange", () => { if (!document.hidden) refresh(); });
render();
refresh();
setInterval(() => { if (!document.hidden && state.page === "dashboard") refresh(); }, 5000);
