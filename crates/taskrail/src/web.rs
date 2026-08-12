//! Local browser dashboard served by the Taskrail daemon.
//!
//! The dashboard intentionally lives behind the same loopback-only daemon as
//! the scheduler and JSON-RPC control plane.  It is a thin client: all state
//! and mutations go through the existing RPC request handler, so the CLI,
//! TUI, MCP adapter, and browser share one policy boundary.

use crate::rpc::{self, Request, Response};
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    net::SocketAddr,
    path::{Path, PathBuf},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
};

const INDEX_HTML: &str = include_str!("../gui/index.html");
const APP_JS: &str = include_str!("../gui/app.js");
const STYLES_CSS: &str = include_str!("../gui/styles.css");
const FAVICON_SVG: &str = include_str!("../gui/favicon.svg");
const MAX_HEADER_BYTES: usize = 32 * 1024;
const DASHBOARD_FALLBACK_PORTS: u16 = 10;

/// Serve the local dashboard and its management API on a loopback address.
pub async fn serve(bind: SocketAddr, registry_path: PathBuf) -> Result<()> {
    if !bind.ip().is_loopback() {
        anyhow::bail!("Taskrail dashboard must bind to a loopback address, got {bind}");
    }
    let (listener, actual_bind) = bind_dashboard_listener(bind).await?;
    let allowed_origins = dashboard_origins(actual_bind);
    if actual_bind == bind {
        eprintln!("Taskrail dashboard listening on http://{actual_bind}");
    } else {
        eprintln!(
            "Taskrail dashboard preferred address http://{bind} is unavailable; using http://{actual_bind}"
        );
    }
    loop {
        let (stream, peer) = listener.accept().await.context("accept dashboard client")?;
        let registry_path = registry_path.clone();
        let allowed_origins = allowed_origins.clone();
        tokio::spawn(async move {
            if let Err(error) = handle_connection(stream, &registry_path, &allowed_origins).await {
                eprintln!("dashboard client {peer} error: {error:#}");
            }
        });
    }
}

async fn bind_dashboard_listener(bind: SocketAddr) -> Result<(TcpListener, SocketAddr)> {
    let bindings = dashboard_bindings(bind);
    let mut errors = Vec::new();
    for candidate in bindings {
        match TcpListener::bind(candidate).await {
            Ok(listener) => {
                let actual_bind = listener.local_addr().with_context(|| {
                    format!("resolve Taskrail dashboard address at {candidate}")
                })?;
                return Ok((listener, actual_bind));
            }
            Err(error) => errors.push(format!("{candidate}: {error}")),
        }
    }
    anyhow::bail!("could not bind Taskrail dashboard; {}", errors.join("; "))
}

fn dashboard_bindings(bind: SocketAddr) -> Vec<SocketAddr> {
    let mut bindings = vec![bind];
    if bind.port() != 0 {
        for offset in 1..=DASHBOARD_FALLBACK_PORTS {
            if let Some(port) = bind.port().checked_add(offset) {
                bindings.push(SocketAddr::new(bind.ip(), port));
            }
        }
    }
    bindings
}

fn dashboard_origins(bind: SocketAddr) -> Vec<String> {
    let port = bind.port();
    let mut origins = vec![
        format!("http://{bind}"),
        format!("http://localhost:{port}"),
        format!("http://127.0.0.1:{port}"),
        format!("http://[::1]:{port}"),
    ];
    origins.sort();
    origins.dedup();
    origins
}

fn origin_is_allowed(method: &str, origin: Option<&str>, allowed_origins: &[String]) -> bool {
    method != "POST"
        || origin.is_some_and(|origin| allowed_origins.iter().any(|allowed| allowed == origin))
}

