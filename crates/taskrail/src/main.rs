use anyhow::{Context, Result};
use chrono::Utc;
use clap::{Parser, Subcommand, ValueEnum};
use std::{
    collections::BTreeMap,
    io::{self, IsTerminal},
    net::SocketAddr,
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
};
use taskrail::{
    adoption::{adopt_source, rollback_source},
    codex::{self, CodexRequest, CodexSandbox},
    core::{Automation, CommandSpec, Event, Metric, Ownership, RuntimeState, StepSpec, Trigger},
    discovery::scan_native_sources,
    github::{self, GhQuery, QueryKind},
    integrations::{
        GithubIntegration, HomebrewIntegration, Integration,
        IntegrationAction as SemanticIntegrationAction, MasIntegration, MoleIntegration,
        RcloneIntegration, ResticIntegration, SecurityIntegration, TopgradeIntegration,
    },
    mcp, rpc, service,
    storage::Registry,
    verification::{self, VerificationCommand},
    worktree,
};
use uuid::Uuid;

#[derive(Debug, Parser)]
#[command(
    name = "taskrail",
    version,
    about = "A local-first ARM64 automation control plane for macOS and Linux"
)]
struct Cli {
    /// Override the local SQLite Registry path.
    #[arg(long, env = "TASKRAIL_REGISTRY")]
    registry: Option<PathBuf>,
    #[command(subcommand)]
    command: Action,
}

#[derive(Debug, Clone, ValueEnum)]
enum SourceKind {
    All,
    Launchd,
    Cron,
    Systemd,
    Homebrew,
}

#[derive(Debug, Clone, ValueEnum)]
enum HttpProfileArg {
    /// Expose only authenticated, read-only inspection tools.
    PublicReadOnly,
    /// Expose the full local tool set to an authenticated private host connection.
    Private,
}

#[derive(Debug, Subcommand)]
enum Action {
    /// Discover existing native automations without changing them.
    Scan {
        #[arg(long, value_enum, default_value_t = SourceKind::All)]
        source: SourceKind,
        #[arg(long)]
        json: bool,
    },
    /// List Registry automations.
    List {
        #[arg(long)]
        json: bool,
    },
    /// Inspect one automation or source.
    Inspect { id: String },
    /// Delete a managed automation that has no recorded run history.
    Delete { id: String },
    /// Register a managed automation from a YAML definition.
    Register { file: PathBuf },
    /// Add a simple command automation without writing YAML first.
    Add {
        /// Stable identifier used by `run`, `pause`, and `logs`.
        id: String,
        /// Executable to run, such as `mo`, `gh`, or an absolute path.
        executable: PathBuf,
        /// Optional display name; defaults to the identifier.
        #[arg(long)]
        name: Option<String>,
        /// Argument passed to the executable. Repeat for multiple arguments.
        #[arg(long = "arg")]
        args: Vec<String>,
        /// Run every N seconds.
        #[arg(long, conflicts_with = "cron")]
        every_seconds: Option<u64>,
        /// Run on a five-field cron expression.
        #[arg(long, conflicts_with = "every_seconds")]
        cron: Option<String>,
        /// Timezone for a cron trigger.
        #[arg(long, default_value = "local")]
        timezone: String,
        /// Working directory for the command.
        #[arg(long)]
        cwd: Option<PathBuf>,
    },
    /// Schedule a typed read-only or dry-run native integration.
    ScheduleIntegration {
        /// Stable identifier used by `run`, `pause`, and `logs`.
        id: String,
        /// Built-in integration id, such as `homebrew` or `gitleaks`.
        integration: String,
        /// Typed action exposed by the selected integration.
        action: String,
        /// JSON object passed to the typed integration action.
        #[arg(long, default_value = "{}")]
        parameters: String,
        /// Optional display name; defaults to integration and action.
        #[arg(long)]
        name: Option<String>,
        /// Run every N seconds.
        #[arg(long, conflicts_with = "cron")]
        every_seconds: Option<u64>,
        /// Run on a five-field cron expression.
        #[arg(long, conflicts_with = "every_seconds")]
        cron: Option<String>,
        /// Timezone for a cron trigger.
        #[arg(long, default_value = "local")]
        timezone: String,
    },
    /// Run Codex non-interactively as an optional automation executor.
    CodexRun {
        #[arg(long, default_value = ".")]
        cwd: PathBuf,
        #[arg(long, conflicts_with = "prompt_file")]
        prompt: Option<String>,
        #[arg(long, conflicts_with = "prompt")]
        prompt_file: Option<PathBuf>,
        #[arg(long, value_enum, default_value_t = CodexSandboxArg::ReadOnly)]
        sandbox: CodexSandboxArg,
        #[arg(long)]
        model: Option<String>,
        /// Optional Codex model catalog path passed through as a config override.
        #[arg(long, env = "TASKRAIL_CODEX_MODEL_CATALOG")]
        model_catalog_json: Option<PathBuf>,
        #[arg(long)]
        output_schema: Option<PathBuf>,
        #[arg(long)]
        worktree_dir: Option<PathBuf>,
        #[arg(long, default_value_t = 30 * 60)]
        timeout_seconds: u64,
    },
    /// Run a prompt through an OpenAI Responses-compatible API.
    ResponsesRun {
        #[arg(long, conflicts_with = "prompt_file")]
        prompt: Option<String>,
        #[arg(long, conflicts_with = "prompt")]
        prompt_file: Option<PathBuf>,
        #[arg(long, default_value = "https://api.openai.com/v1")]
        base_url: String,
        #[arg(long, default_value = "gpt-5")]
        model: String,
        /// Name of the environment variable containing the API key.
        #[arg(long, default_value = "OPENAI_API_KEY")]
        api_key_env: String,
        /// Opt into provider-side response storage. The default is store=false.
        #[arg(long)]
        store: bool,
        #[arg(long, default_value_t = 30 * 60)]
        timeout_seconds: u64,
        #[arg(long)]
        json: bool,
    },
    /// Run an automation once.
    Run {
        id: String,
        /// Observed automations are read-only by default; this flag is an explicit exception.
        #[arg(long)]
        allow_observed: bool,
    },
    /// Pause a managed or adopted automation without changing its native source.
    Pause { id: String },
    /// Resume a paused managed or adopted automation.
    Resume { id: String },
    /// Cancel an active run in this daemon.
    Cancel { run_id: String },
    /// Read bounded stdout/stderr for a recorded run.
    Logs { run_id: String },
    /// Acknowledge a source drift and update its baseline; requires --apply.
    AcknowledgeDrift {
        id: String,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        apply: bool,
    },
    /// Adopt a native source. Native mutation requires --apply.
    Adopt {
        id: String,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        apply: bool,
    },
    /// Restore the native snapshot for an adoption transaction.
    Rollback { tx_id: String },
    /// List recent adoption journal records without changing native state.
    Adoptions {
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
    /// List persisted integration approval requests.
    Approvals {
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
    /// Request a persisted approval for a typed native integration action.
    ApprovalRequest {
        #[command(subcommand)]
        action: ApprovalAction,
        #[arg(long, default_value_t = 3600)]
        ttl_seconds: u64,
    },
    /// Approve or reject a persisted native integration request.
    ApprovalDecide {
        id: String,
        #[arg(long, conflicts_with = "reject")]
        approve: bool,
        #[arg(long)]
        reject: bool,
    },
    /// Execute one approved request and consume its one-time grant.
    ApprovalExecute { approval_id: String },
    /// Inspect one adoption journal record without changing native state.
    AdoptionInspect { tx_id: String },
    /// Check ownership and permission invariants.
    Doctor {
        #[arg(value_enum)]
        check: Option<DoctorCheck>,
    },
    /// List bounded read-only items that need operator attention.
    Inbox {
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
    /// List locally recorded operational and provider usage metrics.
    Metrics,
    /// List the newest local audit events.
    Events {
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
    /// List recent immutable run records and their automation snapshots.
    Runs {
        #[arg(long)]
        automation: Option<String>,
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
    /// Explain what a run would do without executing it.
    Explain { id: String },
    /// Print a low-cost terminal dashboard.
    Tui,
    /// Run a deterministic argv verifier in a directory.
    Verify {
        #[arg(long, default_value = ".")]
        cwd: PathBuf,
        #[arg(long)]
        executable: PathBuf,
        #[arg(long = "arg")]
        args: Vec<String>,
        #[arg(long, default_value_t = 0)]
        expected_exit_code: i32,
        #[arg(long, default_value_t = 30 * 60)]
        timeout_seconds: u64,
    },
    /// Read structured GitHub state through `gh` without write operations.
    GithubWatch {
        #[arg(long)]
        repo: String,
        #[arg(long, value_enum, default_value_t = GithubQueryArg::Pulls)]
        query: GithubQueryArg,
        #[arg(long)]
        pull_number: Option<u64>,
        #[arg(long, default_value_t = 60)]
        timeout_seconds: u64,
        /// Poll repeatedly; omitted means one snapshot and exit.
        #[arg(long)]
        interval_seconds: Option<u64>,
    },
    /// Create or remove an isolated Git worktree.
    Worktree {
        #[command(subcommand)]
        action: WorktreeAction,
    },
    /// Evaluate scheduled managed/adopted automations.
    Daemon {
        /// Perform one scheduler pass and exit.
        #[arg(long)]
        once: bool,
        /// Expose the local JSON-RPC control plane on the platform endpoint.
        #[arg(long)]
        socket: Option<PathBuf>,
        #[arg(long, default_value_t = 30)]
        interval_seconds: u64,
        /// Install the per-user service that keeps the scheduler running.
        #[arg(long, conflicts_with = "uninstall")]
        install: bool,
        /// Remove the installed per-user service.
        #[arg(long, conflicts_with = "install")]
        uninstall: bool,
        /// Refresh the local native-automation inventory at this interval.
        #[arg(long, default_value_t = 5 * 60)]
        discovery_interval_seconds: u64,
    },
    /// Print the Registry path and daemon boundary.
    Status,
    /// List every built-in semantic integration and its declared capabilities.
    Integrations,
    /// Diagnose local integration availability without running an automation.
    Integration {
        #[command(subcommand)]
        action: IntegrationAction,
    },
    /// Expose the local Taskrail daemon as an MCP server over stdio.
    Mcp {
        /// Local endpoint served by `taskrail daemon`.
        #[arg(long)]
        socket: Option<PathBuf>,
    },
    /// Expose an explicit multi-host Taskrail fleet gateway over stdio.
    McpFleet {
        /// YAML config containing named MCP endpoints and token environment references.
        #[arg(long, env = "TASKRAIL_FLEET_CONFIG")]
        config: Option<PathBuf>,
    },
    /// Expose an authenticated MCP profile over Streamable HTTP.
    McpHttp {
        /// Local address for a TLS-terminating reverse proxy to reach.
        #[arg(long, env = "TASKRAIL_MCP_HTTP_BIND", default_value = "127.0.0.1:8787")]
        bind: SocketAddr,
        /// Name of the environment variable containing the bearer token.
        #[arg(
            long,
            env = "TASKRAIL_MCP_BEARER_TOKEN_ENV",
            default_value = "TASKRAIL_MCP_BEARER_TOKEN"
        )]
        bearer_token_env: String,
        /// Name of the environment variable containing comma-separated allowed Origin values.
        #[arg(
            long,
            env = "TASKRAIL_MCP_ALLOWED_ORIGINS_ENV",
            default_value = "TASKRAIL_MCP_ALLOWED_ORIGINS"
        )]
        allowed_origins_env: String,
        /// Maximum accepted JSON request body size.
        #[arg(long, env = "TASKRAIL_MCP_MAX_BODY_BYTES", default_value_t = 1024 * 1024)]
        max_body_bytes: usize,
        /// Explicit HTTP exposure profile; public-read-only is the safe default.
        #[arg(
            long,
            value_enum,
            env = "TASKRAIL_MCP_HTTP_PROFILE",
            default_value = "public-read-only"
        )]
        profile: HttpProfileArg,
        /// Local endpoint served by `taskrail daemon` (restricted Unix socket).
        #[arg(long)]
        socket: Option<PathBuf>,
    },
}

