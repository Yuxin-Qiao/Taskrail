//! A small, explicit multi-host gateway for Taskrail MCP endpoints.
//!
//! The fleet layer is intentionally a client-side registry, not a hosted
//! control plane. It stores endpoint metadata and environment-variable names,
//! never bearer values. Every remote host remains the authority for its own
//! Registry, policy, approvals, and execution.

use anyhow::{Context, Result};
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{net::IpAddr, path::Path, time::Duration};

const CONFIG_VERSION: u32 = 1;
const MAX_HOSTS: usize = 64;
const MAX_ID_LENGTH: usize = 64;
const MAX_LABEL_LENGTH: usize = 160;
const DEFAULT_TIMEOUT_SECONDS: u64 = 15;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetConfig {
    #[serde(default = "default_config_version")]
    pub version: u32,
    #[serde(default)]
    pub hosts: Vec<FleetHost>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetHost {
    /// Stable local routing key. This is not the remote Registry host ID.
    pub id: String,
    pub label: String,
    /// MCP `/mcp` endpoint. Credentials must be supplied through `token_env`.
    pub endpoint: String,
    #[serde(default)]
    pub token_env: Option<String>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Opt in to exposing mutating fleet tools for this host. The remote MCP
    /// server must still advertise the requested tool and enforce its policy.
    #[serde(default)]
    pub allow_writes: bool,
}

fn default_config_version() -> u32 {
    CONFIG_VERSION
}

fn default_enabled() -> bool {
    true
}

impl FleetConfig {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("read Taskrail fleet config {}", path.display()))?;
        let config: Self = serde_yaml::from_str(&content)
            .with_context(|| format!("parse Taskrail fleet config {}", path.display()))?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        if self.version != CONFIG_VERSION {
            anyhow::bail!(
                "unsupported Taskrail fleet config version {}; expected {}",
                self.version,
                CONFIG_VERSION
            );
        }
        if self.hosts.is_empty() {
            anyhow::bail!("Taskrail fleet config must contain at least one host");
        }
        if self.hosts.len() > MAX_HOSTS {
            anyhow::bail!("Taskrail fleet config supports at most {MAX_HOSTS} hosts");
        }
        for (index, host) in self.hosts.iter().enumerate() {
            validate_host(host).with_context(|| format!("invalid fleet host at index {index}"))?;
            if self
                .hosts
                .iter()
                .take(index)
                .any(|previous| previous.id == host.id)
            {
                anyhow::bail!("duplicate Taskrail fleet host id {}", host.id);
            }
        }
        Ok(())
    }

    pub fn enabled_hosts(&self) -> impl Iterator<Item = &FleetHost> {
        self.hosts.iter().filter(|host| host.enabled)
    }
}

