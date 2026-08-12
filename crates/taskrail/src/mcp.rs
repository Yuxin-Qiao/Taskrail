//! Model Context Protocol adapter for ChatGPT and other MCP hosts.
//!
//! The adapter deliberately stays at the edge of Taskrail. The daemon remains
//! the only component that owns the Registry and executes automations; this
//! process translates MCP tool calls into the daemon's local JSON-RPC calls.

use crate::rpc;
use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

const SERVER_NAME: &str = "Taskrail";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");
const INSTRUCTIONS: &str = "Taskrail manages scheduled automations on this host. Call taskrail_status first. When the user asks what automations exist on the local computer, call taskrail_discover_local_automations for a fresh native-scheduler scan, then use taskrail_list_automations or taskrail_get_automation for details. Use taskrail_mole, taskrail_restic, and taskrail_rclone only with their typed actions; writes, destructive cleanup, backups, and syncs are policy-controlled and dry-run should be used first where available. Persisted approvals are plan-bound, expiring, and one-time; they never grant shell access. Use direct argv commands only. Do not claim an automation ran unless the tool result reports its run status. ChatGPT Scheduled tasks can call these tools at their scheduled time.";
const PUBLIC_INSTRUCTIONS: &str = "Taskrail is running in the public read-only review profile. Call taskrail_status first, then use discovery and inspection tools to summarize this host's automation state. This profile never creates, edits, deletes, adopts, pauses, resumes, runs, cancels, or approves work.";

#[derive(Debug, Deserialize)]
struct Request {
    jsonrpc: String,
    #[serde(default)]
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum McpProfile {
    Local,
    PublicReadOnly,
}

const PUBLIC_READ_ONLY_TOOLS: &[&str] = &[
    "taskrail_status",
    "taskrail_list_automations",
    "taskrail_discover_local_automations",
    "taskrail_scan_native",
    "taskrail_list_integrations",
    "taskrail_list_adoptions",
    "taskrail_get_adoption",
    "taskrail_github",
    "taskrail_mas",
    "taskrail_osv_scanner",
    "taskrail_gitleaks",
    "taskrail_trivy",
    "taskrail_get_automation",
    "taskrail_list_runs",
    "taskrail_get_run_logs",
    "taskrail_list_attention",
    "taskrail_list_events",
];

fn profile_from_environment() -> McpProfile {
    match std::env::var("TASKRAIL_MCP_PROFILE").ok().as_deref() {
        Some("public") | Some("public-read-only") => McpProfile::PublicReadOnly,
        _ => McpProfile::Local,
    }
}

fn is_public_tool(name: &str) -> bool {
    PUBLIC_READ_ONLY_TOOLS.contains(&name)
}

pub async fn serve_stdio(socket_path: PathBuf) -> Result<()> {
    let profile = profile_from_environment();
    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin);
    let mut stdout = tokio::io::stdout();
    let mut line = String::new();

    while reader.read_line(&mut line).await? > 0 {
        if line.trim().is_empty() {
            line.clear();
            continue;
        }
        let response = match serde_json::from_str::<Request>(&line) {
            Ok(request) => handle_request_with_profile(request, &socket_path, profile).await,
            Err(error) => Some(error_response(
                Value::Null,
                -32700,
                format!("invalid JSON: {error}"),
            )),
        };
        if let Some(response) = response {
            let mut payload = serde_json::to_vec(&response)?;
            payload.push(b'\n');
            stdout.write_all(&payload).await?;
            stdout.flush().await?;
        }
        line.clear();
    }
    Ok(())
}

async fn handle_request_with_profile(
    request: Request,
    socket_path: &PathBuf,
    profile: McpProfile,
) -> Option<Value> {
    request.id.as_ref()?;
    let id = request.id.unwrap_or(Value::Null);
    if request.jsonrpc != "2.0" {
        return Some(error_response(id, -32600, "jsonrpc must be \"2.0\"".into()));
    }
    let result = match request.method.as_str() {
        "initialize" => Ok(initialize_result_for_profile(&request.params, profile)),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tool_descriptors_for_profile(profile) })),
        "resources/list" => Ok(json!({ "resources": [] })),
        "prompts/list" => Ok(json!({ "prompts": [] })),
        "tools/call" => call_tool_with_profile(&request.params, socket_path, profile).await,
        "notifications/initialized" => Ok(Value::Null),
        method => Err(anyhow::anyhow!("method not found: {method}")),
    };
    Some(match result {
        Ok(result) => success_response(id, result),
        Err(error) => error_response(id, -32000, error.to_string()),
    })
}

fn initialize_result_for_profile(params: &Value, profile: McpProfile) -> Value {
    let protocol_version = params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .filter(|version| matches!(*version, "2025-06-18" | "2025-03-26" | "2024-11-05"))
        .unwrap_or("2025-06-18");
    json!({
        "protocolVersion": protocol_version,
        "capabilities": {
            "tools": {"listChanged": false}
        },
        "serverInfo": {
            "name": SERVER_NAME,
            "version": SERVER_VERSION
        },
        "instructions": match profile {
            McpProfile::Local => INSTRUCTIONS,
            McpProfile::PublicReadOnly => PUBLIC_INSTRUCTIONS,
        }
    })
}