#[derive(Debug, Clone, ValueEnum)]
enum DoctorCheck {
    Ownership,
    Permissions,
    Drift,
    Adoption,
}

#[derive(Debug, Clone, ValueEnum)]
enum CodexSandboxArg {
    ReadOnly,
    WorkspaceWrite,
}

#[derive(Debug, Clone, ValueEnum)]
enum GithubQueryArg {
    Issues,
    Pulls,
    FailedRuns,
    Checks,
}

#[derive(Debug, Subcommand)]
enum WorktreeAction {
    Create {
        repository: PathBuf,
        path: PathBuf,
        #[arg(long)]
        base: Option<String>,
    },
    Remove {
        repository: PathBuf,
        path: PathBuf,
        #[arg(long)]
        force: bool,
    },
}

#[derive(Debug, Subcommand)]
// Keep the explicit `*-doctor` names stable and discoverable in the CLI.
#[allow(clippy::enum_variant_names)]
enum IntegrationAction {
    /// Run the Mole semantic integration.
    Mole {
        #[command(subcommand)]
        action: MoleAction,
    },
    /// Run the restic semantic integration.
    Restic {
        #[command(subcommand)]
        action: ResticAction,
    },
    /// Run the rclone semantic integration.
    Rclone {
        #[command(subcommand)]
        action: RcloneAction,
    },
    /// Run the existing read-only GitHub integration through the semantic layer.
    Github {
        #[command(subcommand)]
        action: GithubAction,
    },
    /// Run Homebrew health and maintenance actions through the semantic layer.
    Homebrew {
        #[command(subcommand)]
        action: HomebrewAction,
    },
    /// Run the macOS App Store read-only integration.
    Mas {
        #[command(subcommand)]
        action: MasAction,
    },
    /// Run the OSV-Scanner read-only integration.
    OsvScanner {
        #[command(subcommand)]
        action: ScannerAction,
    },
    /// Run the Gitleaks read-only integration.
    Gitleaks {
        #[command(subcommand)]
        action: ScannerAction,
    },
    /// Run the Trivy read-only integration.
    Trivy {
        #[command(subcommand)]
        action: ScannerAction,
    },
    /// Inspect or request a policy-controlled Topgrade system update.
    Topgrade {
        #[command(subcommand)]
        action: TopgradeAction,
    },
    /// Check Codex CLI availability and Git-repository preconditions.
    CodexDoctor {
        #[arg(long, default_value = ".")]
        cwd: PathBuf,
    },
    /// Check GitHub CLI availability and local authentication state.
    GhDoctor {
        #[arg(long, default_value = "github.com")]
        hostname: String,
    },
    /// Check the local Taskrail/MCP prerequisites for ChatGPT Scheduled tasks.
    ChatgptDoctor {
        /// Profile name used by `tunnel-client`; this check never prints credentials.
        #[arg(long, default_value = "taskrail-local")]
        profile: String,
    },
    /// Start a managed tunnel-client runtime for this Taskrail host.
    ChatgptConnect {
        /// Existing OpenAI Tunnel ID; defaults to CONTROL_PLANE_TUNNEL_ID.
        #[arg(long, env = "CONTROL_PLANE_TUNNEL_ID")]
        tunnel_id: Option<String>,
        /// Alias used by tunnel-client runtime supervision.
        #[arg(long, default_value = "taskrail-local")]
        alias: String,
        /// Profile written by tunnel-client.
        #[arg(long, default_value = "taskrail-local")]
        profile: String,
    },
}

#[derive(Debug, Subcommand)]
enum MoleAction {
    /// Detect Mole and report its version.
    Detect,
    /// Check Mole availability without running cleanup.
    Doctor,
    /// Read the Mole version through the semantic execution path.
    Version,
    /// Analyze disk usage with Mole's structured JSON mode.
    Analyze,
    /// Read current system health with Mole's structured JSON mode.
    Status,
    /// Read bounded cleanup history.
    History {
        #[arg(long, default_value_t = 20)]
        limit: u64,
    },
    /// Preview cleanup. Real cleanup remains approval-gated and unavailable here.
    Clean {
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Debug, Subcommand)]
enum ResticAction {
    /// Detect restic and report its version.
    Detect,
    /// Check restic CLI availability without accessing a repository.
    Doctor,
    /// Read repository snapshots using bounded JSON output.
    Snapshots,
    /// Back up one explicit path; execution is held by policy.
    Backup {
        path: String,
        #[arg(long)]
        repository_env: Option<String>,
        #[arg(long)]
        password_env: Option<String>,
    },
    /// Check repository integrity.
    Check,
    /// Preview the typed forget operation; execution is held by policy.
    Forget,
    /// Preview the typed prune operation; execution is held by policy.
    Prune,
}

#[derive(Debug, Subcommand)]
enum RcloneAction {
    /// Detect rclone and report its version.
    Detect,
    /// Check rclone CLI availability without transferring data.
    Doctor,
    /// List configured remotes without exposing credentials.
    ListRemotes,
    /// Compare explicit source and destination paths.
    Check { source: String, destination: String },
    /// Copy from one explicit source to one explicit destination.
    Copy { source: String, destination: String },
    /// Preview or request a typed sync; real sync is held by policy.
    Sync {
        source: String,
        destination: String,
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Debug, Subcommand)]
enum GithubAction {
    Detect,
    Doctor,
    Issues { repo: String },
    Pulls { repo: String },
    FailedRuns { repo: String },
    Checks { repo: String, pull_number: u64 },
}

#[derive(Debug, Subcommand)]
enum HomebrewAction {
    Detect,
    Doctor,
    Outdated,
    BundleCheck {
        file: String,
    },
    Upgrade {
        #[arg(long)]
        dry_run: bool,
    },
    Cleanup {
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Debug, Subcommand)]
enum MasAction {
    Detect,
    Doctor,
    List,
    Outdated,
}

#[derive(Debug, Subcommand)]
enum ScannerAction {
    Detect,
    Doctor,
    Scan {
        path: String,
        #[arg(long)]
        baseline: Option<String>,
        #[arg(long, default_value = "filesystem")]
        scan_type: String,
    },
}

#[derive(Debug, Subcommand)]
enum TopgradeAction {
    Detect,
    Doctor,
    Inspect,
    Plan,
    Run,
}

#[derive(Debug, Subcommand)]
enum ApprovalAction {
    MoleClean {
        #[arg(long)]
        dry_run: bool,
    },
    ResticBackup {
        path: String,
    },
    ResticForget,
    ResticPrune,
    RcloneCopy {
        source: String,
        destination: String,
    },
    RcloneSync {
        source: String,
        destination: String,
    },
    HomebrewUpgrade,
    HomebrewCleanup,
    TopgradeRun,
}

struct CodexCliOptions {
    cwd: PathBuf,
    prompt: Option<String>,
    prompt_file: Option<PathBuf>,
    sandbox: CodexSandboxArg,
    model: Option<String>,
    model_catalog_json: Option<PathBuf>,
    output_schema: Option<PathBuf>,
    worktree_dir: Option<PathBuf>,
    timeout_seconds: u64,
}

struct ResponsesCliOptions {
    prompt: Option<String>,
    prompt_file: Option<PathBuf>,
    base_url: String,
    model: String,
    api_key_env: String,
    store: bool,
    timeout_seconds: u64,
    json: bool,
}

struct AddOptions {
    id: String,
    executable: PathBuf,
    name: Option<String>,
    args: Vec<String>,
    every_seconds: Option<u64>,
    cron: Option<String>,
    timezone: String,
    cwd: Option<PathBuf>,
}