async fn handle_connection(
    stream: TcpStream,
    registry_path: &Path,
    allowed_origins: &[String],
) -> Result<()> {
    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    reader
        .read_line(&mut request_line)
        .await
        .context("read dashboard request line")?;
    if request_line.is_empty() {
        return Ok(());
    }
    let mut parts = request_line.split_whitespace();
    let method = parts
        .next()
        .context("dashboard request method is missing")?;
    let target = parts
        .next()
        .context("dashboard request target is missing")?;
    let version = parts.next().context("dashboard HTTP version is missing")?;
    if version != "HTTP/1.1" {
        return write_response(
            reader.into_inner(),
            400,
            "Bad Request",
            "application/json; charset=utf-8",
            br#"{"error":"only HTTP/1.1 is supported"}"#,
        )
        .await;
    }

    let mut headers = BTreeMap::new();
    let mut header_bytes = request_line.len();
    loop {
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .await
            .context("read dashboard header")?;
        header_bytes += line.len();
        if header_bytes > MAX_HEADER_BYTES {
            return write_response(
                reader.into_inner(),
                431,
                "Request Header Fields Too Large",
                "application/json; charset=utf-8",
                br#"{"error":"request headers exceed the limit"}"#,
            )
            .await;
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        let (name, value) = line
            .split_once(':')
            .context("dashboard header is missing a colon")?;
        headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_owned());
    }

    // The dashboard currently sends bodyless GET/POST requests.  Drain a
    // bounded body if a client supplies one so the connection cannot retain
    // unread bytes, but do not accept it as an alternate command channel.
    if let Some(length) = headers.get("content-length") {
        let length = length.parse::<usize>().context("invalid Content-Length")?;
        if length > 1024 * 1024 {
            return write_response(
                reader.into_inner(),
                413,
                "Payload Too Large",
                "application/json; charset=utf-8",
                br#"{"error":"request body exceeds the limit"}"#,
            )
            .await;
        }
        let mut body = vec![0; length];
        reader
            .read_exact(&mut body)
            .await
            .context("read dashboard body")?;
    }

    if !origin_is_allowed(
        method,
        headers.get("origin").map(String::as_str),
        allowed_origins,
    ) {
        return write_response(
            reader.into_inner(),
            403,
            "Forbidden",
            "application/json; charset=utf-8",
            br#"{"error":"dashboard write requests require a same-origin Origin header"}"#,
        )
        .await;
    }

    let (path, query) = split_target(target);
    let (status, content_type, body) = route(method, path, query, registry_path).await;
    write_response(
        reader.into_inner(),
        status,
        status_text(status),
        content_type,
        &body,
    )
    .await
}

fn split_target(target: &str) -> (&str, &str) {
    target
        .split_once('?')
        .map_or((target, ""), |(path, query)| (path, query))
}

async fn route(
    method: &str,
    path: &str,
    query: &str,
    registry_path: &Path,
) -> (u16, &'static str, Vec<u8>) {
    match (method, path) {
        ("GET", "/") | ("GET", "/index.html") => {
            return (
                200,
                "text/html; charset=utf-8",
                INDEX_HTML.as_bytes().to_vec(),
            );
        }
        ("GET", "/gui/app.js") => {
            return (
                200,
                "text/javascript; charset=utf-8",
                APP_JS.as_bytes().to_vec(),
            );
        }
        ("GET", "/gui/styles.css") => {
            return (
                200,
                "text/css; charset=utf-8",
                STYLES_CSS.as_bytes().to_vec(),
            );
        }
        ("GET", "/favicon.ico") => {
            return (
                200,
                "image/svg+xml; charset=utf-8",
                FAVICON_SVG.as_bytes().to_vec(),
            );
        }
        ("GET", "/healthz") => {
            return json_response(
                200,
                json!({
                    "status": "ok",
                    "service": "taskrail",
                    "version": env!("CARGO_PKG_VERSION"),
                    "dashboard": true,
                }),
            );
        }
        _ => {}
    }

    if !path.starts_with("/api/") {
        return json_response(404, json!({"error": "not found"}));
    }

    let (rpc_method, params) = match map_api_request(method, path, query) {
        Ok(value) => value,
        Err(error) => return json_response(400, json!({"error": error})),
    };
    let response = rpc::handle_request(
        Request {
            jsonrpc: "2.0".into(),
            id: Value::from(1),
            method: rpc_method,
            params,
        },
        registry_path,
    )
    .await;
    rpc_response(response)
}