fn tool_descriptors() -> Vec<Value> {
    vec![
        tool(
            "taskrail_status",
            "Taskrail status",
            "Use this first to verify that the Taskrail daemon is connected and identify the local Mac or Linux host. This check only reads daemon status and a fresh native-scheduler summary.",
            object_schema(json!({}), &[]),
            true,
            false,
            true,
        ),
        tool(
            "taskrail_list_automations",
            "List automations",
            "Use this when the user wants to see scheduled, paused, observed, or managed automations on this host.",
            object_schema(json!({}), &[]),
            true,
            false,
            true,
        ),
        tool(
            "taskrail_discover_local_automations",
            "Discover local automations",
            "Use this when the user asks what automation tasks already exist on this Mac or Linux host. It performs a fresh read-only scan of launchd, cron, systemd, and Homebrew services and returns safe summaries without changing native scheduler definitions or the Taskrail Registry.",
            object_schema(
                json!({
                    "source": {"type":"string", "enum":["all","launchd","cron","systemd","homebrew"], "default":"all"},
                }),
                &[],
            ),
            true,
            false,
            true,
        ),
        tool(
            "taskrail_scan_native",
            "Scan native schedulers",
            "Use this when the user wants a fresh, read-only scan of launchd, cron, systemd, or Homebrew services on this host. It does not modify native scheduler definitions or the Taskrail Registry.",
            object_schema(
                json!({
                    "source": {"type":"string", "enum":["all","launchd","cron","systemd","homebrew"]},
                }),
                &[],
            ),
            true,
            false,
            true,
        ),
        tool(
            "taskrail_list_integrations",
            "List integrations",
            "Use this to see the built-in native integrations, whether each executable is available on this host, and which doctor checks need configuration.",
            object_schema(json!({}), &[]),
            true,
            false,
            true,
        ),
        tool(
            "taskrail_schedule_integration",
            "Schedule native integration",
            "Use this to persist a typed, read-only or dry-run native integration as a managed Automation. Recurring writes are refused; one-time writes must go through the expiring approval flow.",
            object_schema(
                json!({
                    "id":{"type":"string","minLength":1},
                    "name":{"type":"string","minLength":1},
                    "integration":{"type":"string","minLength":1},
                    "action":{"type":"string","minLength":1},
                    "parameters":{"type":"object"},
                    "trigger":{"type":"string","enum":["manual","interval","cron"],"default":"manual"},
                    "interval_seconds":{"type":"integer","minimum":1},
                    "cron":{"type":"string","minLength":1},
                    "timezone":{"type":"string","default":"local"}
                }),
                &["id", "integration", "action"],
            ),
            false,
            false,
            false,
        ),
        tool(
            "taskrail_list_adoptions",
            "List adoption transactions",
            "Use this to inspect the journal of native scheduler adoption transactions and their current recovery state.",
            object_schema(
                json!({"limit":{"type":"integer","minimum":1,"maximum":500}}),
                &[],
            ),
            true,
            false,
            true,
        ),
        tool(
            "taskrail_get_adoption",
            "Inspect adoption",
            "Use this to inspect one adoption transaction before deciding whether to roll it back.",
            object_schema(json!({"tx_id":{"type":"string","minLength":1}}), &["tx_id"]),
            true,
            false,
            true,
        ),
        tool(
            "taskrail_adopt_automation",
            "Adopt native automation",
            "Use this first with apply=false for a preflight. Only use apply=true when the user explicitly asks Taskrail to disable the native scheduler entry and make the control plane the owner.",
            object_schema(
                json!({
                    "id":{"type":"string","minLength":1},
                    "apply":{"type":"boolean","default":false}
                }),
                &["id"],
            ),
            false,
            true,
            false,
        ),
        tool(
            "taskrail_rollback_adoption",
            "Rollback adoption",
            "Use this only when the user explicitly requests restoring a native scheduler snapshot. The transaction ID is required and the Registry owner is left fail-closed for review.",
            object_schema(json!({"tx_id":{"type":"string","minLength":1}}), &["tx_id"]),
            false,
            true,
            false,
        ),
        tool(
            "taskrail_acknowledge_drift",
            "Acknowledge source drift",
            "Use this after a fresh native scan shows an intentional external change. It updates the baseline and leaves the owned automation paused for an explicit resume.",
            object_schema(json!({"id":{"type":"string","minLength":1}}), &["id"]),
            false,
            false,
            true,
        ),
        tool(
            "taskrail_mole",
            "Mole integration",
            "Use this for typed Mole actions on this Mac: detect, doctor, version, analyze, status, history, or clean. Prefer clean with dry_run=true. Real cleanup is destructive and remains held by Taskrail policy until durable approval exists; this tool never accepts shell arguments.",
            object_schema(
                json!({
                    "action":{"type":"string","enum":["detect","doctor","version","analyze","status","history","clean"]},
                    "dry_run":{"type":"boolean","default":true},
                    "limit":{"type":"integer","minimum":1,"maximum":200,"default":20},
                    "approval_id":{"type":"string","minLength":1},
                }),
                &["action"],
            ),
            false,
            true,
            true,
        ),
        tool(
            "taskrail_restic",
            "restic integration",
            "Use this for typed restic repository actions: detect, doctor, snapshots, backup, check, forget, or prune. Backup and destructive repository actions are held by Taskrail policy; credentials are environment references only.",
            object_schema(
                json!({
                    "action":{"type":"string","enum":["detect","doctor","snapshots","backup","check","forget","prune"]},
                    "path":{"type":"string","minLength":1},
                    "repository_env":{"type":"string","pattern":"^[A-Za-z_][A-Za-z0-9_]*$"},
                    "password_env":{"type":"string","pattern":"^[A-Za-z_][A-Za-z0-9_]*$"},
                    "approval_id":{"type":"string","minLength":1},
                }),
                &["action"],
            ),
            false,
            true,
            true,
        ),
        tool(
            "taskrail_rclone",
            "rclone integration",
            "Use this for typed rclone actions: detect, doctor, list-remotes, check, copy, or sync. Prefer sync with dry_run=true; copy and real sync are policy-controlled and never accept shell arguments.",
            object_schema(
                json!({
                    "action":{"type":"string","enum":["detect","doctor","list-remotes","check","copy","sync"]},
                    "source":{"type":"string","minLength":1},
                    "destination":{"type":"string","minLength":1},
                    "dry_run":{"type":"boolean","default":true},
                    "config_env":{"type":"string","pattern":"^[A-Za-z_][A-Za-z0-9_]*$"},
                    "approval_id":{"type":"string","minLength":1},
                }),
                &["action"],
            ),
            false,
            true,
            true,
        ),
        tool(
            "taskrail_github",
            "GitHub integration",
            "Use this for the existing typed, read-only GitHub CLI observations: detect, doctor, issues, pulls, failed-runs, or checks. This tool never accepts arbitrary gh api or write arguments.",
            object_schema(
                json!({
                    "action":{"type":"string","enum":["detect","doctor","issues","pulls","failed-runs","checks"]},
                    "repo":{"type":"string","pattern":"^[^/\\s]+/[^/\\s]+$"},
                    "pull_number":{"type":"integer","minimum":1},
                }),
                &["action"],
            ),
            true,
            false,
            true,
        ),
        tool(
            "taskrail_homebrew",
            "Homebrew integration",
            "Use this for typed Homebrew actions: detect, doctor, outdated, bundle-check, upgrade, or cleanup. Prefer upgrade and cleanup with dry_run=true; real writes require Taskrail policy approval and sudo is never used.",
            object_schema(
                json!({
                    "action":{"type":"string","enum":["detect","doctor","outdated","bundle-check","upgrade","cleanup"]},
                    "file":{"type":"string","minLength":1},
                    "dry_run":{"type":"boolean","default":true},
                    "approval_id":{"type":"string","minLength":1},
                }),
                &["action"],
            ),
            false,
            true,
            true,
        ),
        tool(
            "taskrail_mas",
            "Mac App Store integration",
            "Use this for typed macOS App Store inspection: detect, doctor, list, or outdated. It is read-only and never installs or updates an app.",
            object_schema(
                json!({"action":{"type":"string","enum":["detect","doctor","list","outdated"]}}),
                &["action"],
            ),
            true,
            false,
            true,
        ),
        tool(
            "taskrail_osv_scanner",
            "OSV scanner integration",
            "Use this for a typed, read-only OSV dependency scan. Findings are normalized and no raw package secrets or scanner output are persisted.",
            object_schema(
                json!({"action":{"type":"string","enum":["detect","doctor","scan"]},"path":{"type":"string","minLength":1}}),
                &["action"],
            ),
            true,
            false,
            true,
        ),
        tool(
            "taskrail_gitleaks",
            "Gitleaks integration",
            "Use this for a typed, read-only Gitleaks scan. Only rule, location, severity, and a derived fingerprint are exposed; secret or match values are never returned.",
            object_schema(
                json!({"action":{"type":"string","enum":["detect","doctor","scan"]},"path":{"type":"string","minLength":1},"baseline":{"type":"string","minLength":1}}),
                &["action"],
            ),
            true,
            false,
            true,
        ),
        tool(
            "taskrail_trivy",
            "Trivy integration",
            "Use this for typed, read-only Trivy filesystem or repository scans. Vulnerabilities, misconfigurations, secrets, and licenses are normalized without raw secret values.",
            object_schema(
                json!({"action":{"type":"string","enum":["detect","doctor","scan"]},"path":{"type":"string","minLength":1},"scan_type":{"type":"string","enum":["filesystem","repository"],"default":"filesystem"}}),
                &["action"],
            ),
            true,
            false,
            true,
        ),
        tool(
            "taskrail_topgrade",
            "Topgrade integration",
            "Use this for typed Topgrade doctor, inspect, plan, or run actions. Inspect and plan are read-only; run is a system write and remains held by Taskrail policy until durable approval exists.",
            object_schema(
                json!({"action":{"type":"string","enum":["detect","doctor","inspect","plan","run"]},"approval_id":{"type":"string","minLength":1}}),
                &["action"],
            ),
            false,
            true,
            true,
        ),
        tool(
            "taskrail_list_approvals",
            "List approvals",
            "Use this to inspect persisted, expiring approval requests for native integration writes. It never approves an action.",
            object_schema(
                json!({"limit":{"type":"integer","minimum":1,"maximum":500}}),
                &[],
            ),
            true,
            false,
            true,
        ),
        tool(
            "taskrail_request_approval",
            "Request approval",
            "Use this to create a persisted, plan-bound approval request for a typed Mole, restic, rclone, Homebrew, or Topgrade write. It does not execute the action.",
            object_schema(
                json!({
                    "integration":{"type":"string","enum":["mole","restic","rclone","homebrew","topgrade"]},
                    "action":{"type":"string","minLength":1},
                    "parameters":{"type":"object"},
                    "ttl_seconds":{"type":"integer","minimum":1,"maximum":604800,"default":3600},
                }),
                &["integration", "action"],
            ),
            false,
            false,
            false,
        ),
        tool(
            "taskrail_approve",
            "Approve action",
            "Use this only after the operator explicitly approves a specific persisted request; approval is one-time and bound to the stored plan fingerprint.",
            object_schema(
                json!({"approval_id":{"type":"string","minLength":1}}),
                &["approval_id"],
            ),
            false,
            false,
            false,
        ),
        tool(
            "taskrail_reject",
            "Reject action",
            "Use this to reject a persisted native integration approval request.",
            object_schema(
                json!({"approval_id":{"type":"string","minLength":1}}),
                &["approval_id"],
            ),
            false,
            false,
            true,
        ),
        tool(
            "taskrail_execute_approved",
            "Execute approved action",
            "Use this to execute a specific approved native integration request by approval_id. The stored plan must match exactly and the grant is consumed once before the existing policy executor starts.",
            object_schema(
                json!({"approval_id":{"type":"string","minLength":1}}),
                &["approval_id"],
            ),
            false,
            true,
            false,
        ),
        tool(
            "taskrail_get_automation",
            "Inspect automation",
            "Use this when the user wants the definition, trigger, ownership, or next run of one automation.",
            object_schema(json!({"id":{"type":"string","minLength":1}}), &["id"]),
            true,
            false,
            true,
        ),
        tool(
            "taskrail_create_automation",
            "Create automation",
            "Use this when the user wants to schedule a direct executable on this host. Confirm the executable, arguments, working directory, and schedule from the request; do not create shell pipelines or shell strings.",
            object_schema(
                json!({
                    "id":{"type":"string","minLength":1},
                    "name":{"type":"string","minLength":1},
                    "executable":{"type":"string","minLength":1},
                    "args":{"type":"array","items":{"type":"string"}},
                    "cwd":{"type":"string","minLength":1},
                    "trigger":{"type":"string","enum":["manual","interval","cron"],"default":"manual"},
                    "interval_seconds":{"type":"integer","minimum":1},
                    "cron":{"type":"string","minLength":1},
                    "timezone":{"type":"string","default":"local"},
                    "timeout_seconds":{"type":"integer","minimum":1,"maximum":86400},
                }),
                &["id", "executable"],
            ),
            false,
            false,
            false,
        ),
        tool(
            "taskrail_delete_automation",
            "Delete automation",
            "Use this only when the user explicitly asks to delete a managed automation. Observed and adopted automations, and automations with immutable run history, are protected.",
            object_schema(json!({"id":{"type":"string","minLength":1}}), &["id"]),
            false,
            true,
            false,
        ),
        tool(
            "taskrail_pause_automation",
            "Pause automation",
            "Use this when the user wants to pause a managed automation without deleting its definition.",
            object_schema(json!({"id":{"type":"string","minLength":1}}), &["id"]),
            false,
            false,
            true,
        ),
        tool(
            "taskrail_resume_automation",
            "Resume automation",
            "Use this when the user wants to resume a paused managed automation.",
            object_schema(json!({"id":{"type":"string","minLength":1}}), &["id"]),
            false,
            false,
            true,
        ),
        tool(
            "taskrail_run_automation",
            "Run automation now",
            "Use this when the user explicitly asks to run a managed automation immediately. The command may have side effects on the host.",
            object_schema(
                json!({
                    "id":{"type":"string","minLength":1},
                    "allow_observed":{"type":"boolean","default":false},
                }),
                &["id"],
            ),
            false,
            true,
            false,
        ),
        tool(
            "taskrail_list_runs",
            "List automation runs",
            "Use this when the user wants recent run status, timing, or failure information.",
            object_schema(
                json!({
                    "automation_id":{"type":"string","minLength":1},
                    "limit":{"type":"integer","minimum":1,"maximum":500},
                }),
                &[],
            ),
            true,
            false,
            true,
        ),
        tool(
            "taskrail_get_run_logs",
            "Get run logs",
            "Use this when the user wants stdout or stderr for a specific automation run, especially after a failure.",
            object_schema(
                json!({"run_id":{"type":"string","minLength":1}}),
                &["run_id"],
            ),
            true,
            false,
            true,
        ),
        tool(
            "taskrail_cancel_run",
            "Cancel run",
            "Use this when the user explicitly asks to stop an active automation run.",
            object_schema(
                json!({"run_id":{"type":"string","minLength":1}}),
                &["run_id"],
            ),
            false,
            true,
            true,
        ),
        tool(
            "taskrail_list_attention",
            "List attention items",
            "Use this when the user asks what needs attention, including failed runs, drift, or paused items.",
            object_schema(
                json!({"limit":{"type":"integer","minimum":1,"maximum":500}}),
                &[],
            ),
            true,
            false,
            true,
        ),
        tool(
            "taskrail_list_events",
            "List audit events",
            "Use this when the user wants a recent activity or audit trail for Taskrail.",
            object_schema(
                json!({"limit":{"type":"integer","minimum":1,"maximum":500}}),
                &[],
            ),
            true,
            false,
            true,
        ),
    ]
}

