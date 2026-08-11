use anyhow::{Context, Result};
use chrono::Utc;
use clap::{Parser, Subcommand, ValueEnum};
use std::{
    collections::BTreeMap,
    io::{self, IsTerminal},
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
};
use taskrail::{
    adoption::{
        AdoptionEngine, CronController, LaunchdController, LaunchdSnapshot, SystemdController,
        SystemdSnapshot, rollback_adoption,
    },
    codex::{self, CodexRequest, CodexSandbox},
    core::{Automation, CommandSpec, Event, Metric, Ownership, RuntimeState, StepSpec, Trigger},
    discovery::{
        CronProvider, DiscoveryProvider, HomebrewProvider, LaunchdProvider, SystemdProvider,
        merge_homebrew_sources, same_native_path,
    },
    github::{self, GhQuery, QueryKind},
    rpc, service,
    storage::Registry,
    verification::{self, VerificationCommand},
    worktree,
};
use uuid::Uuid;

#[derive(Debug, Parser)]
#[command(
    name = "taskrail",
    version,
    about = "A local automation manager for developers"
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
        /// Expose the local JSON-RPC control plane on this Unix socket.
        #[arg(long)]
        socket: Option<PathBuf>,
        #[arg(long, default_value_t = 30)]
        interval_seconds: u64,
        /// Install the user LaunchAgent that keeps the scheduler running.
        #[arg(long, conflicts_with = "uninstall")]
        install: bool,
        /// Remove the installed user LaunchAgent.
        #[arg(long, conflicts_with = "install")]
        uninstall: bool,
    },
    /// Print the Registry path and daemon boundary.
    Status,
    /// Diagnose local integration availability without running an automation.
    Integration {
        #[command(subcommand)]
        action: IntegrationAction,
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
enum IntegrationAction {
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
}

struct CodexCliOptions {
    cwd: PathBuf,
    prompt: Option<String>,
    prompt_file: Option<PathBuf>,
    sandbox: CodexSandboxArg,
    model: Option<String>,
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

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let registry = Registry::open(cli.registry.unwrap_or_else(default_registry_path))?;
    match cli.command {
        Action::Scan { source, json } => scan(&registry, source, json),
        Action::List { json } => list(&registry, json),
        Action::Inspect { id } => inspect(&registry, &id),
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
        Action::CodexRun {
            cwd,
            prompt,
            prompt_file,
            sandbox,
            model,
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
        } => {
            if install {
                install_daemon(&registry)
            } else if uninstall {
                uninstall_daemon()
            } else {
                daemon(&registry, once, socket, interval_seconds).await
            }
        }
        Action::Status => {
            println!("registry: {}", registry.path().display());
            match daemon_launch_agent_path() {
                Ok(path) if path.exists() => println!("daemon: installed ({})", path.display()),
                _ => println!("daemon: not installed; run `taskrail daemon --install` on macOS"),
            }
            Ok(())
        }
        Action::Integration { action } => integration_doctor(action),
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

fn integration_doctor(action: IntegrationAction) -> Result<()> {
    let report = match action {
        IntegrationAction::CodexDoctor { cwd } => codex_doctor(&cwd),
        IntegrationAction::GhDoctor { hostname } => gh_doctor(&hostname),
    }?;
    println!("{}", serde_json::to_string_pretty(&report)?);
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
            command: CommandSpec {
                executable,
                args,
                cwd,
                ..CommandSpec::default()
            },
            responses: None,
        }],
        ..Automation::default()
    };
    registry.save_automation(&automation)?;
    println!("added automation {}", automation.name);
    Ok(())
}

fn daemon_launch_agent_path() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").context("HOME is required for the user daemon")?;
    Ok(PathBuf::from(home).join("Library/LaunchAgents/com.taskrail.daemon.plist"))
}

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

fn install_daemon(registry: &Registry) -> Result<()> {
    #[cfg(not(target_os = "macos"))]
    {
        let _ = registry;
        anyhow::bail!("daemon installation currently supports macOS user LaunchAgents only");
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
}

fn uninstall_daemon() -> Result<()> {
    #[cfg(not(target_os = "macos"))]
    {
        anyhow::bail!("daemon installation currently supports macOS user LaunchAgents only");
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
}

fn default_registry_path() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".local/share/taskrail/registry.sqlite3")
}