fn validate_host(host: &FleetHost) -> Result<()> {
    if host.id.is_empty()
        || host.id.len() > MAX_ID_LENGTH
        || !host
            .id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._-".contains(character))
    {
        anyhow::bail!("id must use only ASCII letters, digits, '.', '_' or '-' and be bounded");
    }
    if host.label.trim().is_empty() || host.label.len() > MAX_LABEL_LENGTH {
        anyhow::bail!("label must be non-empty and at most {MAX_LABEL_LENGTH} bytes");
    }
    let url = Url::parse(&host.endpoint).context("endpoint must be a valid URL")?;
    if url.username() != "" || url.password().is_some() {
        anyhow::bail!("endpoint must not contain embedded credentials");
    }
    let scheme = url.scheme();
    let host_name = url.host_str().context("endpoint host is missing")?;
    let loopback = host_name
        .parse::<IpAddr>()
        .map(|address| address.is_loopback())
        .unwrap_or_else(|_| host_name.eq_ignore_ascii_case("localhost"));
    if scheme != "https" && !(scheme == "http" && loopback) {
        anyhow::bail!("endpoint must use HTTPS; plain HTTP is allowed only for localhost");
    }
    if !url.path().ends_with("/mcp") {
        anyhow::bail!("endpoint path must end with /mcp");
    }
    if let Some(token_env) = &host.token_env {
        let mut characters = token_env.chars();
        let valid_first = characters
            .next()
            .is_some_and(|character| character == '_' || character.is_ascii_alphabetic());
        if !valid_first
            || !characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
        {
            anyhow::bail!("token_env must be a valid environment variable name");
        }
    }
    if host.allow_writes && host.token_env.is_none() {
        anyhow::bail!("allow_writes hosts must define token_env");
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct FleetGateway {
    config: FleetConfig,
    client: Client,
}

impl FleetGateway {
    pub fn from_config(config: FleetConfig) -> Result<Self> {
        config.validate()?;
        let client = Client::builder()
            .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECONDS))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .context("build Taskrail fleet HTTP client")?;
        Ok(Self { config, client })
    }

    pub fn config(&self) -> &FleetConfig {
        &self.config
    }

    pub fn host(&self, id: &str) -> Result<&FleetHost> {
        self.config
            .enabled_hosts()
            .find(|host| host.id == id)
            .with_context(|| format!("Taskrail fleet host not found or disabled: {id}"))
    }

    pub async fn call_tool(&self, host_id: &str, tool: &str, arguments: Value) -> Result<Value> {
        let host = self.host(host_id)?;
        if (is_mutating_tool(tool) || is_mutating_integration_call(tool, &arguments))
            && !host.allow_writes
        {
            anyhow::bail!(
                "fleet host {host_id} is read-only; set allow_writes=true only for an explicitly trusted private endpoint"
            );
        }
        let result = self
            .call_mcp(
                host,
                "tools/call",
                json!({
                    "name": tool,
                    "arguments": arguments,
                }),
            )
            .await?;
        if result
            .get("isError")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            anyhow::bail!(remote_error_text(&result));
        }
        Ok(result
            .get("structuredContent")
            .and_then(|value| value.get("result"))
            .cloned()
            .unwrap_or(result))
    }

    pub async fn host_overview(&self, host_id: &str) -> Result<Value> {
        self.call_tool(host_id, "taskrail_overview", json!({}))
            .await
    }

    pub async fn fleet_overview(&self) -> Value {
        let mut hosts = Vec::new();
        for host in &self.config.hosts {
            if !host.enabled {
                hosts.push(json!({
                    "id": host.id,
                    "label": host.label,
                    "enabled": false,
                    "allow_writes": host.allow_writes,
                    "status": "disabled",
                }));
                continue;
            }
            let result = self.host_overview(&host.id).await;
            let mut summary = json!({
                "id": host.id,
                "label": host.label,
                "enabled": true,
                "allow_writes": host.allow_writes,
                "status": if result.is_ok() { "online" } else { "offline" },
            });
            match result {
                Ok(overview) => summary["overview"] = overview,
                Err(error) => summary["error"] = json!(bounded_error(&error.to_string())),
            }
            hosts.push(summary);
        }
        let online_count = hosts
            .iter()
            .filter(|host| host["status"] == "online")
            .count();
        let enabled_count = hosts.iter().filter(|host| host["enabled"] == true).count();
        json!({
            "host_count": hosts.len(),
            "enabled_count": enabled_count,
            "online_count": online_count,
            "hosts": hosts,
        })
    }

    async fn call_mcp(&self, host: &FleetHost, method: &str, params: Value) -> Result<Value> {
        let token = host
            .token_env
            .as_deref()
            .map(std::env::var)
            .transpose()
            .with_context(|| {
                format!("read token environment variable for fleet host {}", host.id)
            })?;
        let request = json!({
            "jsonrpc": "2.0",
            "id": format!("fleet-{}", uuid::Uuid::new_v4()),
            "method": method,
            "params": params,
        });
        let mut builder = self
            .client
            .post(&host.endpoint)
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .header("mcp-protocol-version", "2025-11-25")
            .json(&request);
        if let Some(token) = token.as_deref() {
            if token.trim().is_empty() {
                anyhow::bail!(
                    "token environment variable for fleet host {} is empty",
                    host.id
                );
            }
            builder = builder.bearer_auth(token);
        }
        let response = builder
            .send()
            .await
            .with_context(|| format!("call Taskrail fleet host {}", host.id))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .with_context(|| format!("read response from Taskrail fleet host {}", host.id))?;
        if !status.is_success() {
            anyhow::bail!(
                "fleet host {} returned HTTP {}: {}",
                host.id,
                status,
                bounded_error(&body)
            );
        }
        let payload: JsonRpcResponse = serde_json::from_str(&body)
            .with_context(|| format!("decode MCP response from fleet host {}", host.id))?;
        if let Some(error) = payload.error {
            anyhow::bail!(
                "fleet host {} MCP error {}: {}",
                host.id,
                error.code,
                error.message
            );
        }
        payload
            .result
            .context("MCP response did not contain a result")
    }
}