fn tool_descriptors_for_profile(profile: McpProfile) -> Vec<Value> {
    let tools = tool_descriptors();
    match profile {
        McpProfile::Local => tools,
        McpProfile::PublicReadOnly => tools
            .into_iter()
            .filter(|tool| tool["name"].as_str().is_some_and(is_public_tool))
            .collect(),
    }
}

fn object_schema(properties: Value, required: &[&str]) -> Value {
    let mut schema = json!({
        "type": "object",
        "properties": properties,
        "additionalProperties": false,
    });
    if !required.is_empty() {
        schema["required"] = json!(required);
    }
    schema
}

fn tool(
    name: &str,
    title: &str,
    description: &str,
    input_schema: Value,
    read_only: bool,
    destructive: bool,
    idempotent: bool,
) -> Value {
    let open_world = matches!(
        name,
        "taskrail_restic"
            | "taskrail_rclone"
            | "taskrail_github"
            | "taskrail_mas"
            | "taskrail_osv_scanner"
            | "taskrail_trivy"
            | "taskrail_run_automation"
            | "taskrail_execute_approved"
    );
    json!({
        "name": name,
        "title": title,
        "description": description,
        "inputSchema": input_schema,
        "outputSchema": {"type":"object","additionalProperties":true},
        "annotations": {
            "readOnlyHint": read_only,
            "destructiveHint": destructive,
            "openWorldHint": open_world,
            "idempotentHint": idempotent,
        }
    })
}