struct ScheduleIntegrationOptions {
    id: String,
    integration: String,
    action: String,
    parameters: String,
    name: Option<String>,
    every_seconds: Option<u64>,
    cron: Option<String>,
    timezone: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let registry_path = cli.registry.unwrap_or_else(default_registry_path);
    let command = match cli.command {
        Action::Mcp { socket } => {
            return mcp::serve_stdio(socket.unwrap_or_else(default_socket_path)).await;
        }
        Action::McpFleet { config } => {
            return mcp::serve_fleet_stdio(config.unwrap_or_else(default_fleet_config_path)).await;
        }
        Action::McpHttp {
            bind,
            bearer_token_env,
            allowed_origins_env,
            max_body_bytes,
            profile,
            socket,
        } => {
            let profile = match profile {
                HttpProfileArg::PublicReadOnly => mcp::HttpProfile::PublicReadOnly,
                HttpProfileArg::Private => mcp::HttpProfile::Private,
            };
            return mcp::serve_http(
                socket.unwrap_or_else(default_socket_path),
                bind,
                bearer_token_env,
                allowed_origins_env,
                max_body_bytes,
                profile,
            )
            .await;
        }
        command => command,
    };
    let registry = Registry::open(registry_path)?;
    match command {
        Action::Scan { source, json } => scan(&registry, source, json),
        Action::List { json } => list(&registry, json),
        Action::Inspect { id } => inspect(&registry, &id),
        Action::Delete { id } => delete(&registry, &id),
        Action::Register { file } => register(&registry, &file),
        Action::Add {
            id,
            executable,
            name,
            args,
            every_seconds,
            cron,
            timezone,
            cwd,
        } => add(
            &registry,
            AddOptions {
                id,
                executable,
                name,
                args,
                every_seconds,
                cron,
                timezone,
                cwd,
            },
        ),
        Action::ScheduleIntegration {
            id,
            integration,
            action,
            parameters,
            name,
            every_seconds,
            cron,
            timezone,
        } => schedule_integration(
            &registry,
            ScheduleIntegrationOptions {
                id,
                integration,
                action,
                parameters,
                name,
                every_seconds,
                cron,
                timezone,
            },
        ),
        Action::CodexRun {
            cwd,
            prompt,
            prompt_file,
            sandbox,
            model,
            model_catalog_json,
            output_schema,
            worktree_dir,
            timeout_seconds,
        } => {
            codex_run(
                &registry,
                CodexCliOptions {
                    cwd,
                    prompt,
                    prompt_file,
                    sandbox,
                    model,
                    model_catalog_json,
                    output_schema,
                    worktree_dir,
                    timeout_seconds,
                },
            )
            .await
        }
        Action::ResponsesRun {
            prompt,
            prompt_file,
            base_url,
            model,
            api_key_env,
            store,
            timeout_seconds,
            json,
        } => {
            responses_run(
                &registry,
                ResponsesCliOptions {
                    prompt,
                    prompt_file,
                    base_url,
                    model,
                    api_key_env,
                    store,
                    timeout_seconds,
                    json,
                },
            )
            .await
        }
        Action::Run { id, allow_observed } => run(&registry, &id, allow_observed).await,
        Action::Adopt { id, dry_run, apply } => adopt(&registry, &id, dry_run, apply),
        Action::Rollback { tx_id } => rollback(&registry, &tx_id),
        Action::Adoptions { limit } => adoptions(&registry, limit),
        Action::Approvals { limit } => approvals(&registry, limit),
        Action::ApprovalRequest {
            action,
            ttl_seconds,
        } => approval_request(&registry, action, ttl_seconds),
        Action::ApprovalDecide {
            id,
            approve,
            reject,
        } => approval_decide(&registry, &id, approve, reject),
        Action::ApprovalExecute { approval_id } => approval_execute(&registry, &approval_id).await,
        Action::AdoptionInspect { tx_id } => adoption_inspect(&registry, &tx_id),
        Action::Doctor { check } => doctor(&registry, check),
        Action::Inbox { limit } => inbox(&registry, limit),
        Action::Metrics => metrics(&registry),
        Action::Events { limit } => events(&registry, limit),
        Action::Runs { automation, limit } => runs(&registry, automation.as_deref(), limit),
        Action::Pause { id } => transition_runtime_state(&registry, &id, RuntimeState::Paused),
        Action::Resume { id } => transition_runtime_state(&registry, &id, RuntimeState::Enabled),
        Action::Cancel { run_id } => cancel_run(&registry, &run_id),
        Action::Logs { run_id } => logs(&registry, &run_id),
        Action::AcknowledgeDrift { id, dry_run, apply } => {
            acknowledge_drift(&registry, &id, dry_run, apply)
        }
        Action::Explain { id } => explain(&registry, &id),
        Action::Tui => dashboard(&registry),
        Action::Verify {
            cwd,
            executable,
            args,
            expected_exit_code,
            timeout_seconds,
        } => verify(&cwd, executable, args, expected_exit_code, timeout_seconds).await,
        Action::GithubWatch {
            repo,
            query,
            pull_number,
            timeout_seconds,
            interval_seconds,
        } => {
            github_watch(
                &registry,
                repo,
                query,
                pull_number,
                timeout_seconds,
                interval_seconds,
            )
            .await
        }
        Action::Worktree { action } => worktree_action(action),
        Action::Daemon {
            once,
            socket,
            interval_seconds,
            install,
            uninstall,
            discovery_interval_seconds,
        } => {
            if install {
                install_daemon(&registry, discovery_interval_seconds)
            } else if uninstall {
                uninstall_daemon()
            } else {
                daemon(
                    &registry,
                    once,
                    socket,
                    interval_seconds,
                    discovery_interval_seconds,
                )
                .await
            }
        }
        Action::Status => {
            println!("registry: {}", registry.path().display());
            if let Some(discovery) = registry.metadata("native_discovery.status")? {
                println!("native discovery: {discovery}");
            }
            #[cfg(target_os = "macos")]
            match daemon_launch_agent_path() {
                Ok(path) if path.exists() => println!("daemon: installed ({})", path.display()),
                _ => println!("daemon: not installed; run `taskrail daemon --install` on macOS"),
            }
            #[cfg(target_os = "linux")]
            match systemd_user_unit_path() {
                Ok(path) if path.exists() => println!("daemon: installed ({})", path.display()),
                _ => println!("daemon: not installed; run `taskrail daemon --install` on Linux"),
            }
            #[cfg(all(not(target_os = "macos"), not(target_os = "linux")))]
            println!(
                "daemon: not installed; run `taskrail daemon --install` on a supported platform"
            );
            Ok(())
        }
        Action::Integrations => integrations(&registry),
        Action::Integration { action } => integration_doctor(&registry, action).await,
        Action::Mcp { .. } | Action::McpFleet { .. } | Action::McpHttp { .. } => {
            unreachable!("MCP transports return before opening the local Registry")
        }
    }
}

#[derive(Debug, serde::Serialize)]
struct IntegrationCheck {
    name: String,
    ok: bool,
    detail: String,
}

#[derive(Debug, serde::Serialize)]
struct IntegrationReport {
    integration: String,
    status: String,
    checks: Vec<IntegrationCheck>,
}