fn map_api_request(method: &str, path: &str, query: &str) -> Result<(String, Value), String> {
    let query = parse_query(query);
    match (method, path) {
        ("GET", "/api/status") => Ok(("daemon.status".into(), json!({}))),
        ("GET", "/api/automations") => Ok(("automation.list".into(), json!({}))),
        ("GET", "/api/integrations") => Ok(("integration.list".into(), json!({}))),
        ("GET", "/api/approvals") => Ok((
            "approvals.list".into(),
            json!({"limit": query_limit(&query)}),
        )),
        ("GET", "/api/runs") => Ok(("runs.list".into(), json!({"limit": query_limit(&query)}))),
        ("GET", "/api/inbox") => Ok(("inbox.list".into(), json!({"limit": query_limit(&query)}))),
        ("GET", "/api/events") => Ok(("events.list".into(), json!({"limit": query_limit(&query)}))),
        ("GET", "/api/metrics") => Ok(("metrics.list".into(), json!({}))),
        ("GET", "/api/discovery") => Ok((
            "automation.discover".into(),
            json!({"source": query.get("source").map(String::as_str).unwrap_or("all")}),
        )),
        _ if method == "GET" && path.starts_with("/api/automations/") => {
            let id = path_suffix(path, "/api/automations/")?;
            Ok(("automation.inspect".into(), json!({"id": id})))
        }
        _ if method == "POST"
            && path.ends_with("/run")
            && path.starts_with("/api/automations/") =>
        {
            let id = action_id(path, "/api/automations/", "/run")?;
            Ok(("automation.run".into(), json!({"id": id})))
        }
        _ if method == "POST"
            && path.ends_with("/pause")
            && path.starts_with("/api/automations/") =>
        {
            let id = action_id(path, "/api/automations/", "/pause")?;
            Ok(("automation.pause".into(), json!({"id": id})))
        }
        _ if method == "POST"
            && path.ends_with("/resume")
            && path.starts_with("/api/automations/") =>
        {
            let id = action_id(path, "/api/automations/", "/resume")?;
            Ok(("automation.resume".into(), json!({"id": id})))
        }
        _ if method == "GET" && path.starts_with("/api/runs/") && path.ends_with("/logs") => {
            let id = action_id(path, "/api/runs/", "/logs")?;
            Ok(("run.logs".into(), json!({"run_id": id})))
        }
        _ if method == "POST" && path.starts_with("/api/runs/") && path.ends_with("/cancel") => {
            let id = action_id(path, "/api/runs/", "/cancel")?;
            Ok(("run.cancel".into(), json!({"run_id": id})))
        }
        _ if method == "POST"
            && path.starts_with("/api/approvals/")
            && path.ends_with("/approve") =>
        {
            let id = action_id(path, "/api/approvals/", "/approve")?;
            Ok(("approval.approve".into(), json!({"approval_id": id})))
        }
        _ if method == "POST"
            && path.starts_with("/api/approvals/")
            && path.ends_with("/reject") =>
        {
            let id = action_id(path, "/api/approvals/", "/reject")?;
            Ok(("approval.reject".into(), json!({"approval_id": id})))
        }
        _ => Err(format!("unsupported dashboard route: {method} {path}")),
    }
}

fn path_suffix(path: &str, prefix: &str) -> Result<String, String> {
    let raw = path
        .strip_prefix(prefix)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "route identifier is missing".to_owned())?;
    let decoded = percent_decode(raw)?;
    if decoded.is_empty() || decoded.contains('/') {
        return Err("route identifier is invalid".into());
    }
    Ok(decoded)
}

fn action_id(path: &str, prefix: &str, suffix: &str) -> Result<String, String> {
    let action_path = path
        .strip_suffix(suffix)
        .ok_or_else(|| "route action is missing".to_owned())?;
    path_suffix(action_path, prefix)
}

fn parse_query(query: &str) -> BTreeMap<String, String> {
    query
        .split('&')
        .filter(|part| !part.is_empty())
        .filter_map(|part| {
            let (key, value) = part.split_once('=').unwrap_or((part, ""));
            Some((percent_decode(key).ok()?, percent_decode(value).ok()?))
        })
        .collect()
}