async fn call_tool_with_profile(
    params: &Value,
    socket_path: &PathBuf,
    profile: McpProfile,
) -> Result<Value> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .context("tools/call requires params.name")?;
    if profile == McpProfile::PublicReadOnly && !is_public_tool(name) {
        anyhow::bail!("tool {name} is not available in the public read-only profile");
    }
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let (rpc_method, rpc_params) = match name {
        "taskrail_status" => ("daemon.status", json!({})),
        "taskrail_list_automations" => ("automation.list", json!({})),
        "taskrail_discover_local_automations" => ("automation.discover", arguments),
        "taskrail_scan_native" => ("automation.discover", arguments),
        "taskrail_list_integrations" => ("integration.list", arguments),
        "taskrail_schedule_integration" => ("integration.create", arguments),
        "taskrail_list_adoptions" => ("adoptions.list", arguments),
        "taskrail_get_adoption" => ("adoption.inspect", arguments),
        "taskrail_adopt_automation" => ("adoption.apply", arguments),
        "taskrail_rollback_adoption" => ("adoption.rollback", arguments),
        "taskrail_acknowledge_drift" => ("source.acknowledge_drift", arguments),
        "taskrail_mole" => {
            let action = arguments
                .get("action")
                .and_then(Value::as_str)
                .context("taskrail_mole requires arguments.action")?;
            let parameters = json!({
                "dry_run": arguments.get("dry_run").and_then(Value::as_bool).unwrap_or(true),
                "limit": arguments.get("limit").and_then(Value::as_u64).unwrap_or(20),
            });
            return call_rpc_tool(
                "taskrail_mole",
                socket_path,
                "integration.mole",
                json!({
                    "action": action,
                    "parameters": parameters,
                    "approval_id": arguments.get("approval_id"),
                }),
            )
            .await;
        }
        "taskrail_restic" => {
            let action = arguments
                .get("action")
                .and_then(Value::as_str)
                .context("taskrail_restic requires arguments.action")?;
            let parameters = integration_parameters(&arguments);
            return call_rpc_tool(
                "taskrail_restic",
                socket_path,
                "integration.restic",
                json!({"action":action,"parameters":parameters,"approval_id":arguments.get("approval_id")}),
            )
            .await;
        }
        "taskrail_rclone" => {
            let action = arguments
                .get("action")
                .and_then(Value::as_str)
                .context("taskrail_rclone requires arguments.action")?;
            let parameters = integration_parameters(&arguments);
            return call_rpc_tool(
                "taskrail_rclone",
                socket_path,
                "integration.rclone",
                json!({"action":action,"parameters":parameters,"approval_id":arguments.get("approval_id")}),
            )
            .await;
        }
        "taskrail_github" => {
            let action = arguments
                .get("action")
                .and_then(Value::as_str)
                .context("taskrail_github requires arguments.action")?;
            let parameters = integration_parameters(&arguments);
            return call_rpc_tool(
                "taskrail_github",
                socket_path,
                "integration.github",
                json!({"action":action,"parameters":parameters,"approval_id":arguments.get("approval_id")}),
            )
            .await;
        }
        "taskrail_homebrew" => {
            let action = arguments
                .get("action")
                .and_then(Value::as_str)
                .context("taskrail_homebrew requires arguments.action")?;
            let parameters = integration_parameters(&arguments);
            return call_rpc_tool(
                "taskrail_homebrew",
                socket_path,
                "integration.homebrew",
                json!({"action":action,"parameters":parameters,"approval_id":arguments.get("approval_id")}),
            )
            .await;
        }
        "taskrail_mas" => {
            return call_typed_integration_tool(
                "taskrail_mas",
                socket_path,
                "integration.mas",
                &arguments,
            )
            .await;
        }
        "taskrail_osv_scanner" => {
            return call_typed_integration_tool(
                "taskrail_osv_scanner",
                socket_path,
                "integration.osv-scanner",
                &arguments,
            )
            .await;
        }
        "taskrail_gitleaks" => {
            return call_typed_integration_tool(
                "taskrail_gitleaks",
                socket_path,
                "integration.gitleaks",
                &arguments,
            )
            .await;
        }
        "taskrail_trivy" => {
            return call_typed_integration_tool(
                "taskrail_trivy",
                socket_path,
                "integration.trivy",
                &arguments,
            )
            .await;
        }
        "taskrail_topgrade" => {
            return call_typed_integration_tool(
                "taskrail_topgrade",
                socket_path,
                "integration.topgrade",
                &arguments,
            )
            .await;
        }
        "taskrail_list_approvals" => {
            return call_rpc_tool(
                "taskrail_list_approvals",
                socket_path,
                "approvals.list",
                arguments,
            )
            .await;
        }
        "taskrail_request_approval" => {
            return call_rpc_tool(
                "taskrail_request_approval",
                socket_path,
                "approval.request",
                arguments,
            )
            .await;
        }
        "taskrail_approve" => {
            return call_rpc_tool(
                "taskrail_approve",
                socket_path,
                "approval.approve",
                arguments,
            )
            .await;
        }
        "taskrail_reject" => {
            return call_rpc_tool("taskrail_reject", socket_path, "approval.reject", arguments)
                .await;
        }
        "taskrail_execute_approved" => {
            return call_rpc_tool(
                "taskrail_execute_approved",
                socket_path,
                "approval.execute",
                arguments,
            )
            .await;
        }
        "taskrail_get_automation" => ("automation.inspect", arguments),
        "taskrail_create_automation" => ("automation.create", arguments),
        "taskrail_delete_automation" => ("automation.delete", arguments),
        "taskrail_pause_automation" => ("automation.pause", arguments),
        "taskrail_resume_automation" => ("automation.resume", arguments),
        "taskrail_run_automation" => ("automation.run", arguments),
        "taskrail_list_runs" => ("runs.list", arguments),
        "taskrail_get_run_logs" => ("run.logs", arguments),
        "taskrail_cancel_run" => ("run.cancel", arguments),
        "taskrail_list_attention" => ("inbox.list", arguments),
        "taskrail_list_events" => ("events.list", arguments),
        _ => anyhow::bail!("unknown Taskrail tool: {name}"),
    };
    let value = sanitize_tool_value(name, rpc::call(socket_path, rpc_method, rpc_params).await?);
    let value = if name == "taskrail_status" {
        // ChatGPT can cache an older tool descriptor for a connected Tunnel.
        // Keep the stable status entry useful for that client by attaching a
        // fresh, safe native-scheduler scan to it as well.
        let discovery = rpc::call(socket_path, "automation.discover", json!({"source":"all"}))
            .await
            .map(sanitize_discovered_sources)?;
        json!({
            "daemon": value,
            "host": {
                "label": std::env::var("TASKRAIL_HOST_LABEL").ok(),
                "platform": std::env::consts::OS,
                "architecture": std::env::consts::ARCH,
            },
            "local_discovery": discovery,
        })
    } else {
        value
    };
    let summary = summarize(name, &value);
    Ok(json!({
        "content": [{"type":"text","text":summary}],
        "structuredContent": {"result": value},
        "isError": false,
    }))
}