async fn integration_doctor(registry: &Registry, action: IntegrationAction) -> Result<()> {
    let report = match action {
        IntegrationAction::Mole { action } => return run_mole(registry, action).await,
        IntegrationAction::Restic { action } => return run_restic(registry, action).await,
        IntegrationAction::Rclone { action } => return run_rclone(registry, action).await,
        IntegrationAction::Github { action } => return run_github(registry, action).await,
        IntegrationAction::Homebrew { action } => return run_homebrew(registry, action).await,
        IntegrationAction::Mas { action } => return run_mas(registry, action).await,
        IntegrationAction::OsvScanner { action } => {
            return run_scanner(registry, SecurityIntegration::osv(), action).await;
        }
        IntegrationAction::Gitleaks { action } => {
            return run_scanner(registry, SecurityIntegration::gitleaks(), action).await;
        }
        IntegrationAction::Trivy { action } => {
            return run_scanner(registry, SecurityIntegration::trivy(), action).await;
        }
        IntegrationAction::Topgrade { action } => return run_topgrade(registry, action).await,
        IntegrationAction::CodexDoctor { cwd } => codex_doctor(&cwd),
        IntegrationAction::GhDoctor { hostname } => gh_doctor(&hostname),
        IntegrationAction::ChatgptDoctor { profile } => chatgpt_doctor(&profile),
        IntegrationAction::ChatgptConnect {
            tunnel_id,
            alias,
            profile,
        } => return chatgpt_connect(tunnel_id, &alias, &profile),
    }?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

async fn run_mole(registry: &Registry, action: MoleAction) -> Result<()> {
    let integration = MoleIntegration::default();
    let action = match action {
        MoleAction::Detect => {
            println!("{}", serde_json::to_string_pretty(&integration.detect())?);
            return Ok(());
        }
        MoleAction::Doctor => {
            println!("{}", serde_json::to_string_pretty(&integration.doctor())?);
            return Ok(());
        }
        MoleAction::Version => SemanticIntegrationAction::new("version")?,
        MoleAction::Analyze => SemanticIntegrationAction::new("analyze")?,
        MoleAction::Status => SemanticIntegrationAction::new("status")?,
        MoleAction::History { limit } => SemanticIntegrationAction::with_parameters(
            "history",
            serde_json::json!({"limit": limit.clamp(1, 200)}),
        )?,
        MoleAction::Clean { dry_run } => SemanticIntegrationAction::with_parameters(
            "clean",
            serde_json::json!({"dry_run": dry_run}),
        )?,
    };
    run_semantic_integration(registry, &integration, &action).await
}

async fn run_restic(registry: &Registry, action: ResticAction) -> Result<()> {
    let integration = ResticIntegration::default();
    let action = match action {
        ResticAction::Detect => {
            println!("{}", serde_json::to_string_pretty(&integration.detect())?);
            return Ok(());
        }
        ResticAction::Doctor => {
            println!("{}", serde_json::to_string_pretty(&integration.doctor())?);
            return Ok(());
        }
        ResticAction::Snapshots => SemanticIntegrationAction::new("snapshots")?,
        ResticAction::Backup {
            path,
            repository_env,
            password_env,
        } => SemanticIntegrationAction::with_parameters(
            "backup",
            serde_json::json!({
                "path": path,
                "repository_env": repository_env,
                "password_env": password_env,
            }),
        )?,
        ResticAction::Check => SemanticIntegrationAction::new("check")?,
        ResticAction::Forget => SemanticIntegrationAction::new("forget")?,
        ResticAction::Prune => SemanticIntegrationAction::new("prune")?,
    };
    run_semantic_integration(registry, &integration, &action).await
}

async fn run_rclone(registry: &Registry, action: RcloneAction) -> Result<()> {
    let integration = RcloneIntegration::default();
    let action = match action {
        RcloneAction::Detect => {
            println!("{}", serde_json::to_string_pretty(&integration.detect())?);
            return Ok(());
        }
        RcloneAction::Doctor => {
            println!("{}", serde_json::to_string_pretty(&integration.doctor())?);
            return Ok(());
        }
        RcloneAction::ListRemotes => SemanticIntegrationAction::new("list-remotes")?,
        RcloneAction::Check {
            source,
            destination,
        } => SemanticIntegrationAction::with_parameters(
            "check",
            serde_json::json!({"source": source, "destination": destination}),
        )?,
        RcloneAction::Copy {
            source,
            destination,
        } => SemanticIntegrationAction::with_parameters(
            "copy",
            serde_json::json!({"source": source, "destination": destination}),
        )?,
        RcloneAction::Sync {
            source,
            destination,
            dry_run,
        } => SemanticIntegrationAction::with_parameters(
            "sync",
            serde_json::json!({"source": source, "destination": destination, "dry_run": dry_run}),
        )?,
    };
    run_semantic_integration(registry, &integration, &action).await
}

async fn run_github(registry: &Registry, action: GithubAction) -> Result<()> {
    let integration = GithubIntegration::default();
    let action = match action {
        GithubAction::Detect => {
            println!("{}", serde_json::to_string_pretty(&integration.detect())?);
            return Ok(());
        }
        GithubAction::Doctor => {
            println!("{}", serde_json::to_string_pretty(&integration.doctor())?);
            return Ok(());
        }
        GithubAction::Issues { repo } => {
            SemanticIntegrationAction::with_parameters("issues", serde_json::json!({"repo": repo}))?
        }
        GithubAction::Pulls { repo } => {
            SemanticIntegrationAction::with_parameters("pulls", serde_json::json!({"repo": repo}))?
        }
        GithubAction::FailedRuns { repo } => SemanticIntegrationAction::with_parameters(
            "failed-runs",
            serde_json::json!({"repo": repo}),
        )?,
        GithubAction::Checks { repo, pull_number } => SemanticIntegrationAction::with_parameters(
            "checks",
            serde_json::json!({"repo": repo, "pull_number": pull_number}),
        )?,
    };
    run_semantic_integration(registry, &integration, &action).await
}

async fn run_homebrew(registry: &Registry, action: HomebrewAction) -> Result<()> {
    let integration = HomebrewIntegration::default();
    let action = match action {
        HomebrewAction::Detect => {
            println!("{}", serde_json::to_string_pretty(&integration.detect())?);
            return Ok(());
        }
        HomebrewAction::Doctor => {
            println!("{}", serde_json::to_string_pretty(&integration.doctor())?);
            return Ok(());
        }
        HomebrewAction::Outdated => SemanticIntegrationAction::new("outdated")?,
        HomebrewAction::BundleCheck { file } => SemanticIntegrationAction::with_parameters(
            "bundle-check",
            serde_json::json!({"file": file}),
        )?,
        HomebrewAction::Upgrade { dry_run } => SemanticIntegrationAction::with_parameters(
            "upgrade",
            serde_json::json!({"dry_run": dry_run}),
        )?,
        HomebrewAction::Cleanup { dry_run } => SemanticIntegrationAction::with_parameters(
            "cleanup",
            serde_json::json!({"dry_run": dry_run}),
        )?,
    };
    run_semantic_integration(registry, &integration, &action).await
}

async fn run_mas(registry: &Registry, action: MasAction) -> Result<()> {
    let integration = MasIntegration::default();
    let action = match action {
        MasAction::Detect => {
            println!("{}", serde_json::to_string_pretty(&integration.detect())?);
            return Ok(());
        }
        MasAction::Doctor => {
            println!("{}", serde_json::to_string_pretty(&integration.doctor())?);
            return Ok(());
        }
        MasAction::List => SemanticIntegrationAction::new("list")?,
        MasAction::Outdated => SemanticIntegrationAction::new("outdated")?,
    };
    run_semantic_integration(registry, &integration, &action).await
}

async fn run_scanner(
    registry: &Registry,
    integration: impl Integration,
    action: ScannerAction,
) -> Result<()> {
    let action = match action {
        ScannerAction::Detect => {
            println!("{}", serde_json::to_string_pretty(&integration.detect())?);
            return Ok(());
        }
        ScannerAction::Doctor => {
            println!("{}", serde_json::to_string_pretty(&integration.doctor())?);
            return Ok(());
        }
        ScannerAction::Scan {
            path,
            baseline,
            scan_type,
        } => SemanticIntegrationAction::with_parameters(
            "scan",
            serde_json::json!({
                "path": path,
                "baseline": baseline,
                "scan_type": scan_type,
            }),
        )?,
    };
    run_semantic_integration(registry, &integration, &action).await
}

async fn run_topgrade(registry: &Registry, action: TopgradeAction) -> Result<()> {
    let integration = TopgradeIntegration::default();
    let action = match action {
        TopgradeAction::Detect => {
            println!("{}", serde_json::to_string_pretty(&integration.detect())?);
            return Ok(());
        }
        TopgradeAction::Doctor => {
            println!("{}", serde_json::to_string_pretty(&integration.doctor())?);
            return Ok(());
        }
        TopgradeAction::Inspect => SemanticIntegrationAction::new("inspect")?,
        TopgradeAction::Plan => SemanticIntegrationAction::new("plan")?,
        TopgradeAction::Run => SemanticIntegrationAction::new("run")?,
    };
    run_semantic_integration(registry, &integration, &action).await
}

async fn run_semantic_integration(
    registry: &Registry,
    integration: &dyn Integration,
    action: &SemanticIntegrationAction,
) -> Result<()> {
    let execution = service::execute_integration(registry.path(), integration, action).await?;
    println!(
        "{}",
        serde_json::to_string_pretty(&execution.semantic_value())?
    );
    if execution.verification.status == taskrail::integrations::VerificationStatus::Failed {
        anyhow::bail!("{} verification failed", action.action);
    }
    Ok(())
}

fn codex_doctor(cwd: &Path) -> Result<IntegrationReport> {
    let (available, version, detail) = probe_version("codex");
    let cwd = cwd
        .canonicalize()
        .with_context(|| format!("resolve integration cwd {}", cwd.display()))?;
    let repository = ProcessCommand::new("git")
        .arg("-C")
        .arg(&cwd)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .map(|output| {
            output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "true"
        })
        .unwrap_or(false);
    let mut checks = vec![IntegrationCheck {
        name: "codex_cli".into(),
        ok: available,
        detail: version.unwrap_or(detail),
    }];
    checks.push(IntegrationCheck {
        name: "git_repository".into(),
        ok: repository,
        detail: if repository {
            cwd.display().to_string()
        } else {
            format!("{} is not a Git worktree", cwd.display())
        },
    });
    Ok(IntegrationReport {
        integration: "codex".into(),
        status: if !available {
            "unavailable"
        } else if !repository {
            "needs_configuration"
        } else {
            "ready"
        }
        .into(),
        checks,
    })
}

fn gh_doctor(hostname: &str) -> Result<IntegrationReport> {
    let (available, version, detail) = probe_version("gh");
    let authenticated = if available {
        ProcessCommand::new("gh")
            .args(["auth", "status", "--hostname", hostname])
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    } else {
        false
    };
    let mut checks = vec![IntegrationCheck {
        name: "gh_cli".into(),
        ok: available,
        detail: version.unwrap_or(detail),
    }];
    checks.push(IntegrationCheck {
        name: "authentication".into(),
        ok: authenticated,
        detail: if authenticated {
            format!("authenticated for {hostname}")
        } else {
            format!("not authenticated for {hostname}")
        },
    });
    Ok(IntegrationReport {
        integration: "gh".into(),
        status: if !available {
            "unavailable"
        } else if !authenticated {
            "needs_configuration"
        } else {
            "ready"
        }
        .into(),
        checks,
    })
}

fn chatgpt_doctor(profile: &str) -> Result<IntegrationReport> {
    let (tunnel_available, version, detail) = probe_version("tunnel-client");
    let managed_runtime = tunnel_runtime_status(profile);
    let daemon_socket = default_socket_path();
    #[cfg(unix)]
    let daemon_connected = std::os::unix::net::UnixStream::connect(&daemon_socket).is_ok();
    #[cfg(not(unix))]
    let daemon_connected = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&daemon_socket)
        .is_ok();
    let mcp_available = ProcessCommand::new("taskrail")
        .args(["mcp", "--help"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    let tunnel_id_present =
        std::env::var_os("CONTROL_PLANE_TUNNEL_ID").is_some() || managed_runtime.tunnel_id_present;
    let runtime_key_present = configured_runtime_key().is_some() || managed_runtime.ready;
    let all_local = daemon_connected && mcp_available && tunnel_available;
    let fully_configured = all_local && tunnel_id_present && runtime_key_present;

    let checks = vec![
        IntegrationCheck {
            name: "taskrail_daemon".into(),
            ok: daemon_connected,
            detail: if daemon_connected {
                format!("connected at {}", daemon_socket.display())
            } else {
                format!(
                    "not connected at {}; start `taskrail daemon`",
                    daemon_socket.display()
                )
            },
        },
        IntegrationCheck {
            name: "mcp_adapter".into(),
            ok: mcp_available,
            detail: if mcp_available {
                "`taskrail mcp` is available".into()
            } else {
                "`taskrail mcp` is unavailable".into()
            },
        },
        IntegrationCheck {
            name: "tunnel_client".into(),
            ok: tunnel_available,
            detail: version.unwrap_or(detail),
        },
        IntegrationCheck {
            name: "tunnel_id".into(),
            ok: tunnel_id_present,
            detail: if tunnel_id_present {
                if managed_runtime.tunnel_id_present
                    && std::env::var_os("CONTROL_PLANE_TUNNEL_ID").is_none()
                {
                    "present in the managed runtime (value hidden)".into()
                } else {
                    "present (value hidden)".into()
                }
            } else {
                "absent; create or select a Tunnel in OpenAI Platform".into()
            },
        },
        IntegrationCheck {
            name: "runtime_key".into(),
            ok: runtime_key_present,
            detail: if runtime_key_present {
                if managed_runtime.ready && configured_runtime_key().is_none() {
                    "present in the ready managed runtime (value hidden)".into()
                } else {
                    "present (value hidden)".into()
                }
            } else {
                "absent; create a runtime key and set CONTROL_PLANE_API_KEY".into()
            },
        },
        IntegrationCheck {
            name: "tunnel_inputs".into(),
            ok: fully_configured,
            detail: if fully_configured {
                if managed_runtime.ready {
                    format!("managed tunnel runtime {profile} is ready")
                } else {
                    format!(
                        "Tunnel ID and runtime key are present; connect profile {profile} with `taskrail integration chatgpt-connect`"
                    )
                }
            } else {
                format!(
                    "profile {profile} cannot be connected until the Tunnel ID and runtime key are configured"
                )
            },
        },
    ];
    Ok(IntegrationReport {
        integration: "chatgpt".into(),
        status: if !tunnel_available {
            "unavailable"
        } else if !fully_configured {
            "needs_configuration"
        } else if !managed_runtime.ready {
            "ready_for_tunnel_connect"
        } else {
            "ready"
        }
        .into(),
        checks,
    })
}

#[derive(Debug, Default)]
struct TunnelRuntimeStatus {
    ready: bool,
    tunnel_id_present: bool,
}

fn tunnel_runtime_status(profile: &str) -> TunnelRuntimeStatus {
    let output = match ProcessCommand::new("tunnel-client")
        .args(["runtimes", "status", profile, "--json"])
        .output()
    {
        Ok(output) if output.status.success() => output,
        _ => return TunnelRuntimeStatus::default(),
    };
    let value: serde_json::Value = match serde_json::from_slice(&output.stdout) {
        Ok(value) => value,
        Err(_) => return TunnelRuntimeStatus::default(),
    };
    TunnelRuntimeStatus {
        ready: value["ready"].as_bool() == Some(true)
            || value["runtime_state"].as_str() == Some("ready"),
        tunnel_id_present: value["tunnel_id"].as_str().is_some_and(|id| !id.is_empty()),
    }
}

fn chatgpt_connect(tunnel_id: Option<String>, alias: &str, profile: &str) -> Result<()> {
    let tunnel_id = tunnel_id
        .filter(|value| !value.trim().is_empty())
        .context("provide --tunnel-id or set CONTROL_PLANE_TUNNEL_ID")?;
    let (tunnel_available, _, tunnel_detail) = probe_version("tunnel-client");
    if !tunnel_available {
        anyhow::bail!("tunnel-client is unavailable: {tunnel_detail}");
    }
    #[cfg(unix)]
    if std::os::unix::net::UnixStream::connect(default_socket_path()).is_err() {
        anyhow::bail!(
            "Taskrail daemon is not reachable at {}; start `taskrail daemon` first",
            default_socket_path().display()
        );
    }
    #[cfg(windows)]
    if std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(default_socket_path())
        .is_err()
    {
        anyhow::bail!(
            "Taskrail daemon is not reachable at {}; start `taskrail daemon` first",
            default_socket_path().display()
        );
    }
    let runtime_key = configured_runtime_key().context(
        "CONTROL_PLANE_API_KEY is absent; create a runtime key and set it only in the local environment or launchd user environment",
    )?;
    let socket = quote_command_argument(&default_socket_path().to_string_lossy());
    let mcp_command = format!("taskrail mcp --socket {socket}");
    let output = ProcessCommand::new("tunnel-client")
        .args([
            "runtimes",
            "connect",
            "--alias",
            alias,
            "--profile",
            profile,
            "--tunnel-id",
            tunnel_id.trim(),
            "--runtime-api-key",
            "env:CONTROL_PLANE_API_KEY",
            "--mcp-command",
            &mcp_command,
            "--json",
        ])
        // The profile stores only the env-var reference. Supplying the value
        // to this short-lived child also lets a launchd user environment
        // survive a shell restart without writing the secret to disk/output.
        .env("CONTROL_PLANE_API_KEY", runtime_key)
        .output()
        .context("start tunnel-client runtime")?;
    if !output.status.success() {
        anyhow::bail!("tunnel-client runtime exited with {}", output.status);
    }
    println!("ChatGPT tunnel runtime {alias} started for Taskrail profile {profile}.");
    println!("verify with: tunnel-client runtimes status {alias} --json");
    Ok(())
}

fn configured_runtime_key() -> Option<String> {
    if let Some(value) = std::env::var_os("CONTROL_PLANE_API_KEY") {
        let value = value.to_string_lossy().into_owned();
        if !value.is_empty() {
            return Some(value);
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = ProcessCommand::new("launchctl")
            .args(["getenv", "CONTROL_PLANE_API_KEY"])
            .output()
            && output.status.success()
        {
            let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    None
}

fn quote_command_argument(value: &str) -> String {
    if value
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || "_./:@%+=,-".contains(character))
    {
        value.to_owned()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

fn probe_version(executable: &str) -> (bool, Option<String>, String) {
    match ProcessCommand::new(executable).arg("--version").output() {
        Ok(output) if output.status.success() => {
            let version = first_non_empty_line(&output.stdout)
                .or_else(|| first_non_empty_line(&output.stderr));
            (true, version, "version command succeeded".into())
        }
        Ok(output) => (
            false,
            None,
            format!("--version exited with {}", output.status),
        ),
        Err(error) => (false, None, error.to_string()),
    }
}

fn first_non_empty_line(bytes: &[u8]) -> Option<String> {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_owned)
}

fn register(registry: &Registry, file: &PathBuf) -> Result<()> {
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("read automation definition {}", file.display()))?;
    let mut automation: Automation = serde_yaml::from_str(&content)
        .with_context(|| format!("parse automation YAML {}", file.display()))?;
    if automation.id.trim().is_empty() || automation.name.trim().is_empty() {
        anyhow::bail!("automation id and name are required");
    }
    if automation.steps.is_empty() {
        anyhow::bail!("automation must contain at least one step");
    }
    for step in &automation.steps {
        if step.command.shell || step.command.invokes_shell() {
            anyhow::bail!(
                "step {} requests shell execution; Taskrail requires direct argv",
                step.id
            );
        }
        if let Some(integration) = &step.integration {
            taskrail::core::IntegrationSpec::new(
                integration.integration.clone(),
                integration.action.clone(),
                integration.parameters.clone(),
            )
            .with_context(|| format!("validate integration step {}", step.id))?;
        }
    }
    automation.ownership = Ownership::Managed;
    automation.runtime_state = RuntimeState::Enabled;
    automation.source_id = None;
    automation.fingerprint = None;
    registry.save_automation(&automation)?;
    println!("registered managed automation {}", automation.name);
    Ok(())
}

fn add(registry: &Registry, options: AddOptions) -> Result<()> {
    let AddOptions {
        id,
        executable,
        name,
        args,
        every_seconds,
        cron,
        timezone,
        cwd,
    } = options;
    if id.trim().is_empty() {
        anyhow::bail!("automation id must not be empty");
    }
    if every_seconds.is_some_and(|seconds| seconds == 0) {
        anyhow::bail!("--every-seconds must be greater than zero");
    }
    let command = CommandSpec {
        executable,
        args,
        cwd,
        ..CommandSpec::default()
    };
    if command.shell || command.invokes_shell() {
        anyhow::bail!(
            "direct argv only: shell executables with -c/-e command strings are not accepted"
        );
    }
    let trigger = match (every_seconds, cron) {
        (Some(seconds), None) => Trigger::Interval { seconds },
        (None, Some(expression)) => Trigger::Cron {
            expression,
            timezone,
        },
        (None, None) => Trigger::Manual,
        (Some(_), Some(_)) => unreachable!("clap enforces trigger conflicts"),
    };
    let next_run_at = taskrail::scheduler::next_run(&trigger, Utc::now())?;
    let automation = Automation {
        id: id.clone(),
        name: name.unwrap_or_else(|| id.clone()),
        ownership: Ownership::Managed,
        runtime_state: RuntimeState::Enabled,
        trigger,
        next_run_at,
        steps: vec![StepSpec {
            id: "command".into(),
            command,
            responses: None,
            integration: None,
        }],
        ..Automation::default()
    };
    registry.save_automation(&automation)?;
    println!("added automation {}", automation.name);
    Ok(())
}

fn schedule_integration(registry: &Registry, options: ScheduleIntegrationOptions) -> Result<()> {
    let ScheduleIntegrationOptions {
        id,
        integration: integration_name,
        action: action_name,
        parameters,
        name,
        every_seconds,
        cron,
        timezone,
    } = options;
    let parameters: serde_json::Value =
        serde_json::from_str(&parameters).context("--parameters must be a JSON object")?;
    if !parameters.is_object() {
        anyhow::bail!("--parameters must be a JSON object");
    }
    let trigger = match (every_seconds, cron) {
        (Some(seconds), None) if seconds > 0 => Trigger::Interval { seconds },
        (Some(_), None) => anyhow::bail!("--every-seconds must be greater than zero"),
        (None, Some(expression)) => Trigger::Cron {
            expression,
            timezone,
        },
        (None, None) => Trigger::Manual,
        (Some(_), Some(_)) => unreachable!("clap enforces trigger conflicts"),
    };
    let integrations = taskrail::integrations::built_in_registry()?;
    let integration_id = taskrail::integrations::IntegrationId::new(integration_name.clone())?;
    let integration = integrations
        .get(&integration_id)
        .with_context(|| format!("integration not registered: {integration_name}"))?;
    let action = SemanticIntegrationAction::with_parameters(action_name, parameters)?;
    let automation = service::create_integration_automation(
        registry.path(),
        integration.as_ref(),
        &action,
        id,
        name,
        trigger,
    )?;
    println!("{}", serde_json::to_string_pretty(&automation)?);
    Ok(())
}

#[cfg(target_os = "macos")]
fn daemon_launch_agent_path() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").context("HOME is required for the user daemon")?;
    Ok(PathBuf::from(home).join("Library/LaunchAgents/com.taskrail.daemon.plist"))
}

#[cfg(target_os = "macos")]
fn launchctl_user_domain() -> Result<String> {
    let output = ProcessCommand::new("id")
        .arg("-u")
        .output()
        .context("resolve current user id")?;
    if !output.status.success() {
        anyhow::bail!(
            "id -u failed: {}",
            first_non_empty_line(&output.stderr).unwrap_or_default()
        );
    }
    let uid = first_non_empty_line(&output.stdout).context("id -u returned no user id")?;
    Ok(format!("gui/{uid}"))
}

fn install_daemon(registry: &Registry, discovery_interval_seconds: u64) -> Result<()> {
    if discovery_interval_seconds == 0 {
        anyhow::bail!("native discovery interval must be greater than zero");
    }
    #[cfg(all(
        not(target_os = "macos"),
        not(target_os = "linux"),
        not(target_os = "windows")
    ))]
    {
        let _ = registry;
        anyhow::bail!(
            "daemon installation currently supports ARM64 macOS LaunchAgents and ARM64 Linux user systemd"
        );
    }
    #[cfg(target_os = "macos")]
    {
        let path = daemon_launch_agent_path()?;
        let parent = path.parent().context("resolve LaunchAgents directory")?;
        std::fs::create_dir_all(parent)?;
        let executable = std::env::current_exe().context("resolve taskrail executable")?;
        let registry_path = registry.path().to_string_lossy().into_owned();
        let mut program_arguments = vec![
            plist::Value::String(executable.to_string_lossy().into_owned()),
            plist::Value::String("--registry".into()),
            plist::Value::String(registry_path),
            plist::Value::String("daemon".into()),
            plist::Value::String("--socket".into()),
            plist::Value::String(default_socket_path().to_string_lossy().into_owned()),
            plist::Value::String("--discovery-interval-seconds".into()),
            plist::Value::String(discovery_interval_seconds.to_string()),
        ];
        let mut plist = plist::Dictionary::new();
        plist.insert(
            "Label".into(),
            plist::Value::String("com.taskrail.daemon".into()),
        );
        plist.insert(
            "ProgramArguments".into(),
            plist::Value::Array(std::mem::take(&mut program_arguments)),
        );
        let mut environment = plist::Dictionary::new();
        environment.insert(
            "PATH".into(),
            plist::Value::String(
                "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin".into(),
            ),
        );
        plist.insert(
            "EnvironmentVariables".into(),
            plist::Value::Dictionary(environment),
        );
        plist.insert("RunAtLoad".into(), plist::Value::Boolean(true));
        plist.insert("KeepAlive".into(), plist::Value::Boolean(true));
        plist.insert(
            "ProcessType".into(),
            plist::Value::String("Background".into()),
        );
        plist.insert("ThrottleInterval".into(), plist::Value::Integer(30.into()));
        plist::to_file_xml(&path, &plist::Value::Dictionary(plist))
            .with_context(|| format!("write daemon LaunchAgent {}", path.display()))?;
        let domain = launchctl_user_domain()?;
        let _ = ProcessCommand::new("launchctl")
            .args(["bootout", &domain, &path.to_string_lossy()])
            .output();
        let output = ProcessCommand::new("launchctl")
            .args(["bootstrap", &domain, &path.to_string_lossy()])
            .output()
            .context("bootstrap daemon LaunchAgent")?;
        if !output.status.success() {
            anyhow::bail!(
                "launchctl bootstrap failed: {}",
                first_non_empty_line(&output.stderr).unwrap_or_default()
            );
        }
        println!("daemon installed and loaded from {}", path.display());
        Ok(())
    }
    #[cfg(target_os = "linux")]
    {
        let path = systemd_user_unit_path()?;
        ensure_systemd_user_manager()?;
        let parent = path.parent().context("resolve systemd user directory")?;
        std::fs::create_dir_all(parent)?;
        let executable = std::env::current_exe().context("resolve taskrail executable")?;
        let unit = systemd_user_unit(
            &executable,
            registry.path(),
            &default_socket_path(),
            discovery_interval_seconds,
        );
        std::fs::write(&path, unit)
            .with_context(|| format!("write systemd user unit {}", path.display()))?;
        run_systemctl_user(["daemon-reload"])?;
        run_systemctl_user(["enable", "--now", "taskrail.service"])?;
        println!("daemon installed and started from {}", path.display());
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        let executable = std::env::current_exe().context("resolve taskrail executable")?;
        let task_name = "Taskrail\\Daemon";
        let task_command = format!(
            "{} --registry {} daemon --socket {}",
            windows_quote_argument(&executable.to_string_lossy()),
            windows_quote_argument(&registry.path().to_string_lossy()),
            windows_quote_argument(&default_socket_path().to_string_lossy()),
        );
        let output = ProcessCommand::new("schtasks.exe")
            .args([
                "/Create",
                "/TN",
                task_name,
                "/TR",
                &task_command,
                "/SC",
                "ONLOGON",
                "/RL",
                "LIMITED",
                "/F",
            ])
            .output()
            .context("create Taskrail Windows Task Scheduler task")?;
        if !output.status.success() {
            anyhow::bail!(
                "schtasks.exe /Create failed: {}",
                first_non_empty_line(&output.stderr).unwrap_or_default()
            );
        }
        let run = ProcessCommand::new("schtasks.exe")
            .args(["/Run", "/TN", task_name])
            .output()
            .context("start Taskrail Windows Task Scheduler task")?;
        if !run.status.success() {
            anyhow::bail!(
                "schtasks.exe /Run failed: {}",
                first_non_empty_line(&run.stderr).unwrap_or_default()
            );
        }
        println!("daemon installed and started as Windows task {task_name}");
        Ok(())
    }
}