#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    result: Option<Value>,
    error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}

pub fn is_mutating_tool(tool: &str) -> bool {
    matches!(
        tool,
        "taskrail_create_automation"
            | "taskrail_delete_automation"
            | "taskrail_pause_automation"
            | "taskrail_resume_automation"
            | "taskrail_run_automation"
            | "taskrail_cancel_run"
            | "taskrail_schedule_integration"
            | "taskrail_adopt_automation"
            | "taskrail_rollback_adoption"
            | "taskrail_acknowledge_drift"
            | "taskrail_request_approval"
            | "taskrail_approve"
            | "taskrail_reject"
            | "taskrail_execute_approved"
    )
}

/// Integration tools contain both read-only and write-capable actions. Keep
/// this decision at the Fleet boundary so a read-only host is rejected before
/// any network request is sent to the remote MCP server.
pub fn is_mutating_integration_call(tool: &str, arguments: &Value) -> bool {
    let action = arguments.get("action").and_then(Value::as_str);
    match (tool, action) {
        ("taskrail_mole", Some("clean")) => !arguments
            .get("dry_run")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        ("taskrail_restic", Some("backup" | "forget" | "prune")) => true,
        ("taskrail_rclone", Some("copy")) => true,
        ("taskrail_rclone", Some("sync")) => !arguments
            .get("dry_run")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        ("taskrail_homebrew", Some("upgrade" | "cleanup")) => !arguments
            .get("dry_run")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        ("taskrail_topgrade", Some("run")) => true,
        _ => false,
    }
}

fn remote_error_text(result: &Value) -> String {
    result
        .get("content")
        .and_then(Value::as_array)
        .and_then(|content| content.iter().find_map(|item| item.get("text")))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| "remote Taskrail tool returned an error".into())
}

