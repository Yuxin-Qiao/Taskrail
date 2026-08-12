use super::model::{
    ArtifactRef, Change, Finding, IntegrationId, IntegrationResult, IntegrationStatus, MetricValue,
    ProcessOutput,
};
use anyhow::Result;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    env,
    path::{Path, PathBuf},
};

pub const MAX_SAFE_TEXT: usize = 512;

pub fn resolve_executable(executable: &Path) -> Option<PathBuf> {
    if executable.components().count() > 1 {
        return executable.is_file().then(|| executable.to_path_buf());
    }
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .map(|directory| directory.join(executable))
        .find(|candidate| candidate.is_file())
}

pub fn first_line(bytes: &[u8]) -> Option<String> {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(ToOwned::to_owned)
}

pub fn safe_text(value: &str) -> String {
    let lower = value.to_ascii_lowercase();
    if [
        "sk-",
        "token=",
        "password=",
        "secret=",
        "api_key=",
        "authorization:",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
    {
        return "[REDACTED]".into();
    }
    let value = value.trim();
    if value.len() <= MAX_SAFE_TEXT {
        value.into()
    } else {
        let mut end = MAX_SAFE_TEXT;
        while end > 0 && !value.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &value[..end])
    }
}

pub fn parse_reported_bytes(value: &str) -> Option<f64> {
    let value = value.trim();
    let split = value.find(|character: char| {
        !(character.is_ascii_digit() || character == '.' || character == ',')
    })?;
    let number = value[..split].replace(',', "").parse::<f64>().ok()?;
    let unit = value[split..].trim().to_ascii_lowercase();
    let multiplier = match unit.as_str() {
        "b" => 1.0,
        "kb" | "kib" => 1024.0,
        "mb" | "mib" => 1024.0_f64.powi(2),
        "gb" | "gib" => 1024.0_f64.powi(3),
        "tb" | "tib" => 1024.0_f64.powi(4),
        _ => return None,
    };
    Some(number * multiplier)
}

pub fn json_lines(stdout: &str) -> Result<Vec<Value>> {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        anyhow::bail!("structured integration output is empty");
    }
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return Ok(match value {
            Value::Array(values) => values,
            value => vec![value],
        });
    }
    let mut values = Vec::new();
    let mut invalid = 0;
    for line in trimmed.lines().filter(|line| !line.trim().is_empty()) {
        match serde_json::from_str::<Value>(line) {
            Ok(value) => values.push(value),
            Err(_) => invalid += 1,
        }
    }
    if values.is_empty() {
        anyhow::bail!("structured integration output is not valid JSON");
    }
    if invalid > values.len() {
        anyhow::bail!("structured integration output is too malformed to parse");
    }
    Ok(values)
}

pub fn failed_result(
    integration: &IntegrationId,
    action: &str,
    output: &ProcessOutput,
    tool_name: &str,
) -> IntegrationResult {
    IntegrationResult {
        integration: integration.clone(),
        action: action.into(),
        status: IntegrationStatus::Failed,
        summary: format!(
            "{tool_name} {action} failed{}.",
            output
                .exit_code
                .map_or_else(String::new, |code| format!(" with exit code {code}"))
        ),
        metrics: BTreeMap::new(),
        findings: Vec::new(),
        changes: Vec::new(),
        artifacts: Vec::new(),
        raw_output_ref: None,
    }
}

pub fn metric(value: f64, unit: &str) -> MetricValue {
    MetricValue {
        value,
        unit: unit.into(),
    }
}

pub fn numeric(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_u64().map(|value| value as f64))
}

pub fn string_number(value: &Value) -> Option<f64> {
    numeric(value).or_else(|| value.as_str().and_then(parse_reported_bytes))
}

pub fn collect_error_findings(values: &[Value], source: &str) -> Vec<Finding> {
    values
        .iter()
        .filter_map(|value| {
            let message = value
                .get("error")
                .and_then(Value::as_str)
                .or_else(|| value.get("message").and_then(Value::as_str))?;
            if message.trim().is_empty() || !is_error_value(value) {
                return None;
            }
            Some(Finding {
                kind: "error".into(),
                title: format!("{source}: {}", safe_text(message)),
                severity: Some("high".into()),
                location: None,
                fingerprint: None,
            })
        })
        .collect()
}

fn is_error_value(value: &Value) -> bool {
    value.get("error").is_some()
        || value
            .get("level")
            .and_then(Value::as_str)
            .is_some_and(|level| level.eq_ignore_ascii_case("error"))
        || value
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|kind| kind.eq_ignore_ascii_case("error"))
}

#[allow(clippy::too_many_arguments)]
pub fn result(
    integration: &IntegrationId,
    action: &str,
    status: IntegrationStatus,
    summary: impl Into<String>,
    metrics: BTreeMap<String, MetricValue>,
    findings: Vec<Finding>,
    changes: Vec<Change>,
    artifacts: Vec<ArtifactRef>,
) -> IntegrationResult {
    IntegrationResult {
        integration: integration.clone(),
        action: action.into(),
        status,
        summary: summary.into(),
        metrics,
        findings,
        changes,
        artifacts,
        raw_output_ref: None,
    }
}