async fn call_rpc_tool(
    name: &str,
    socket_path: &PathBuf,
    rpc_method: &str,
    rpc_params: Value,
) -> Result<Value> {
    let value = sanitize_tool_value(name, rpc::call(socket_path, rpc_method, rpc_params).await?);
    let summary = summarize(name, &value);
    Ok(json!({
        "content": [{"type":"text","text":summary}],
        "structuredContent": {"result": value},
        "isError": false,
    }))
}

async fn call_typed_integration_tool(
    name: &str,
    socket_path: &PathBuf,
    rpc_method: &str,
    arguments: &Value,
) -> Result<Value> {
    let action = arguments
        .get("action")
        .and_then(Value::as_str)
        .with_context(|| format!("{name} requires arguments.action"))?;
    let parameters = integration_parameters(arguments);
    call_rpc_tool(
        name,
        socket_path,
        rpc_method,
        json!({
            "action": action,
            "parameters": parameters,
            "approval_id": arguments.get("approval_id"),
        }),
    )
    .await
}

fn integration_parameters(arguments: &Value) -> Value {
    let mut object = arguments.as_object().cloned().unwrap_or_default();
    object.remove("action");
    object.remove("approval_id");
    Value::Object(object)
}

fn sanitize_automation_value(mut value: Value) -> Value {
    match &mut value {
        Value::Array(items) => {
            for item in items {
                sanitize_automation_object(item);
            }
        }
        _ => sanitize_automation_object(&mut value),
    }
    value
}

fn sanitize_automation_object(value: &mut Value) {
    let Some(steps) = value.get_mut("steps").and_then(Value::as_array_mut) else {
        return;
    };
    for step in steps {
        if let Some(command) = step.get_mut("command").and_then(Value::as_object_mut) {
            if let Some(cwd) = command.get_mut("cwd") {
                *cwd = sanitize_local_path(Some(cwd));
            }
            if let Some(executable) = command.get_mut("executable") {
                *executable = sanitize_local_path(Some(executable));
            }
            if let Some(args) = command.get_mut("args") {
                *args = sanitize_command_args(args);
            }
            if let Some(environment) = command.get_mut("env").and_then(Value::as_object_mut) {
                for value in environment.values_mut() {
                    *value = Value::String("[REDACTED]".into());
                }
            }
        }
        if let Some(integration) = step.get_mut("integration").and_then(Value::as_object_mut) {
            sanitize_secret_like_values(integration);
        }
    }
}