fn bounded_error(value: &str) -> String {
    const MAX_ERROR_BYTES: usize = 512;
    value.chars().take(MAX_ERROR_BYTES).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn host(allow_writes: bool) -> FleetHost {
        FleetHost {
            id: "macbook".into(),
            label: "MacBook".into(),
            endpoint: "https://taskrail.example/mcp".into(),
            token_env: allow_writes.then_some("TASKRAIL_MACBOOK_TOKEN".into()),
            enabled: true,
            allow_writes,
        }
    }

    #[test]
    fn validates_safe_fleet_config_without_credentials() {
        let config = FleetConfig {
            version: 1,
            hosts: vec![host(false)],
        };
        config.validate().unwrap();
        let serialized = serde_yaml::to_string(&config).unwrap();
        assert!(!serialized.contains("Bearer"));
        assert!(!serialized.contains("secret"));
    }

    #[test]
    fn rejects_plain_http_remote_and_embedded_credentials() {
        let mut config = FleetConfig {
            version: 1,
            hosts: vec![host(false)],
        };
        config.hosts[0].endpoint = "http://taskrail.example/mcp".into();
        assert!(config.validate().is_err());
        config.hosts[0].endpoint = "https://user:secret@taskrail.example/mcp".into();
        assert!(config.validate().is_err());
    }

    #[test]
    fn allows_loopback_http_for_local_gateway() {
        let mut config = FleetConfig {
            version: 1,
            hosts: vec![host(false)],
        };
        config.hosts[0].endpoint = "http://127.0.0.1:8787/mcp".into();
        config.validate().unwrap();
    }

    #[test]
    fn writes_require_explicit_host_opt_in() {
        assert!(is_mutating_tool("taskrail_run_automation"));
        assert!(!is_mutating_tool("taskrail_overview"));
        assert!(is_mutating_integration_call(
            "taskrail_mole",
            &json!({"action":"clean"})
        ));
        assert!(!is_mutating_integration_call(
            "taskrail_mole",
            &json!({"action":"clean","dry_run":true})
        ));
        assert!(is_mutating_integration_call(
            "taskrail_rclone",
            &json!({"action":"sync"})
        ));
        assert!(!is_mutating_integration_call(
            "taskrail_rclone",
            &json!({"action":"sync","dry_run":true})
        ));
        let mut config = FleetConfig {
            version: 1,
            hosts: vec![host(false)],
        };
        config.hosts[0].allow_writes = true;
        assert!(config.validate().is_err());
    }

    #[test]
    fn checked_in_example_is_disabled_and_credential_free() {
        let config: FleetConfig =
            serde_yaml::from_str(include_str!("../../../examples/fleet.yaml")).unwrap();
        config.validate().unwrap();
        assert!(config.hosts.iter().all(|host| !host.enabled));
        assert!(config.hosts.iter().all(|host| !host.allow_writes));
    }

    #[tokio::test]
    async fn read_only_hosts_block_mutations_before_network() {
        let gateway = FleetGateway::from_config(FleetConfig {
            version: 1,
            hosts: vec![host(false)],
        })
        .unwrap();
        let error = gateway
            .call_tool("macbook", "taskrail_run_automation", json!({}))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("read-only"));
        let error = gateway
            .call_tool("macbook", "taskrail_mole", json!({"action":"clean"}))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("read-only"));
    }

    #[tokio::test]
    async fn routes_read_only_tool_to_named_localhost_host() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0_u8; 4096];
            let header_end = loop {
                let count = stream.read(&mut buffer).await.unwrap();
                assert!(count > 0, "mock server received an incomplete request");
                request.extend_from_slice(&buffer[..count]);
                if let Some(position) = request.windows(4).position(|window| window == b"\r\n\r\n")
                {
                    break position + 4;
                }
            };
            let headers = String::from_utf8_lossy(&request[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    line.strip_prefix("Content-Length:")
                        .or_else(|| line.strip_prefix("content-length:"))
                })
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            while request.len() < header_end + content_length {
                let count = stream.read(&mut buffer).await.unwrap();
                assert!(count > 0, "mock server received an incomplete request body");
                request.extend_from_slice(&buffer[..count]);
            }
            let request = String::from_utf8_lossy(&request);
            assert!(request.starts_with("POST /mcp HTTP/1.1"));
            assert!(request.contains("taskrail_overview"));

            let body = r#"{"jsonrpc":"2.0","id":"mock","result":{"content":[{"type":"text","text":"ok"}],"structuredContent":{"result":{"host":{"id":"remote-host"}}},"isError":false}}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        });

        let config = FleetConfig {
            version: 1,
            hosts: vec![FleetHost {
                id: "macbook".into(),
                label: "MacBook".into(),
                endpoint: format!("http://{address}/mcp"),
                token_env: None,
                enabled: true,
                allow_writes: false,
            }],
        };
        let gateway = FleetGateway::from_config(config).unwrap();
        let result = gateway.host_overview("macbook").await.unwrap();

        assert_eq!(result["host"]["id"], "remote-host");
        server.await.unwrap();
    }
}