fn uninstall_daemon() -> Result<()> {
    #[cfg(all(
        not(target_os = "macos"),
        not(target_os = "linux"),
        not(target_os = "windows")
    ))]
    {
        anyhow::bail!(
            "daemon installation currently supports ARM64 macOS LaunchAgents and ARM64 Linux user systemd"
        );
    }
    #[cfg(target_os = "macos")]
    {
        let path = daemon_launch_agent_path()?;
        if !path.exists() {
            println!("daemon is not installed");
            return Ok(());
        }
        let domain = launchctl_user_domain()?;
        let output = ProcessCommand::new("launchctl")
            .args(["bootout", &domain, &path.to_string_lossy()])
            .output()
            .context("unload daemon LaunchAgent")?;
        if !output.status.success()
            && !String::from_utf8_lossy(&output.stderr).contains("Could not find service")
        {
            anyhow::bail!(
                "launchctl bootout failed: {}",
                first_non_empty_line(&output.stderr).unwrap_or_default()
            );
        }
        std::fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))?;
        println!("daemon uninstalled");
        Ok(())
    }
    #[cfg(target_os = "linux")]
    {
        let path = systemd_user_unit_path()?;
        if !path.exists() {
            println!("daemon is not installed");
            return Ok(());
        }
        run_systemctl_user(["disable", "--now", "taskrail.service"])?;
        std::fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))?;
        run_systemctl_user(["daemon-reload"])?;
        println!("daemon uninstalled");
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        let task_name = "Taskrail\\Daemon";
        let output = ProcessCommand::new("schtasks.exe")
            .args(["/Delete", "/TN", task_name, "/F"])
            .output()
            .context("remove Taskrail Windows Task Scheduler task")?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !output.status.success() && !stderr.to_ascii_lowercase().contains("cannot find") {
            anyhow::bail!(
                "schtasks.exe /Delete failed: {}",
                first_non_empty_line(&output.stderr).unwrap_or_default()
            );
        }
        println!("daemon uninstalled");
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn systemd_user_unit_path() -> Result<PathBuf> {
    Ok(xdg_home("XDG_CONFIG_HOME", ".config").join("systemd/user/taskrail.service"))
}