fn sanitize_command_args(value: &Value) -> Value {
    let Some(items) = value.as_array() else {
        return value.clone();
    };
    let mut redact_next = false;
    Value::Array(
        items
            .iter()
            .map(|item| {
                let Some(text) = item.as_str() else {
                    return item.clone();
                };
                let normalized = text.to_ascii_lowercase();
                let flag = normalized.trim_start_matches('-');
                let sensitive_flag = flag.split('=').next().is_some_and(|key| {
                    (key.contains("token")
                        || key.contains("password")
                        || key.contains("secret")
                        || key.contains("api_key")
                        || key.contains("authorization")
                        || key.contains("private_key"))
                        && !key.ends_with("_env")
                        && !key.ends_with("_file")
                        && !key.ends_with("_ref")
                });
                let inline_secret = normalized.starts_with("sk-")
                    || normalized.starts_with("ghp_")
                    || normalized.starts_with("github_pat_")
                    || normalized.starts_with("bearer ")
                    || normalized.contains("token=")
                    || normalized.contains("password=")
                    || normalized.contains("secret=")
                    || normalized.contains("api_key=");
                let redact = redact_next || inline_secret || (sensitive_flag && text.contains('='));
                redact_next = sensitive_flag && !text.contains('=');
                if redact {
                    Value::String("[REDACTED]".into())
                } else {
                    Value::String(sanitize_home_prefix(text))
                }
            })
            .collect(),
    )
}

fn sanitize_secret_like_values(value: &mut serde_json::Map<String, Value>) {
    let mut sanitized = Value::Object(std::mem::take(value));
    sanitize_private_value(&mut sanitized);
    if let Value::Object(object) = sanitized {
        *value = object;
    }
}

fn sanitize_tool_value(name: &str, mut value: Value) -> Value {
    value = match name {
        "taskrail_discover_local_automations" | "taskrail_scan_native" => {
            sanitize_discovered_sources(value)
        }
        "taskrail_list_automations" | "taskrail_get_automation" => sanitize_automation_value(value),
        "taskrail_list_adoptions" | "taskrail_get_adoption" => sanitize_adoption_value(value),
        "taskrail_list_runs" => sanitize_run_list_value(value),
        "taskrail_list_events" => sanitize_event_list_value(value),
        _ => {
            sanitize_private_value(&mut value);
            value
        }
    };
    sanitize_private_value(&mut value);
    value
}