async fn codex_run(registry: &Registry, options: CodexCliOptions) -> Result<()> {
    let CodexCliOptions {
        cwd,
        prompt,
        prompt_file,
        sandbox,
        model,
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
    let mut discovered = Vec::new();
    if matches!(source, SourceKind::All | SourceKind::Launchd) {
        discovered.extend(LaunchdProvider::default().scan()?);
    }
    if matches!(source, SourceKind::All | SourceKind::Cron) {
        discovered.extend(CronProvider::default().scan()?);
    }
    if matches!(source, SourceKind::All | SourceKind::Systemd) {
        discovered.extend(SystemdProvider::default().scan()?);
    }
    if matches!(source, SourceKind::All | SourceKind::Homebrew) {
        let homebrew = HomebrewProvider::default().scan()?;
        if matches!(source, SourceKind::All) {
            let unmatched = merge_homebrew_sources(&mut discovered, homebrew);
            discovered.extend(unmatched);
        } else {
            let mut launchd = LaunchdProvider::default().scan()?;
            let unmatched = merge_homebrew_sources(&mut launchd, homebrew.clone());
            let mut related = homebrew
                .iter()
                .filter_map(|homebrew| {
                    launchd.iter().find(|native| {
                        native.provider == "launchd"
                            && same_native_path(native.path.as_deref(), homebrew.path.as_deref())
                    })
                })
                .cloned()
                .collect::<Vec<_>>();
            related.extend(unmatched);
            discovered.extend(related);
        }
    }
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
) -> Result<()> {
    if interval_seconds == 0 {
        anyhow::bail!("daemon interval must be greater than zero");
    }
    let recovered = service::recover_interrupted_runs(registry.path())?;
    if recovered > 0 {
        eprintln!("recovered {recovered} interrupted run(s) after daemon restart");
    }
    let mut server = if once {
        None
    } else if let Some(socket) = socket {
        let registry_path = registry.path().to_path_buf();
        Some(tokio::spawn(async move {
            rpc::serve(socket, registry_path).await
        }))
    } else {
        None
    };
    loop {
        if let Some(server) = server.as_mut() {
            if server.is_finished() {
                server.await??;
                anyhow::bail!("RPC server stopped unexpectedly");
            }
        }
        let pass = service::scheduled_pass(registry.path()).await?;
        println!(
            "scheduler pass: {} automation(s) due, {} failed",
            pass.due, pass.failed
        );
        if once {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_secs(interval_seconds)).await;
    }
}

fn adopt(registry: &Registry, id: &str, dry_run: bool, apply: bool) -> Result<()> {
    if dry_run == apply {
        anyhow::bail!("choose exactly one of --dry-run or --apply");
    }
    let source = registry
        .list_sources()?
        .into_iter()
        .find(|source| source.source_id == id || source.native_id == id)
        .with_context(|| format!("source not found: {id}; run `taskrail scan` first"))?;
    let automation = registry
        .get_automation(&source.source_id)?
        .with_context(|| "discovered source has no Registry automation")?;
    let report = match source.provider.as_str() {
        "cron" => AdoptionEngine::new(registry, CronController::default())
            .adopt(&source, automation, apply)?,
        "launchd" => AdoptionEngine::new(registry, LaunchdController::default())
            .adopt(&source, automation, apply)?,
        "systemd" => AdoptionEngine::new(registry, SystemdController::default())
            .adopt(&source, automation, apply)?,
        provider => anyhow::bail!(
            "native adoption for {provider} is not implemented; source remains observe-only"
        ),
    };
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

fn rollback(registry: &Registry, tx_id: &str) -> Result<()> {
    let (source_id, state, snapshot_json, _, _) = registry
        .adoption(tx_id)?
        .with_context(|| format!("adoption transaction not found: {tx_id}"))?;
    let source = registry
        .list_sources()?
        .into_iter()
        .find(|source| source.source_id == source_id)
        .with_context(|| format!("source not found for adoption transaction {tx_id}"))?;
    let automation = registry
        .list_automations()?
        .into_iter()
        .find(|automation| automation.source_id.as_deref() == Some(source_id.as_str()));
    match source.provider.as_str() {
        "cron" => {
            let snapshot: taskrail::adoption::CronSnapshot =
                serde_json::from_str(&snapshot_json).context("decode cron adoption snapshot")?;
            rollback_adoption(
                registry,
                tx_id,
                &source_id,
                &snapshot,
                automation,
                &CronController::default(),
            )?;
        }
        "launchd" => {
            let snapshot: LaunchdSnapshot =
                serde_json::from_str(&snapshot_json).context("decode launchd adoption snapshot")?;
            rollback_adoption(
                registry,
                tx_id,
                &source_id,
                &snapshot,
                automation,
                &LaunchdController::default(),
            )?;
        }
        "systemd" => {
            let snapshot: SystemdSnapshot =
                serde_json::from_str(&snapshot_json).context("decode systemd adoption snapshot")?;
            rollback_adoption(
                registry,
                tx_id,
                &source_id,
                &snapshot,
                automation,
                &SystemdController::default(),
            )?;
        }
        provider => anyhow::bail!("cannot rollback unsupported provider {provider}"),
    }
    println!("rolled back {tx_id} from {state:?}");
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