#[cfg(target_os = "linux")]
fn systemd_user_unit(
    executable: &Path,
    registry: &Path,
    socket: &Path,
    discovery_interval_seconds: u64,
) -> String {
    format!(
        "[Unit]\nDescription=Taskrail local automation daemon\nAfter=default.target\n\n[Service]\nExecStart={} --registry {} daemon --socket {} --discovery-interval-seconds {}\nRuntimeDirectory=taskrail\nRuntimeDirectoryMode=0700\nUMask=0077\nRestart=on-failure\nRestartSec=5\n\n[Install]\nWantedBy=default.target\n",
        systemd_quote(executable),
        systemd_quote(registry),
        systemd_quote(socket),
        discovery_interval_seconds,
    )
}

#[cfg(target_os = "linux")]
fn systemd_quote(path: &Path) -> String {
    let value = path.to_string_lossy();
    if value
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || "_./:@%+=,-".contains(character))
    {
        return value.replace('%', "%%");
    }
    format!(
        "\"{}\"",
        value
            .replace('%', "%%")
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
    )
}

#[cfg(target_os = "linux")]
fn run_systemctl_user<const N: usize>(args: [&str; N]) -> Result<()> {
    let output = ProcessCommand::new("systemctl")
        .arg("--user")
        .args(args)
        .output()
        .context("run systemctl --user")?;
    if !output.status.success() {
        anyhow::bail!(
            "systemctl --user {} failed: {}",
            args.join(" "),
            first_non_empty_line(&output.stderr).unwrap_or_default()
        );
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn ensure_systemd_user_manager() -> Result<()> {
    let output = ProcessCommand::new("systemctl")
        .args(["--user", "show-environment"])
        .output()
        .context("check systemd user manager")?;
    if !output.status.success() {
        let detail = first_non_empty_line(&output.stderr).unwrap_or_default();
        anyhow::bail!(
            "systemd user services are unavailable{}; start a user session or enable lingering with `loginctl enable-linger $USER`",
            if detail.is_empty() {
                String::new()
            } else {
                format!(": {detail}")
            }
        );
    }
    Ok(())
}

fn default_registry_path() -> PathBuf {
    #[cfg(target_os = "linux")]
    {
        xdg_home("XDG_DATA_HOME", ".local/share").join("taskrail/registry.sqlite3")
    }
    #[cfg(target_os = "windows")]
    {
        let base = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| user_home().join("AppData/Local"));
        base.join("taskrail/registry.sqlite3")
    }
    #[cfg(all(not(target_os = "linux"), not(target_os = "windows")))]
    {
        user_home().join(".local/share/taskrail/registry.sqlite3")
    }
}

fn default_fleet_config_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        let base = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| user_home().join("AppData/Roaming"));
        return base.join("taskrail/fleet.yaml");
    }
    #[cfg(target_os = "linux")]
    if let Some(config_home) = absolute_env_path("XDG_CONFIG_HOME") {
        return config_home.join("taskrail/fleet.yaml");
    }
    user_home().join(".config/taskrail/fleet.yaml")
}

fn default_socket_path() -> PathBuf {
    #[cfg(target_os = "linux")]
    if let Some(runtime_dir) = absolute_env_path("XDG_RUNTIME_DIR") {
        return runtime_dir.join("taskrail/taskraild.sock");
    }
    #[cfg(target_os = "windows")]
    {
        PathBuf::from(format!(
            r"\\.\pipe\taskrail-{}",
            sanitize_pipe_component(
                &std::env::var("USERNAME")
                    .or_else(|_| std::env::var("USER"))
                    .unwrap_or_else(|_| "default".into())
            )
        ))
    }
    #[cfg(not(target_os = "windows"))]
    {
        user_home().join(".local/share/taskrail/taskraild.sock")
    }
}

#[cfg(target_os = "linux")]
fn xdg_home(variable: &str, fallback_suffix: &str) -> PathBuf {
    absolute_env_path(variable).unwrap_or_else(|| user_home().join(fallback_suffix))
}

#[cfg(target_os = "linux")]
fn absolute_env_path(variable: &str) -> Option<PathBuf> {
    let path = std::env::var_os(variable).map(PathBuf::from)?;
    (!path.as_os_str().is_empty() && path.is_absolute()).then_some(path)
}

fn user_home() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(target_os = "windows")]
fn sanitize_pipe_component(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    if sanitized.is_empty() {
        "default".into()
    } else {
        sanitized
    }
}

#[cfg(target_os = "windows")]
fn windows_quote_argument(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\\\""))
}

async fn codex_run(registry: &Registry, options: CodexCliOptions) -> Result<()> {
    let CodexCliOptions {
        cwd,
        prompt,
        prompt_file,
        sandbox,
        model,
        model_catalog_json,
        output_schema,
        worktree_dir,
        timeout_seconds,
    } = options;
    let prompt = match (prompt, prompt_file) {
        (Some(prompt), None) => prompt,
        (None, Some(path)) => std::fs::read_to_string(&path)
            .with_context(|| format!("read Codex prompt {}", path.display()))?,
        (None, None) => anyhow::bail!("provide exactly one of --prompt or --prompt-file"),
        (Some(_), Some(_)) => unreachable!("clap enforces prompt conflicts"),
    };
    let cwd = cwd
        .canonicalize()
        .with_context(|| format!("resolve Codex cwd {}", cwd.display()))?;
    let codex_sandbox = match sandbox {
        CodexSandboxArg::ReadOnly => CodexSandbox::ReadOnly,
        CodexSandboxArg::WorkspaceWrite => CodexSandbox::WorkspaceWrite,
    };
    if matches!(codex_sandbox, CodexSandbox::WorkspaceWrite) && worktree_dir.is_none() {
        anyhow::bail!(
            "workspace-write Codex runs require --worktree-dir; the main checkout is not an approved write target"
        );
    }
    let request = CodexRequest {
        cwd: cwd.clone(),
        prompt,
        sandbox: codex_sandbox,
        model,
        model_catalog_json,
        output_schema,
        add_dirs: Vec::new(),
        timeout_seconds,
    };
    let handle = match worktree_dir {
        Some(path) => Some(worktree::create(&cwd, path, None)?),
        None => None,
    };
    let mut request = request;
    if let Some(handle) = &handle {
        request.cwd = handle.path.clone();
    }
    let result = codex::execute(&request).await?;
    if let Some(usage) = result.summary.usage.as_ref() {
        for key in [
            "input_tokens",
            "cached_input_tokens",
            "output_tokens",
            "reasoning_output_tokens",
            "total_tokens",
        ] {
            if let Some(value) = usage.get(key).and_then(serde_json::Value::as_f64) {
                registry.record_metric(&Metric {
                    id: format!("metric_codex_{}", Uuid::new_v4()),
                    run_id: None,
                    key: key.into(),
                    value,
                    unit: "tokens".into(),
                    source: "codex.exec".into(),
                    recorded_at: Utc::now(),
                })?;
            }
        }
    }
    println!("{}", serde_json::to_string_pretty(&result)?);
    if let Some(handle) = handle {
        eprintln!(
            "worktree retained for inspection: {}",
            handle.path.display()
        );
    }
    if result.status == "succeeded" {
        Ok(())
    } else {
        anyhow::bail!("Codex run {}", result.status)
    }
}

async fn responses_run(registry: &Registry, options: ResponsesCliOptions) -> Result<()> {
    let ResponsesCliOptions {
        prompt,
        prompt_file,
        base_url,
        model,
        api_key_env,
        store,
        timeout_seconds,
        json,
    } = options;
    let prompt = match (prompt, prompt_file) {
        (Some(prompt), None) => prompt,
        (None, Some(path)) => std::fs::read_to_string(&path)
            .with_context(|| format!("read Responses prompt {}", path.display()))?,
        (None, None) => anyhow::bail!("provide exactly one of --prompt or --prompt-file"),
        (Some(_), Some(_)) => unreachable!("clap enforces prompt conflicts"),
    };
    let config = taskrail::responses::ResponsesConfig::from_env(
        base_url,
        model,
        &api_key_env,
        store,
        timeout_seconds,
    )?;
    let result = config.execute(prompt).await?;
    if let Some(usage) = &result.usage {
        for (key, value) in [
            ("input_tokens", usage.input_tokens),
            ("output_tokens", usage.output_tokens),
            ("total_tokens", usage.total_tokens),
        ] {
            if let Some(value) = value {
                registry.record_metric(&Metric {
                    id: format!("metric_responses_{}", Uuid::new_v4()),
                    run_id: None,
                    key: key.into(),
                    value: value as f64,
                    unit: "tokens".into(),
                    source: "responses.api".into(),
                    recorded_at: Utc::now(),
                })?;
            }
        }
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!("{}", result.output_text);
    }
    Ok(())
}