fn sanitize_private_value(value: &mut Value) {
    match value {
        Value::Object(object) => {
            for (key, item) in object.iter_mut() {
                let normalized = key.to_ascii_lowercase();
                let reference = normalized.ends_with("_env")
                    || normalized.ends_with("_file")
                    || normalized.ends_with("_ref");
                if normalized == "raw" {
                    *item = json!("[OMITTED]");
                } else if normalized == "env" {
                    *item = json!("[REDACTED]");
                } else if matches!(
                    normalized.as_str(),
                    "path" | "cwd" | "executable" | "source" | "destination" | "file" | "baseline"
                ) {
                    *item = sanitize_local_path(Some(item));
                } else if !reference
                    && (normalized.contains("secret")
                        || normalized.contains("token")
                        || normalized.contains("password")
                        || normalized.contains("api_key")
                        || normalized.contains("private_key"))
                {
                    *item = json!("[REDACTED]");
                } else {
                    sanitize_private_value(item);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                sanitize_private_value(item);
            }
        }
        Value::String(text) => {
            *text = sanitize_home_prefix(text);
        }
        _ => {}
    }
}

fn sanitize_home_prefix(path: &str) -> String {
    let Some(home) = std::env::var_os("HOME").and_then(|value| value.into_string().ok()) else {
        return path.to_owned();
    };
    if path == home {
        return "~".into();
    }
    path.replace(&format!("{home}/"), "~/")
}

fn sanitize_adoption_value(mut value: Value) -> Value {
    match &mut value {
        Value::Array(items) => {
            for item in items {
                sanitize_adoption_object(item);
            }
        }
        _ => sanitize_adoption_object(&mut value),
    }
    value
}

fn sanitize_adoption_object(value: &mut Value) {
    let Some(snapshot) = value.get_mut("snapshot") else {
        return;
    };
    let snapshot_value = std::mem::take(snapshot);
    let sanitized = sanitize_discovered_sources(json!([snapshot_value]));
    *snapshot = sanitized
        .get("automations")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .cloned()
        .unwrap_or_else(|| json!({"redacted": true}));
}

fn sanitize_event_list_value(mut value: Value) -> Value {
    if let Some(events) = value.as_array_mut() {
        for event in events {
            sanitize_event_object(event);
        }
    }
    value
}

fn sanitize_event_object(value: &mut Value) {
    sanitize_private_value(value);
}

fn sanitize_run_list_value(mut value: Value) -> Value {
    if let Some(runs) = value.as_array_mut() {
        for run in runs {
            if let Some(snapshot) = run.get_mut("automation_snapshot") {
                let sanitized = sanitize_automation_value(std::mem::take(snapshot));
                *snapshot = sanitized;
            }
        }
    }
    value
}

fn summarize(name: &str, value: &Value) -> String {
    match name {
        "taskrail_status" => format!(
            "Taskrail daemon connected on {} ({}), managing {} automation(s); fresh local scan found {} source(s).",
            std::env::consts::OS,
            std::env::consts::ARCH,
            value
                .get("daemon")
                .and_then(|daemon| daemon.get("automation_count"))
                .and_then(Value::as_u64)
                .unwrap_or(0),
            value
                .get("local_discovery")
                .and_then(|discovery| discovery.get("count"))
                .and_then(Value::as_u64)
                .unwrap_or(0)
        ),
        "taskrail_list_automations" => format!(
            "Taskrail returned {} automation(s).",
            value.as_array().map_or(0, Vec::len)
        ),
        "taskrail_list_integrations" => format!(
            "Taskrail returned {} integration descriptor(s).",
            value
                .get("descriptors")
                .and_then(Value::as_array)
                .map_or(0, Vec::len)
        ),
        "taskrail_schedule_integration" => {
            let name = value
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("automation");
            format!("Created scheduled native integration automation {name}.")
        }
        "taskrail_list_adoptions" => format!(
            "Taskrail returned {} adoption transaction(s).",
            value.as_array().map_or(0, Vec::len)
        ),
        "taskrail_adopt_automation" => format!(
            "Native adoption preflight/apply returned state {}.",
            value
                .get("state")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
        ),
        "taskrail_rollback_adoption" => {
            "Adoption rollback completed; the control-plane owner is fail-closed for review.".into()
        }
        "taskrail_acknowledge_drift" => {
            "Source drift baseline updated; the owned automation remains paused.".into()
        }
        "taskrail_discover_local_automations" => format!(
            "Fresh local scan discovered {} automation source(s).",
            value
                .get("automations")
                .and_then(Value::as_array)
                .map_or(0, Vec::len)
        ),
        "taskrail_mole" => {
            if let Some(status) = value
                .get("verification")
                .and_then(|item| item.get("status"))
            {
                format!("Mole integration completed; verification status {status}.")
            } else if let Some(status) = value.get("status").and_then(Value::as_str) {
                format!("Mole integration status: {status}.")
            } else {
                "Mole integration inspection completed.".into()
            }
        }
        "taskrail_restic" => integration_summary("restic", value),
        "taskrail_rclone" => integration_summary("rclone", value),
        "taskrail_github" => integration_summary("GitHub", value),
        "taskrail_homebrew" => integration_summary("Homebrew", value),
        "taskrail_mas" => integration_summary("mas", value),
        "taskrail_osv_scanner" => integration_summary("OSV-Scanner", value),
        "taskrail_gitleaks" => integration_summary("Gitleaks", value),
        "taskrail_trivy" => integration_summary("Trivy", value),
        "taskrail_topgrade" => integration_summary("Topgrade", value),
        "taskrail_list_approvals" => format!(
            "Taskrail returned {} approval request(s).",
            value.as_array().map_or(0, Vec::len)
        ),
        "taskrail_request_approval" => "Approval request persisted; no action was executed.".into(),
        "taskrail_approve" => {
            "Approval granted; the next matching typed action may consume it once.".into()
        }
        "taskrail_reject" => "Approval request rejected; no action was executed.".into(),
        "taskrail_execute_approved" => "Approved typed action execution completed.".into(),
        "taskrail_scan_native" => format!(
            "Fresh read-only scan found {} native automation source(s).",
            value.as_array().map_or(0, Vec::len)
        ),
        "taskrail_create_automation" => {
            let name = value
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("automation");
            format!("Created managed automation {name}.")
        }
        "taskrail_delete_automation" => {
            "Managed automation deleted; immutable run history was preserved.".into()
        }
        "taskrail_run_automation" => {
            let run_id = value
                .get("run_id")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let status = value
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            format!("Automation run {run_id} finished with status {status}.")
        }
        _ => format!("Taskrail tool {name} completed."),
    }
}

fn integration_summary(name: &str, value: &Value) -> String {
    if let Some(status) = value
        .get("verification")
        .and_then(|item| item.get("status"))
    {
        format!("{name} integration completed; verification status {status}.")
    } else if let Some(status) = value.get("status").and_then(Value::as_str) {
        format!("{name} integration status: {status}.")
    } else {
        format!("{name} integration inspection completed.")
    }
}

fn sanitize_discovered_sources(value: Value) -> Value {
    let automations = value
        .as_array()
        .into_iter()
        .flatten()
        .map(|source| {
            let command = source.get("command").and_then(|command| {
                Some(json!({
                    "executable": sanitize_local_path(command.get("executable")),
                    "args": sanitize_command_args(command.get("args")?),
                    "cwd": sanitize_local_path(command.get("cwd")),
                    "shell": command.get("shell").unwrap_or(&Value::Bool(false)),
                }))
            });
            json!({
                "id": source.get("source_id"),
                "name": source.get("native_id"),
                "provider": source.get("provider"),
                "kind": source.get("kind"),
                "enabled": source.get("enabled"),
                "path": sanitize_local_path(source.get("path")),
                "trigger": source.get("trigger"),
                "command": command,
                "ownership": "observed",
            })
        })
        .collect::<Vec<_>>();
    let mut providers = automations
        .iter()
        .filter_map(|item| item.get("provider").and_then(Value::as_str))
        .collect::<Vec<_>>();
    providers.sort_unstable();
    providers.dedup();
    json!({
        "count": automations.len(),
        "providers": providers,
        "automations": automations,
        "native_definitions_changed": false,
    })
}

fn sanitize_local_path(value: Option<&Value>) -> Value {
    let Some(Value::String(path)) = value else {
        return value.cloned().unwrap_or(Value::Null);
    };
    Value::String(sanitize_home_prefix(path))
}

fn success_response(id: Value, result: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"result":result})
}