fn query_limit(query: &BTreeMap<String, String>) -> usize {
    query
        .get("limit")
        .and_then(|value| value.parse::<usize>().ok())
        .map(|value| value.clamp(1, 500))
        .unwrap_or(100)
}

fn percent_decode(value: &str) -> Result<String, String> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err("invalid percent encoding".into());
            }
            let high =
                hex(bytes[index + 1]).ok_or_else(|| "invalid percent encoding".to_owned())?;
            let low = hex(bytes[index + 2]).ok_or_else(|| "invalid percent encoding".to_owned())?;
            output.push(high * 16 + low);
            index += 3;
        } else if bytes[index] == b'+' {
            output.push(b' ');
            index += 1;
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(output).map_err(|_| "percent-decoded value is not UTF-8".into())
}

fn hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn rpc_response(response: Response) -> (u16, &'static str, Vec<u8>) {
    match (response.result, response.error) {
        (Some(result), None) => json_response(200, result),
        (_, Some(error)) => {
            let status = if error.code == -32602 || error.code == -32601 {
                400
            } else {
                500
            };
            json_response(status, json!({"error": error.message, "code": error.code}))
        }
        _ => json_response(500, json!({"error": "invalid daemon response"})),
    }
}

fn json_response(status: u16, body: Value) -> (u16, &'static str, Vec<u8>) {
    (
        status,
        "application/json; charset=utf-8",
        serde_json::to_vec(&body)
            .unwrap_or_else(|_| b"{\"error\":\"serialization failed\"}".to_vec()),
    )
}

async fn write_response(
    mut stream: TcpStream,
    status: u16,
    reason: &str,
    content_type: &str,
    body: &[u8],
) -> Result<()> {
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nContent-Security-Policy: default-src 'self'; style-src 'self'; script-src 'self'; connect-src 'self'; object-src 'none'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(header.as_bytes())
        .await
        .context("write dashboard response headers")?;
    stream
        .write_all(body)
        .await
        .context("write dashboard response body")?;
    Ok(())
}

fn status_text(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        413 => "Payload Too Large",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        _ => "Error",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DASHBOARD_FALLBACK_PORTS, dashboard_bindings, dashboard_origins, map_api_request,
        origin_is_allowed, percent_decode,
    };

    #[test]
    fn dashboard_bindings_include_a_bounded_loopback_fallback_range() {
        let bindings = dashboard_bindings("127.0.0.1:10100".parse().unwrap());
        assert_eq!(bindings.first().unwrap().to_string(), "127.0.0.1:10100");
        assert_eq!(bindings.len(), usize::from(DASHBOARD_FALLBACK_PORTS) + 1);
        assert_eq!(bindings.last().unwrap().to_string(), "127.0.0.1:10110");
    }

    #[test]
    fn maps_dashboard_routes_to_existing_rpc_methods() {
        assert_eq!(
            map_api_request("POST", "/api/automations/hello/run", "").unwrap(),
            ("automation.run".into(), serde_json::json!({"id": "hello"}))
        );
        assert_eq!(
            map_api_request("GET", "/api/runs/abc/logs", "").unwrap(),
            ("run.logs".into(), serde_json::json!({"run_id": "abc"}))
        );
    }

    #[test]
    fn percent_decodes_utf8_and_rejects_invalid_values() {
        assert_eq!(percent_decode("hello%20world").unwrap(), "hello world");
        assert!(percent_decode("bad%ZZ").is_err());
    }

    #[test]
    fn dashboard_write_requests_require_a_loopback_same_origin() {
        let origins = dashboard_origins("127.0.0.1:10100".parse().unwrap());
        assert!(origin_is_allowed("GET", None, &origins));
        assert!(origin_is_allowed(
            "POST",
            Some("http://127.0.0.1:10100"),
            &origins
        ));
        assert!(origin_is_allowed(
            "POST",
            Some("http://localhost:10100"),
            &origins
        ));
        assert!(!origin_is_allowed("POST", None, &origins));
        assert!(!origin_is_allowed(
            "POST",
            Some("https://evil.example"),
            &origins
        ));
    }
}