fn inbox(registry: &Registry, limit: usize) -> Result<()> {
    if !(1..=500).contains(&limit) {
        anyhow::bail!("inbox limit must be between 1 and 500");
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&registry.list_inbox(limit)?)?
    );
    Ok(())
}

fn metrics(registry: &Registry) -> Result<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(&registry.list_metrics()?)?
    );
    Ok(())
}

fn events(registry: &Registry, limit: usize) -> Result<()> {
    if !(1..=500).contains(&limit) {
        anyhow::bail!("event limit must be between 1 and 500");
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&registry.list_events(limit)?)?
    );
    Ok(())
}

fn runs(registry: &Registry, automation_id: Option<&str>, limit: usize) -> Result<()> {
    if !(1..=500).contains(&limit) {
        anyhow::bail!("run limit must be between 1 and 500");
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&registry.list_runs(limit, automation_id)?)?
    );
    Ok(())
}

fn transition_runtime_state(registry: &Registry, id: &str, desired: RuntimeState) -> Result<()> {
    let automation = registry.transition_runtime_state(id, desired)?;
    println!("{}", serde_json::to_string_pretty(&automation)?);
    Ok(())
}

fn cancel_run(registry: &Registry, run_id: &str) -> Result<()> {
    service::cancel_run(registry.path(), run_id)?;
    println!(
        "{}",
        serde_json::json!({"run_id": run_id, "status": "cancel_requested"})
    );
    Ok(())
}

fn logs(registry: &Registry, run_id: &str) -> Result<()> {
    let logs = registry
        .get_run_logs(run_id)?
        .with_context(|| format!("run not found: {run_id}"))?;
    println!("{}", serde_json::to_string_pretty(&logs)?);
    Ok(())
}

fn acknowledge_drift(registry: &Registry, id: &str, dry_run: bool, apply: bool) -> Result<()> {
    if dry_run == apply {
        anyhow::bail!("choose exactly one of --dry-run or --apply");
    }
    let source = registry
        .list_sources()?
        .into_iter()
        .find(|source| source.source_id == id || source.native_id == id)
        .with_context(|| format!("source not found: {id}"))?;
    let automation = registry
        .get_automation(&source.source_id)?
        .with_context(|| format!("source has no Registry automation: {}", source.source_id))?;
    let expected = automation.fingerprint.clone().unwrap_or_default();
    if !matches!(
        automation.ownership,
        Ownership::Adopted | Ownership::Managed
    ) || expected == source.fingerprint
        || automation.runtime_state != RuntimeState::NeedsAttention
    {
        anyhow::bail!(
            "source {} has no owned drift requiring acknowledgement",
            source.source_id
        );
    }
    if dry_run {
        println!(
            "{}",
            serde_json::json!({
                "source_id": source.source_id,
                "expected_fingerprint": expected,
                "observed_fingerprint": source.fingerprint,
                "resulting_runtime_state": "paused",
                "action": "update baseline and require explicit resume",
            })
        );
        return Ok(());
    }
    let updated = registry.acknowledge_source_drift(id)?;
    println!("{}", serde_json::to_string_pretty(&updated)?);
    Ok(())
}

async fn verify(
    cwd: &PathBuf,
    executable: PathBuf,
    args: Vec<String>,
    expected_exit_code: i32,
    timeout_seconds: u64,
) -> Result<()> {
    let report = verification::run(
        cwd,
        vec![VerificationCommand {
            name: executable.to_string_lossy().into_owned(),
            command: taskrail::CommandSpec::argv(executable, args),
            expected_exit_code,
        }],
        timeout_seconds,
    )
    .await?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    if report.status == "pass" {
        Ok(())
    } else {
        anyhow::bail!("verification failed")
    }
}

async fn github_watch(
    registry: &Registry,
    repo: String,
    query: GithubQueryArg,
    pull_number: Option<u64>,
    timeout_seconds: u64,
    interval_seconds: Option<u64>,
) -> Result<()> {
    if interval_seconds.is_some_and(|value| value == 0) {
        anyhow::bail!("GitHub watch interval must be greater than zero");
    }
    let kind = match query {
        GithubQueryArg::Issues => QueryKind::Issues,
        GithubQueryArg::Pulls => QueryKind::Pulls,
        GithubQueryArg::FailedRuns => QueryKind::FailedRuns,
        GithubQueryArg::Checks => QueryKind::Checks,
    };
    let query = GhQuery {
        repo,
        kind,
        pull_number,
    };
    loop {
        let snapshot = github::execute(&query, timeout_seconds).await?;
        let observation = github::observe(&query, &snapshot)?;
        let previous = registry.record_github_snapshot(
            &observation.watch_key,
            &observation.repo,
            observation.kind.as_str(),
            observation.pull_number,
            &observation.fingerprint,
            &serde_json::to_value(&snapshot)?,
        )?;
        let changed = previous.as_deref() != Some(observation.fingerprint.as_str());
        if changed {
            registry.append_event(&Event {
                run_id: None,
                occurred_at: Utc::now(),
                event_type: "github.snapshot.changed".into(),
                payload: serde_json::json!({
                    "watch_key": observation.watch_key,
                    "repo": observation.repo,
                    "kind": observation.kind,
                    "pull_number": observation.pull_number,
                    "previous_fingerprint": previous,
                    "fingerprint": observation.fingerprint,
                    "item_count": observation.item_count,
                }),
            })?;
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "watch_key": observation.watch_key,
                "repo": observation.repo,
                "kind": observation.kind,
                "pull_number": observation.pull_number,
                "changed": changed,
                "previous_fingerprint": previous,
                "fingerprint": observation.fingerprint,
                "item_count": observation.item_count,
            }))?
        );
        let Some(interval_seconds) = interval_seconds else {
            return Ok(());
        };
        tokio::time::sleep(std::time::Duration::from_secs(interval_seconds)).await;
    }
}

fn worktree_action(action: WorktreeAction) -> Result<()> {
    match action {
        WorktreeAction::Create {
            repository,
            path,
            base,
        } => {
            let handle = worktree::create(repository, path, base.as_deref())?;
            println!(
                "{}",
                serde_json::json!({"repository": handle.repository, "path": handle.path})
            );
            Ok(())
        }
        WorktreeAction::Remove {
            repository,
            path,
            force,
        } => {
            worktree::remove(
                &worktree::WorktreeHandle {
                    repository,
                    path: path.clone(),
                },
                force,
            )?;
            println!("removed worktree {}", path.display());
            Ok(())
        }
    }
}

fn scan(registry: &Registry, source: SourceKind, json: bool) -> Result<()> {
    let source = match source {
        SourceKind::All => "all",
        SourceKind::Launchd => "launchd",
        SourceKind::Cron => "cron",
        SourceKind::Systemd => "systemd",
        SourceKind::Homebrew => "homebrew",
    };
    let discovered = scan_native_sources(source)?;
    for item in &discovered {
        registry.reconcile_discovered_source(item)?;
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&discovered)?);
    } else {
        println!("discovered {} native automation(s)", discovered.len());
        for item in discovered {
            println!(
                "{}\t{}\t{}\t{}",
                item.source_id,
                item.kind,
                if item.enabled { "enabled" } else { "disabled" },
                item.command
                    .as_ref()
                    .map_or("(no command)".into(), |c| c.display())
            );
        }
    }
    Ok(())
}

fn list(registry: &Registry, json: bool) -> Result<()> {
    let automations = registry.list_automations()?;
    if json {
        println!("{}", serde_json::to_string_pretty(&automations)?);
    } else {
        if automations.is_empty() {
            println!("no automations registered; run `taskrail scan`");
        }
        for automation in automations {
            println!(
                "{}\t{}\t{:?}\t{:?}",
                automation.id, automation.name, automation.ownership, automation.runtime_state
            );
        }
    }
    Ok(())
}

fn inspect(registry: &Registry, id: &str) -> Result<()> {
    if let Some(automation) = registry.get_automation(id)? {
        println!("{}", serde_json::to_string_pretty(&automation)?);
        return Ok(());
    }
    let source = registry
        .list_sources()?
        .into_iter()
        .find(|source| source.source_id == id || source.native_id == id);
    match source {
        Some(source) => println!("{}", serde_json::to_string_pretty(&source)?),
        None => anyhow::bail!("automation or source not found: {id}"),
    }
    Ok(())
}

fn delete(registry: &Registry, id: &str) -> Result<()> {
    let automation = registry.delete_automation(id)?;
    println!("deleted managed automation {}", automation.name);
    Ok(())
}

async fn run(registry: &Registry, id: &str, allow_observed: bool) -> Result<()> {
    let result = service::run_named(registry.path(), id, allow_observed).await?;
    print!("{}", result.stdout);
    eprint!("{}", result.stderr);
    if result.status == "succeeded" {
        Ok(())
    } else {
        anyhow::bail!("automation run {}", result.status)
    }
}

async fn daemon(
    registry: &Registry,
    once: bool,
    socket: Option<PathBuf>,
    interval_seconds: u64,
    discovery_interval_seconds: u64,
) -> Result<()> {
    if interval_seconds == 0 {
        anyhow::bail!("daemon interval must be greater than zero");
    }
    if discovery_interval_seconds == 0 {
        anyhow::bail!("native discovery interval must be greater than zero");
    }
    let recovered = service::recover_interrupted_runs(registry.path())?;
    if recovered > 0 {
        eprintln!("recovered {recovered} interrupted run(s) after daemon restart");
    }
    let mut server = if once {
        None
    } else {
        let socket = socket.unwrap_or_else(default_socket_path);
        let registry_path = registry.path().to_path_buf();
        Some(tokio::spawn(async move {
            rpc::serve(socket, registry_path).await
        }))
    };
    let mut next_discovery = std::time::Instant::now();
    loop {
        if let Some(server) = server.as_mut()
            && server.is_finished()
        {
            server.await??;
            anyhow::bail!("RPC server stopped unexpectedly");
        }
        if once || std::time::Instant::now() >= next_discovery {
            match service::native_discovery_pass(registry.path()) {
                Ok(summary) => eprintln!(
                    "native discovery: {} source(s), {} drifted, {} missing, {} unrunnable",
                    summary.source_count, summary.drifted, summary.missing, summary.unrunnable
                ),
                Err(error) => {
                    eprintln!("native discovery failed: {error:#}");
                    if let Err(record_error) =
                        service::record_native_discovery_failure(registry.path(), &error)
                    {
                        eprintln!("record native discovery failure: {record_error:#}");
                    }
                }
            }
            next_discovery = std::time::Instant::now()
                + std::time::Duration::from_secs(discovery_interval_seconds);
        }
        let pass = service::scheduled_pass(registry.path()).await?;
        println!(
            "scheduler pass: {} automation(s) due, {} failed",
            pass.due, pass.failed
        );
        if once {
            return Ok(());
        }
        let scheduler_sleep = std::time::Duration::from_secs(interval_seconds);
        let discovery_sleep = next_discovery.saturating_duration_since(std::time::Instant::now());
        tokio::time::sleep(scheduler_sleep.min(discovery_sleep)).await;
    }
}