fn error_response(id: Value, code: i32, message: String) -> Value {
    json!({
        "jsonrpc":"2.0",
        "id":id,
        "error":{"code":code,"message":message}
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptors_have_unique_names_and_schemas() {
        let tools = tool_descriptors();
        let mut names = tools
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect::<Vec<_>>();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), tools.len());
        assert!(names.contains(&"taskrail_mole"));
        assert!(names.contains(&"taskrail_restic"));
        assert!(names.contains(&"taskrail_rclone"));
        assert!(names.contains(&"taskrail_github"));
        assert!(names.contains(&"taskrail_homebrew"));
        assert!(names.contains(&"taskrail_mas"));
        assert!(names.contains(&"taskrail_osv_scanner"));
        assert!(names.contains(&"taskrail_gitleaks"));
        assert!(names.contains(&"taskrail_trivy"));
        assert!(names.contains(&"taskrail_topgrade"));
        assert!(names.contains(&"taskrail_list_approvals"));
        assert!(names.contains(&"taskrail_request_approval"));
        assert!(names.contains(&"taskrail_approve"));
        assert!(names.contains(&"taskrail_reject"));
        assert!(names.contains(&"taskrail_execute_approved"));
        for tool in tools {
            assert_eq!(tool["inputSchema"]["type"], "object");
            assert!(
                tool["description"]
                    .as_str()
                    .unwrap()
                    .starts_with("Use this")
            );
            assert!(tool["annotations"].is_object());
        }
    }

    #[test]
    fn initialize_result_advertises_tools_and_instructions() {
        let result = initialize_result_for_profile(
            &json!({"protocolVersion":"2025-06-18"}),
            McpProfile::Local,
        );
        assert_eq!(result["serverInfo"]["name"], SERVER_NAME);
        assert_eq!(result["capabilities"]["tools"]["listChanged"], false);
        assert!(
            result["instructions"]
                .as_str()
                .unwrap()
                .contains("Scheduled")
        );
        assert!(
            result["instructions"]
                .as_str()
                .unwrap()
                .contains("discover_local_automations")
        );
    }

    #[test]
    fn discovered_sources_are_summarized_without_raw_definition_or_environment() {
        let value = sanitize_discovered_sources(json!([{
            "source_id": "launchd:example",
            "native_id": "example",
            "provider": "launchd",
            "kind": "task",
            "enabled": true,
            "path": "/Users/example/Library/LaunchAgents/example.plist",
            "trigger": {"kind":"manual"},
            "command": {
                "executable": "/bin/echo",
                "args": ["--token", "secret-value", "--path", "/Users/example/private"],
                "cwd": null,
                "env": {"TOKEN":"secret"},
                "shell": false
            },
            "raw": "secret raw plist"
        }]));
        assert_eq!(value["count"], 1);
        assert_eq!(value["automations"][0]["name"], "example");
        assert!(value["automations"][0].get("raw").is_none());
        assert!(value["automations"][0]["command"].get("env").is_none());
        assert_eq!(value["automations"][0]["command"]["args"][1], "[REDACTED]");
    }

    #[test]
    fn automation_definitions_redact_environment_values_before_mcp_response() {
        let value = sanitize_automation_value(json!({
            "id": "example",
            "steps": [{
                "command": {
                    "args": ["--token", "secret-value", "--path", "/Users/example/private"],
                    "env": {"TOKEN": "secret-value"}
                }
            }]
        }));
        assert_eq!(value["steps"][0]["command"]["env"]["TOKEN"], "[REDACTED]");
        assert_eq!(value["steps"][0]["command"]["args"][1], "[REDACTED]");
        assert_eq!(
            value["steps"][0]["command"]["args"][3],
            "/Users/example/private"
        );
    }

    #[test]
    fn automation_definitions_redact_integration_secret_like_values() {
        let value = sanitize_automation_value(json!({
            "steps": [{
                "integration": {
                    "integration": "example",
                    "action": "run",
                    "parameters": {
                        "token": "secret-value",
                        "token_env": "EXAMPLE_TOKEN"
                    }
                }
            }]
        }));
        assert_eq!(
            value["steps"][0]["integration"]["parameters"]["token"],
            "[REDACTED]"
        );
        assert_eq!(
            value["steps"][0]["integration"]["parameters"]["token_env"],
            "EXAMPLE_TOKEN"
        );
    }

    #[test]
    fn automation_definitions_redact_integration_paths() {
        let home = std::env::var("HOME").unwrap();
        let value = sanitize_automation_value(json!({
            "steps": [{
                "integration": {
                    "integration": "example",
                    "action": "inspect",
                    "parameters": {
                        "path": format!("{home}/Projects/private-repo"),
                        "source": format!("{home}/private-source"),
                        "destination": "remote:backup",
                    }
                }
            }]
        }));
        assert_eq!(
            value["steps"][0]["integration"]["parameters"]["path"],
            "~/Projects/private-repo"
        );
        assert_eq!(
            value["steps"][0]["integration"]["parameters"]["source"],
            "~/private-source"
        );
        assert_eq!(
            value["steps"][0]["integration"]["parameters"]["destination"],
            "remote:backup"
        );
    }

    #[test]
    fn local_paths_redact_the_current_home_prefix() {
        let home = std::env::var("HOME").unwrap();
        assert_eq!(
            sanitize_local_path(Some(&json!(format!("{home}/work")))),
            "~/work"
        );
    }

    #[test]
    fn public_profile_exposes_only_read_only_tools() {
        let tools = tool_descriptors_for_profile(McpProfile::PublicReadOnly);
        assert_eq!(tools.len(), PUBLIC_READ_ONLY_TOOLS.len());
        assert!(tools.iter().all(|tool| {
            tool["annotations"]["readOnlyHint"] == true
                && tool["annotations"]["destructiveHint"] == false
        }));
        assert!(
            tools
                .iter()
                .all(|tool| is_public_tool(tool["name"].as_str().unwrap()))
        );
        assert!(
            tools
                .iter()
                .all(|tool| tool["outputSchema"]["type"] == "object")
        );
        assert!(
            !tools
                .iter()
                .any(|tool| tool["name"] == "taskrail_run_automation")
        );
        assert!(
            !tools
                .iter()
                .any(|tool| tool["name"] == "taskrail_execute_approved")
        );
    }

    #[tokio::test]
    async fn public_profile_rejects_hidden_write_tool_calls() {
        let path = PathBuf::from("/tmp/taskrail-public-profile-test.sock");
        let response = handle_request_with_profile(
            Request {
                jsonrpc: "2.0".into(),
                id: Some(json!(1)),
                method: "tools/call".into(),
                params: json!({
                    "name": "taskrail_run_automation",
                    "arguments": {"id": "anything"}
                }),
            },
            &path,
            McpProfile::PublicReadOnly,
        )
        .await
        .unwrap();
        assert_eq!(response["error"]["code"], -32000);
        assert!(
            response["error"]["message"]
                .as_str()
                .unwrap()
                .contains("public read-only profile")
        );
    }

    #[test]
    fn adoption_and_event_summaries_omit_raw_private_fields() {
        let adoption = sanitize_adoption_value(json!([{
            "snapshot": {
                "source_id": "cron:private",
                "native_id": "private",
                "provider": "cron",
                "path": "/Users/example/private.cron",
                "raw": "TOKEN=secret"
            }
        }]));
        assert!(adoption[0]["snapshot"].get("raw").is_none());
        assert!(adoption[0]["snapshot"]["path"].is_string());

        let events = sanitize_event_list_value(json!([{
            "payload": {"raw": "secret", "env": {"TOKEN": "secret"}}
        }]));
        assert_eq!(events[0]["payload"]["raw"], "[OMITTED]");
        assert_eq!(events[0]["payload"]["env"], "[REDACTED]");
    }

    #[test]
    fn lifecycle_tools_are_advertised_with_safe_annotations() {
        let tools = tool_descriptors();
        let find = |name: &str| tools.iter().find(|tool| tool["name"] == name).unwrap();
        assert_eq!(
            find("taskrail_list_integrations")["annotations"]["readOnlyHint"],
            true
        );
        assert_eq!(
            find("taskrail_schedule_integration")["annotations"]["destructiveHint"],
            false
        );
        assert_eq!(
            find("taskrail_adopt_automation")["annotations"]["destructiveHint"],
            true
        );
        assert_eq!(
            find("taskrail_delete_automation")["annotations"]["destructiveHint"],
            true
        );
    }
}