fn adopt(registry: &Registry, id: &str, dry_run: bool, apply: bool) -> Result<()> {
    if dry_run == apply {
        anyhow::bail!("choose exactly one of --dry-run or --apply");
    }
    let report = adopt_source(registry, id, apply)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

fn rollback(registry: &Registry, tx_id: &str) -> Result<()> {
    rollback_source(registry, tx_id)?;
    println!("rolled back {tx_id}");
    Ok(())
}

fn adoptions(registry: &Registry, limit: usize) -> Result<()> {
    if !(1..=500).contains(&limit) {
        anyhow::bail!("adoption limit must be between 1 and 500");
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&registry.list_adoptions(limit)?)?
    );
    Ok(())
}

fn approvals(registry: &Registry, limit: usize) -> Result<()> {
    if !(1..=500).contains(&limit) {
        anyhow::bail!("approval limit must be between 1 and 500");
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&registry.list_approvals(limit)?)?
    );
    Ok(())
}

fn integrations(_registry: &Registry) -> Result<()> {
    let registry = taskrail::integrations::built_in_registry()?;
    println!("{}", serde_json::to_string_pretty(&registry.descriptors())?);
    Ok(())
}

async fn approval_execute(registry: &Registry, approval_id: &str) -> Result<()> {
    let execution = service::execute_approved_integration(registry.path(), approval_id).await?;
    println!(
        "{}",
        serde_json::to_string_pretty(&execution.semantic_value())?
    );
    Ok(())
}

fn approval_request(registry: &Registry, action: ApprovalAction, ttl_seconds: u64) -> Result<()> {
    let (integration, action) = match action {
        ApprovalAction::MoleClean { dry_run } => (
            Box::new(MoleIntegration::default()) as Box<dyn Integration>,
            SemanticIntegrationAction::with_parameters(
                "clean",
                serde_json::json!({"dry_run": dry_run}),
            )?,
        ),
        ApprovalAction::ResticBackup { path } => (
            Box::new(ResticIntegration::default()) as Box<dyn Integration>,
            SemanticIntegrationAction::with_parameters(
                "backup",
                serde_json::json!({"path": path}),
            )?,
        ),
        ApprovalAction::ResticForget => (
            Box::new(ResticIntegration::default()) as Box<dyn Integration>,
            SemanticIntegrationAction::new("forget")?,
        ),
        ApprovalAction::ResticPrune => (
            Box::new(ResticIntegration::default()) as Box<dyn Integration>,
            SemanticIntegrationAction::new("prune")?,
        ),
        ApprovalAction::RcloneCopy {
            source,
            destination,
        } => (
            Box::new(RcloneIntegration::default()) as Box<dyn Integration>,
            SemanticIntegrationAction::with_parameters(
                "copy",
                serde_json::json!({"source": source, "destination": destination}),
            )?,
        ),
        ApprovalAction::RcloneSync {
            source,
            destination,
        } => (
            Box::new(RcloneIntegration::default()) as Box<dyn Integration>,
            SemanticIntegrationAction::with_parameters(
                "sync",
                serde_json::json!({"source": source, "destination": destination, "dry_run": false}),
            )?,
        ),
        ApprovalAction::HomebrewUpgrade => (
            Box::new(HomebrewIntegration::default()) as Box<dyn Integration>,
            SemanticIntegrationAction::with_parameters(
                "upgrade",
                serde_json::json!({"dry_run": false}),
            )?,
        ),
        ApprovalAction::HomebrewCleanup => (
            Box::new(HomebrewIntegration::default()) as Box<dyn Integration>,
            SemanticIntegrationAction::with_parameters(
                "cleanup",
                serde_json::json!({"dry_run": false}),
            )?,
        ),
        ApprovalAction::TopgradeRun => (
            Box::new(TopgradeIntegration::default()) as Box<dyn Integration>,
            SemanticIntegrationAction::new("run")?,
        ),
    };
    let approval = service::request_integration_approval(
        registry.path(),
        integration.as_ref(),
        &action,
        ttl_seconds,
    )?;
    println!("{}", serde_json::to_string_pretty(&approval)?);
    Ok(())
}

fn approval_decide(registry: &Registry, id: &str, approve: bool, reject: bool) -> Result<()> {
    if approve == reject {
        anyhow::bail!("choose exactly one of --approve or --reject");
    }
    let decision = if approve { "approved" } else { "rejected" };
    let approval = registry.decide_approval(id, decision)?;
    registry.append_event(&Event {
        run_id: None,
        occurred_at: Utc::now(),
        event_type: format!("integration.approval.{decision}"),
        payload: serde_json::json!({"approval_id": id, "status": decision}),
    })?;
    println!("{}", serde_json::to_string_pretty(&approval)?);
    Ok(())
}

fn adoption_inspect(registry: &Registry, tx_id: &str) -> Result<()> {
    let record = registry
        .get_adoption(tx_id)?
        .with_context(|| format!("adoption transaction not found: {tx_id}"))?;
    println!("{}", serde_json::to_string_pretty(&record)?);
    Ok(())
}

fn doctor(registry: &Registry, check: Option<DoctorCheck>) -> Result<()> {
    let check = check.unwrap_or(DoctorCheck::Ownership);
    match check {
        DoctorCheck::Ownership => {
            let mut by_fingerprint = BTreeMap::<String, Vec<String>>::new();
            for automation in registry.list_automations()? {
                if let (Some(fingerprint), true) = (
                    automation.fingerprint,
                    matches!(
                        automation.ownership,
                        Ownership::Adopted | Ownership::Managed
                    ),
                ) {
                    by_fingerprint
                        .entry(fingerprint)
                        .or_default()
                        .push(automation.id);
                }
            }
            let duplicates = by_fingerprint
                .into_iter()
                .filter(|(_, ids)| ids.len() > 1)
                .collect::<Vec<_>>();
            if duplicates.is_empty() {
                println!("ownership: OK (no duplicate active owners)");
                Ok(())
            } else {
                println!("ownership: ERROR");
                for (fingerprint, ids) in duplicates {
                    println!("{fingerprint}: {}", ids.join(", "));
                }
                anyhow::bail!("duplicate active owners detected")
            }
        }
        DoctorCheck::Permissions => {
            println!(
                "permissions: argv-only subprocesses; system LaunchDaemons are observation-only"
            );
            Ok(())
        }
        DoctorCheck::Drift => doctor_drift(registry),
        DoctorCheck::Adoption => doctor_adoption(registry),
    }
}

fn doctor_adoption(registry: &Registry) -> Result<()> {
    let pending = registry
        .list_adoptions(500)?
        .into_iter()
        .filter(|adoption| !adoption.state.is_terminal())
        .collect::<Vec<_>>();
    if pending.is_empty() {
        println!("adoption: OK (no non-terminal transactions)");
        return Ok(());
    }
    println!("adoption: ERROR");
    for adoption in &pending {
        println!(
            "{}: source={} state={:?} step={}",
            adoption.tx_id, adoption.source_id, adoption.state, adoption.step
        );
    }
    anyhow::bail!(
        "{} non-terminal adoption transaction(s); inspect and explicitly rollback",
        pending.len()
    )
}

fn doctor_drift(registry: &Registry) -> Result<()> {
    let mut issues = Vec::new();
    for source in registry.list_sources()? {
        let Some(automation) = registry.get_automation(&source.source_id)? else {
            continue;
        };
        if automation.runtime_state == RuntimeState::NeedsAttention {
            issues.push(format!(
                "{}: runtime state is needs_attention",
                source.source_id
            ));
        }
        if matches!(
            automation.ownership,
            Ownership::Adopted | Ownership::Managed
        ) && automation.fingerprint.as_deref() != Some(source.fingerprint.as_str())
        {
            issues.push(format!(
                "{}: expected fingerprint {:?}, observed {}",
                source.source_id, automation.fingerprint, source.fingerprint
            ));
        }
    }
    if issues.is_empty() {
        println!("drift: OK");
        Ok(())
    } else {
        println!("drift: ERROR");
        for issue in &issues {
            println!("{issue}");
        }
        anyhow::bail!("{} drift issue(s) detected", issues.len())
    }
}

fn explain(registry: &Registry, id: &str) -> Result<()> {
    let automation = registry
        .get_automation(id)?
        .with_context(|| format!("automation not found: {id}"))?;
    println!(
        "Would execute automation {} (revision {})",
        automation.name, automation.revision
    );
    println!(
        "ownership: {:?}\ntrigger: {:?}",
        automation.ownership, automation.trigger
    );
    for (index, step) in automation.steps.iter().enumerate() {
        println!(
            "{}. {}\n   shell: {}",
            index + 1,
            step.command.display(),
            step.command.shell || step.command.invokes_shell()
        );
    }
    println!("\nNo commands have been executed.");
    Ok(())
}

fn dashboard(registry: &Registry) -> Result<()> {
    if io::stdin().is_terminal() && io::stdout().is_terminal() {
        return taskrail::tui::run(registry.path());
    }
    text_dashboard(registry)
}

fn text_dashboard(registry: &Registry) -> Result<()> {
    let automations = registry.list_automations()?;
    let inbox = registry.list_inbox(100)?;
    println!("Automation Control Plane");
    println!(
        "{} registered · {} observed · {} adopted/managed",
        automations.len(),
        automations
            .iter()
            .filter(|a| a.ownership == Ownership::Observed)
            .count(),
        automations
            .iter()
            .filter(|a| a.ownership != Ownership::Observed)
            .count()
    );
    println!("\nNAME\tOWNERSHIP\tSTATE\tNEXT RUN");
    for automation in automations {
        println!(
            "{}\t{:?}\t{:?}\t{}",
            automation.name,
            automation.ownership,
            automation.runtime_state,
            automation
                .next_run_at
                .map_or("manual".into(), |value| value.to_rfc3339())
        );
    }
    println!("\nINBOX\t{} item(s)", inbox.len());
    for item in inbox.iter().take(10) {
        println!(
            "{}\t{}\t{}\t{}",
            item.severity, item.kind, item.status, item.title
        );
    }
    Ok(())
}

#[cfg(test)]
mod platform_tests {
    #[cfg(target_os = "linux")]
    use std::path::Path;

    #[cfg(target_os = "linux")]
    use super::{systemd_quote, systemd_user_unit};

    #[cfg(target_os = "linux")]
    #[test]
    fn systemd_user_unit_is_private_and_uses_direct_argv() {
        let unit = systemd_user_unit(
            Path::new("/home/me/bin/taskrail"),
            Path::new("/home/me/.local/share/taskrail/registry.sqlite3"),
            Path::new("/run/user/1000/taskrail/taskraild.sock"),
            300,
        );
        assert!(unit.contains("ExecStart=/home/me/bin/taskrail --registry"));
        assert!(unit.contains("RuntimeDirectory=taskrail"));
        assert!(unit.contains("RuntimeDirectoryMode=0700"));
        assert!(unit.contains("UMask=0077"));
        assert!(unit.contains("Restart=on-failure"));
        assert!(unit.contains("--discovery-interval-seconds 300"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn systemd_quote_escapes_percent_and_spaces() {
        assert_eq!(
            systemd_quote(Path::new("/home/me/100% complete/taskrail")),
            "\"/home/me/100%% complete/taskrail\""
        );
        assert_eq!(
            systemd_quote(Path::new("/home/me/taskrail")),
            "/home/me/taskrail"
        );
    }
}
