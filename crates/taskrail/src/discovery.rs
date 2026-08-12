use crate::core::{CommandSpec, DiscoveredSource, Trigger, fingerprint_bytes};
use anyhow::{Context, Result};
use plist::Value;
use serde::{Deserialize, Serialize};
use std::process::Command;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

pub trait DiscoveryProvider {
    fn name(&self) -> &'static str;
    fn scan(&self) -> Result<Vec<DiscoveredSource>>;
}

#[derive(Debug, Clone)]
pub struct NativeDiscoverySnapshot {
    pub sources: Vec<DiscoveredSource>,
    /// Only providers listed here were queried authoritatively. An unavailable
    /// provider is excluded so its old observations are not falsely reported
    /// as missing.
    pub complete_providers: BTreeSet<String>,
}

#[derive(Debug, Clone)]
pub struct LaunchdProvider {
    pub roots: Vec<PathBuf>,
    /// Fixture-friendly loaded state keyed by launchd label.
    pub runtime_states: Option<BTreeMap<String, bool>>,
    pub user_domain: Option<String>,
    pub home: Option<PathBuf>,
}

impl Default for LaunchdProvider {
    fn default() -> Self {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let mut roots = Vec::new();
        if let Some(home) = home {
            roots.push(home.join("Library/LaunchAgents"));
        }
        roots.extend([
            PathBuf::from("/Library/LaunchAgents"),
            PathBuf::from("/Library/LaunchDaemons"),
        ]);
        Self {
            roots,
            runtime_states: None,
            user_domain: None,
            home: None,
        }
    }
}

impl LaunchdProvider {
    fn user_agent_loaded(&self, source: &DiscoveredSource) -> Option<bool> {
        let path = source.path.as_deref()?;
        let home = self
            .home
            .clone()
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))?;
        let user_root = home.join("Library").join("LaunchAgents");
        let path = path.canonicalize().ok()?;
        let user_root = user_root.canonicalize().ok()?;
        if !path.starts_with(user_root) {
            return None;
        }
        if let Some(states) = &self.runtime_states {
            return Some(states.get(&source.native_id).copied().unwrap_or(false));
        }
        let domain = self.user_domain.clone().or_else(|| {
            let output = Command::new("id").arg("-u").output().ok()?;
            if !output.status.success() {
                return None;
            }
            Some(format!(
                "gui/{}",
                String::from_utf8(output.stdout).ok()?.trim()
            ))
        })?;
        let target = format!("{domain}/{}", source.native_id);
        Command::new("launchctl")
            .args(["print", &target])
            .output()
            .ok()
            .map(|output| output.status.success())
    }
}

impl DiscoveryProvider for LaunchdProvider {
    fn name(&self) -> &'static str {
        "launchd"
    }

    fn scan(&self) -> Result<Vec<DiscoveredSource>> {
        let mut discovered = Vec::new();
        for root in &self.roots {
            if !root.is_dir() {
                continue;
            }
            for entry in
                std::fs::read_dir(root).with_context(|| format!("read {}", root.display()))?
            {
                let path = entry?.path();
                if path.extension().and_then(|ext| ext.to_str()) != Some("plist") {
                    continue;
                }
                if let Some(source) = parse_launchd_plist(&path)? {
                    let mut source = source;
                    if let Some(loaded) = self.user_agent_loaded(&source) {
                        source.enabled = loaded;
                    }
                    discovered.push(source);
                }
            }
        }
        Ok(discovered)
    }
}

pub fn parse_launchd_plist(path: &Path) -> Result<Option<DiscoveredSource>> {
    let bytes =
        std::fs::read(path).with_context(|| format!("read launchd plist {}", path.display()))?;
    let value = Value::from_reader_xml(bytes.as_slice())
        .with_context(|| format!("parse launchd plist {}", path.display()))?;
    let dict = match value {
        Value::Dictionary(dict) => dict,
        _ => return Ok(None),
    };
    let native_id = dict
        .get("Label")
        .and_then(Value::as_string)
        .map(str::to_owned)
        .or_else(|| {
            path.file_stem()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "unknown".to_owned());
    let command = launchd_command(&dict);
    let trigger = if let Some(seconds) = dict
        .get("StartInterval")
        .and_then(Value::as_unsigned_integer)
    {
        Trigger::Interval { seconds }
    } else if let Some(calendar) = dict.get("StartCalendarInterval") {
        Trigger::Cron {
            expression: calendar_to_cron(calendar),
            timezone: "local".into(),
        }
    } else {
        Trigger::Manual
    };
    Ok(Some(DiscoveredSource {
        source_id: format!("launchd:{native_id}"),
        provider: "launchd".into(),
        native_id,
        path: Some(path.to_path_buf()),
        enabled: true,
        kind: if dict.contains_key("KeepAlive") {
            "service"
        } else {
            "task"
        }
        .into(),
        fingerprint: fingerprint_bytes(&std::fs::read(path)?),
        command,
        trigger,
        raw: String::from_utf8_lossy(&bytes).into_owned(),
    }))
}

fn launchd_command(dict: &plist::Dictionary) -> Option<CommandSpec> {
    let program = dict
        .get("Program")
        .and_then(Value::as_string)
        .map(PathBuf::from);
    let args = dict
        .get("ProgramArguments")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_string)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        });
    match (program, args) {
        (Some(executable), Some(mut args)) => Some(CommandSpec {
            executable,
            args: {
                if !args.is_empty() {
                    args.remove(0);
                }
                args
            },
            cwd: None,
            env: Default::default(),
            shell: false,
        }),
        (None, Some(mut args)) if !args.is_empty() => Some(CommandSpec {
            executable: PathBuf::from(args.remove(0)),
            args,
            cwd: None,
            env: Default::default(),
            shell: false,
        }),
        _ => None,
    }
}

fn calendar_to_cron(value: &Value) -> String {
    let Some(dict) = value.as_dictionary() else {
        return "* * * * *".into();
    };
    let minute = dict
        .get("Minute")
        .and_then(Value::as_unsigned_integer)
        .map_or("*".into(), |v| v.to_string());
    let hour = dict
        .get("Hour")
        .and_then(Value::as_unsigned_integer)
        .map_or("*".into(), |v| v.to_string());
    let day = dict
        .get("Day")
        .and_then(Value::as_unsigned_integer)
        .map_or("*".into(), |v| v.to_string());
    let month = dict
        .get("Month")
        .and_then(Value::as_unsigned_integer)
        .map_or("*".into(), |v| v.to_string());
    let weekday = dict
        .get("Weekday")
        .and_then(Value::as_unsigned_integer)
        .map_or("*".into(), |v| v.to_string());
    format!("{minute} {hour} {day} {month} {weekday}")
}

#[derive(Debug, Clone, Default)]
pub struct CronProvider {
    pub crontab: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct SystemdProvider {
    /// Fixture-friendly unit-file listing. When absent, `systemctl --user` is queried.
    pub unit_list: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HomebrewService {
    pub name: String,
    pub status: String,
    pub user: Option<String>,
    pub file: Option<PathBuf>,
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone)]
pub struct HomebrewProvider {
    /// Fixture-friendly output from `brew services list --json`.
    pub listing: Option<String>,
    pub brew_path: PathBuf,
}

impl Default for HomebrewProvider {
    fn default() -> Self {
        Self {
            listing: None,
            brew_path: PathBuf::from("brew"),
        }
    }
}

impl HomebrewProvider {
    pub fn services(&self) -> Result<Vec<HomebrewService>> {
        let listing = match &self.listing {
            Some(listing) => listing.clone(),
            None => {
                let output = match Command::new(&self.brew_path)
                    .args(["services", "list", "--json"])
                    .output()
                {
                    Ok(output) => output,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        return Ok(Vec::new());
                    }
                    Err(error) => {
                        return Err(error).context("run brew services list --json");
                    }
                };
                if !output.status.success() {
                    anyhow::bail!(
                        "brew services list --json failed: {}",
                        String::from_utf8_lossy(&output.stderr).trim()
                    );
                }
                String::from_utf8(output.stdout).context("brew services output is not UTF-8")?
            }
        };
        serde_json::from_str(&listing).context("parse brew services JSON")
    }
}

impl DiscoveryProvider for HomebrewProvider {
    fn name(&self) -> &'static str {
        "homebrew"
    }

    fn scan(&self) -> Result<Vec<DiscoveredSource>> {
        self.services()?.into_iter().map(homebrew_source).collect()
    }
}

/// Read-only discovery for Windows Task Scheduler.
///
/// The provider consumes `schtasks.exe /Query /FO CSV /V` output on Windows.
/// A fixture can be supplied in tests and in deterministic review runs. Task
/// Scheduler definitions are observations only; adoption is intentionally not
/// implemented for this provider.
#[derive(Debug, Clone)]
pub struct TaskSchedulerProvider {
    /// Fixture-friendly CSV output from `schtasks.exe /Query /FO CSV /V`.
    pub listing: Option<String>,
    pub executable: PathBuf,
}

impl Default for TaskSchedulerProvider {
    fn default() -> Self {
        Self {
            listing: None,
            executable: if cfg!(windows) {
                PathBuf::from("schtasks.exe")
            } else {
                PathBuf::from("schtasks")
            },
        }
    }
}

impl DiscoveryProvider for TaskSchedulerProvider {
    fn name(&self) -> &'static str {
        "task-scheduler"
    }

    fn scan(&self) -> Result<Vec<DiscoveredSource>> {
        let listing = match &self.listing {
            Some(listing) => listing.clone(),
            None => {
                #[cfg(not(windows))]
                {
                    return Ok(Vec::new());
                }
                #[cfg(windows)]
                {
                    let output = match Command::new(&self.executable)
                        .args(["/Query", "/FO", "CSV", "/V"])
                        .output()
                    {
                        Ok(output) => output,
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                            return Ok(Vec::new());
                        }
                        Err(error) => return Err(error).context("run schtasks.exe query"),
                    };
                    if !output.status.success() {
                        anyhow::bail!(
                            "schtasks.exe query failed: {}",
                            String::from_utf8_lossy(&output.stderr).trim()
                        );
                    }
                    String::from_utf8_lossy(&output.stdout).into_owned()
                }
            }
        };
        parse_task_scheduler_csv(&listing)
    }
}

pub fn parse_task_scheduler_csv(content: &str) -> Result<Vec<DiscoveredSource>> {
    let mut lines = content.lines().filter(|line| !line.trim().is_empty());
    let Some(header_line) = lines.next() else {
        return Ok(Vec::new());
    };
    let headers = parse_csv_record(header_line)
        .into_iter()
        .map(|header| header.trim_start_matches('\u{feff}').to_ascii_lowercase())
        .collect::<Vec<_>>();
    let task_name_index = find_csv_column(&headers, &["taskname", "task name"]).unwrap_or(0);
    let status_index = find_csv_column(&headers, &["status", "scheduled task state"]);
    let state_index = find_csv_column(&headers, &["scheduled task state", "state"]);
    let command_index = find_csv_column(&headers, &["tasktorun", "task to run", "command"]);
    let mut result = Vec::new();
    for line in lines {
        let fields = parse_csv_record(line);
        let Some(task_name) = fields.get(task_name_index).map(|value| value.trim()) else {
            continue;
        };
        if task_name.is_empty() || task_name.eq_ignore_ascii_case("taskname") {
            continue;
        }
        let native_id = task_name.trim_start_matches('\\').to_owned();
        let status = status_index
            .and_then(|index| fields.get(index))
            .map(String::as_str)
            .unwrap_or_default();
        let state = state_index
            .and_then(|index| fields.get(index))
            .map(String::as_str)
            .unwrap_or_default();
        let enabled = ![status, state]
            .iter()
            .any(|value| value.trim().eq_ignore_ascii_case("disabled"));
        let command = command_index
            .and_then(|index| fields.get(index))
            .and_then(|value| parse_windows_command(value));
        result.push(DiscoveredSource {
            source_id: format!("task-scheduler:{native_id}"),
            provider: "task-scheduler".into(),
            native_id,
            path: None,
            enabled,
            kind: "task".into(),
            fingerprint: fingerprint_bytes(line.as_bytes()),
            command,
            trigger: Trigger::Manual,
            raw: line.to_owned(),
        });
    }
    Ok(result)
}

fn find_csv_column(headers: &[String], names: &[&str]) -> Option<usize> {
    headers.iter().position(|header| {
        names.iter().any(|name| {
            let name = name.to_ascii_lowercase();
            header == &name || header.replace([' ', '_'], "") == name.replace([' ', '_'], "")
        })
    })
}

fn parse_csv_record(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '"' if quoted && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => {
                fields.push(std::mem::take(&mut field));
            }
            _ => field.push(character),
        }
    }
    fields.push(field);
    fields
}

fn parse_windows_command(value: &str) -> Option<CommandSpec> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    for character in value.trim().chars() {
        match character {
            '"' => quoted = !quoted,
            character if character.is_whitespace() && !quoted => {
                if !field.is_empty() {
                    fields.push(std::mem::take(&mut field));
                }
            }
            _ => field.push(character),
        }
    }
    if !field.is_empty() {
        fields.push(field);
    }
    let (executable, args) = fields.split_first()?;
    if executable.eq_ignore_ascii_case("n/a") {
        return None;
    }
    Some(CommandSpec {
        executable: PathBuf::from(executable),
        args: args.to_vec(),
        cwd: None,
        env: Default::default(),
        shell: false,
    })
}

fn homebrew_source(service: HomebrewService) -> Result<DiscoveredSource> {
    let raw = serde_json::to_string(&service)?;
    if service
        .file
        .as_deref()
        .and_then(|path| path.extension())
        .and_then(|extension| extension.to_str())
        == Some("plist")
    {
        let path = service
            .file
            .as_deref()
            .expect("extension implies file path");
        if let Ok(Some(mut native)) = parse_launchd_plist(path) {
            native.raw = format!("{}\n# homebrew-service: {}", native.raw, raw);
            return Ok(native);
        }
    }
    Ok(DiscoveredSource {
        source_id: format!("homebrew:{}", service.name),
        provider: "homebrew".into(),
        native_id: service.name,
        path: service.file,
        enabled: service.status == "started",
        kind: "service".into(),
        fingerprint: fingerprint_bytes(raw.as_bytes()),
        command: None,
        trigger: Trigger::Manual,
        raw,
    })
}

pub fn same_native_path(left: Option<&Path>, right: Option<&Path>) -> bool {
    let (Some(left), Some(right)) = (left, right) else {
        return false;
    };
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

/// Enrich matching launchd entities and return only Homebrew services without
/// a matching native plist. The caller can persist the returned sources while
/// keeping matched services under their canonical launchd identity.
pub fn merge_homebrew_sources(
    native_sources: &mut [DiscoveredSource],
    homebrew_sources: Vec<DiscoveredSource>,
) -> Vec<DiscoveredSource> {
    let mut unmatched = Vec::new();
    for homebrew in homebrew_sources {
        let Some(native) = native_sources
            .iter_mut()
            .find(|native| same_native_path(native.path.as_deref(), homebrew.path.as_deref()))
        else {
            unmatched.push(homebrew);
            continue;
        };
        if homebrew.provider != "launchd" {
            native.raw = format!("{}\n# homebrew-service: {}", native.raw, homebrew.raw);
        }
    }
    unmatched
}

/// Scan every supported native scheduler without mutating native definitions.
///
/// This is shared by the CLI, the daemon supervisor, and the MCP/RPC surface so
/// each entry point observes the same source set and Homebrew/launchd identity
/// reconciliation rules.
pub fn scan_native_sources(source: &str) -> Result<Vec<DiscoveredSource>> {
    if !matches!(source, "all" | "launchd" | "cron" | "systemd" | "homebrew") {
        anyhow::bail!(
            "unknown native source {source}; expected all, launchd, cron, systemd, or homebrew"
        );
    }

    let mut discovered = Vec::new();
    if matches!(source, "all" | "launchd") {
        discovered.extend(LaunchdProvider::default().scan()?);
    }
    if matches!(source, "all" | "cron") {
        discovered.extend(CronProvider::default().scan()?);
    }
    if matches!(source, "all" | "systemd") {
        discovered.extend(SystemdProvider::default().scan()?);
    }
    if matches!(source, "all" | "homebrew") {
        let homebrew = HomebrewProvider::default().scan()?;
        if source == "all" {
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
    Ok(discovered)
}

/// Return a complete native snapshot plus the providers that were actually
/// queried authoritatively on this host. Missing executables or unavailable
/// user managers are deliberately not considered complete: an unavailable
/// provider must never make old observations look deleted.
pub fn scan_native_snapshot(source: &str) -> Result<NativeDiscoverySnapshot> {
    let sources = scan_native_sources(source)?;
    let mut complete_providers = BTreeSet::new();
    let requested = |provider: &str| source == "all" || source == provider;

    if requested("launchd") && cfg!(target_os = "macos") && executable_available("launchctl") {
        complete_providers.insert("launchd".into());
    }
    if requested("cron")
        && cfg!(any(target_os = "macos", target_os = "linux"))
        && executable_available("crontab")
    {
        complete_providers.insert("cron".into());
    }
    if requested("systemd") && cfg!(target_os = "linux") && systemd_user_manager_available() {
        complete_providers.insert("systemd".into());
    }
    if requested("homebrew")
        && cfg!(any(target_os = "macos", target_os = "linux"))
        && executable_available("brew")
    {
        complete_providers.insert("homebrew".into());
    }

    Ok(NativeDiscoverySnapshot {
        sources,
        complete_providers,
    })
}

fn executable_available(name: &str) -> bool {
    if name.contains(std::path::MAIN_SEPARATOR) {
        return Path::new(name).is_file();
    }
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .map(|directory| directory.join(name))
        .any(|candidate| candidate.is_file())
}

fn systemd_user_manager_available() -> bool {
    let Ok(output) = Command::new("systemctl")
        .args([
            "--user",
            "list-unit-files",
            "--type=service",
            "--no-legend",
            "--no-pager",
        ])
        .output()
    else {
        return false;
    };
    output.status.success()
}

impl DiscoveryProvider for SystemdProvider {
    fn name(&self) -> &'static str {
        "systemd"
    }

    fn scan(&self) -> Result<Vec<DiscoveredSource>> {
        #[cfg(not(target_os = "linux"))]
        if self.unit_list.is_none() {
            return Ok(Vec::new());
        }
        let listing = match &self.unit_list {
            Some(listing) => listing.clone(),
            None => {
                let output = match Command::new("systemctl")
                    .args([
                        "--user",
                        "list-unit-files",
                        "--type=service",
                        "--no-legend",
                        "--no-pager",
                    ])
                    .output()
                {
                    Ok(output) => output,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        return Ok(Vec::new());
                    }
                    Err(error) => {
                        return Err(error).context("run systemctl --user list-unit-files");
                    }
                };
                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    if systemd_user_manager_unavailable(&stderr) {
                        return Ok(Vec::new());
                    }
                    anyhow::bail!("systemctl --user list-unit-files failed: {}", stderr.trim());
                }
                String::from_utf8(output.stdout).context("systemctl unit listing is not UTF-8")?
            }
        };
        let mut discovered = Vec::new();
        for line in listing.lines() {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            let Some(unit) = fields.first().filter(|unit| unit.ends_with(".service")) else {
                continue;
            };
            let state = fields.get(1).copied().unwrap_or("unknown");
            let raw = if self.unit_list.is_some() {
                line.to_owned()
            } else {
                systemd_show(unit).unwrap_or_else(|_| line.to_owned())
            };
            let properties = parse_systemd_properties(&raw);
            let command = properties
                .get("ExecStart")
                .and_then(|value| parse_systemd_command(value));
            let trigger = properties
                .get("OnUnitActiveSec")
                .and_then(|value| parse_systemd_duration(value))
                .map_or(Trigger::Manual, |seconds| Trigger::Interval { seconds });
            let path = properties
                .get("FragmentPath")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from);
            discovered.push(DiscoveredSource {
                source_id: format!("systemd:user:{unit}"),
                provider: "systemd".into(),
                native_id: (*unit).into(),
                path,
                enabled: matches!(state, "enabled" | "static" | "alias"),
                kind: "service".into(),
                fingerprint: fingerprint_bytes(raw.as_bytes()),
                command,
                trigger,
                raw,
            });
        }
        Ok(discovered)
    }
}

fn systemd_user_manager_unavailable(stderr: &str) -> bool {
    let stderr = stderr.to_ascii_lowercase();
    stderr.contains("failed to connect to bus")
        || stderr.contains("failed to connect to user bus")
        || stderr.contains("no medium found")
}

fn systemd_show(unit: &str) -> Result<String> {
    let output = Command::new("systemctl")
        .args([
            "--user",
            "show",
            unit,
            "--no-pager",
            "--property=FragmentPath,ExecStart,OnUnitActiveSec,OnCalendar,UnitFileState,ActiveState",
        ])
        .output()
        .with_context(|| format!("show systemd unit {unit}"))?;
    if !output.status.success() {
        anyhow::bail!(
            "systemctl show {unit} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    String::from_utf8(output.stdout).context("systemctl unit properties are not UTF-8")
}

fn parse_systemd_properties(raw: &str) -> std::collections::BTreeMap<String, String> {
    raw.lines()
        .filter_map(|line| {
            let (key, value) = line.split_once('=')?;
            Some((key.to_owned(), value.to_owned()))
        })
        .collect()
}

fn parse_systemd_command(value: &str) -> Option<CommandSpec> {
    let value = value.trim();
    if value.is_empty() || value.starts_with("{ path=NULL") {
        return None;
    }
    let value = value.strip_prefix("{ path=").unwrap_or(value);
    let value = value.split(';').next()?.trim();
    let parts = value
        .split_whitespace()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let (executable, args) = parts.split_first()?;
    Some(CommandSpec {
        executable: PathBuf::from(executable),
        args: args.to_vec(),
        cwd: None,
        env: Default::default(),
        shell: false,
    })
}

fn parse_systemd_duration(value: &str) -> Option<u64> {
    let value = value.trim();
    if let Some(value) = value.strip_suffix("min") {
        return value.parse::<u64>().ok()?.checked_mul(60);
    }
    if let Some(value) = value.strip_suffix("h") {
        return value.parse::<u64>().ok()?.checked_mul(60 * 60);
    }
    if let Some(value) = value.strip_suffix("s") {
        return value.parse::<u64>().ok();
    }
    value.parse::<u64>().ok()
}

impl DiscoveryProvider for CronProvider {
    fn name(&self) -> &'static str {
        "cron"
    }

    fn scan(&self) -> Result<Vec<DiscoveredSource>> {
        let content = match &self.crontab {
            Some(content) => content.clone(),
            None => {
                let output = match Command::new("crontab").arg("-l").output() {
                    Ok(output) => output,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        return Ok(Vec::new());
                    }
                    Err(error) => return Err(error).context("run crontab -l"),
                };
                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    if stderr.contains("no crontab") {
                        return Ok(Vec::new());
                    }
                    anyhow::bail!("crontab -l failed: {}", stderr.trim());
                }
                String::from_utf8(output.stdout).context("crontab output is not UTF-8")?
            }
        };
        parse_crontab(&content)
    }
}

pub fn parse_crontab(content: &str) -> Result<Vec<DiscoveredSource>> {
    let mut result = Vec::new();
    for (index, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('@') {
            continue;
        }
        let fields = trimmed.split_whitespace().collect::<Vec<_>>();
        if fields.len() < 6 || fields[..5].iter().any(|field| field.contains('=')) {
            continue;
        }
        let command_text = fields[5..].join(" ");
        let command = parse_cron_command(&command_text);
        let native_id = format!("line-{}", index + 1);
        result.push(DiscoveredSource {
            source_id: format!("cron:{native_id}"),
            provider: "cron".into(),
            native_id,
            path: None,
            enabled: true,
            kind: "task".into(),
            fingerprint: fingerprint_bytes(trimmed.as_bytes()),
            command,
            trigger: Trigger::Cron {
                expression: fields[..5].join(" "),
                timezone: "local".into(),
            },
            raw: line.to_owned(),
        });
    }
    Ok(result)
}

fn parse_cron_command(value: &str) -> Option<CommandSpec> {
    let parts = value
        .split_whitespace()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let (executable, args) = parts.split_first()?;
    let executable = executable.trim_matches(['\'', '"']);
    Some(CommandSpec {
        executable: PathBuf::from(executable),
        args: args.to_vec(),
        cwd: None,
        env: Default::default(),
        shell: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use tempfile::tempdir;

    #[test]
    fn parses_cron_without_scanning_path() {
        let sources = parse_crontab(
            "SHELL=/bin/zsh\n# comment\n0 3 * * 0 /usr/local/bin/mo clean --dry-run\n",
        )
        .unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(
            sources[0].trigger,
            Trigger::Cron {
                expression: "0 3 * * 0".into(),
                timezone: "local".into()
            }
        );
        assert_eq!(
            sources[0].command.as_ref().unwrap().executable,
            PathBuf::from("/usr/local/bin/mo")
        );
        assert!(!sources[0].command.as_ref().unwrap().shell);
    }

    #[test]
    fn parses_launchd_program_arguments() {
        let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>Label</key><string>com.example.demo</string><key>ProgramArguments</key><array><string>/bin/echo</string><string>hello</string></array><key>StartInterval</key><integer>60</integer></dict></plist>"#;
        let value = Value::from_reader_xml(Cursor::new(xml)).unwrap();
        let dict = value.as_dictionary().unwrap();
        let command = launchd_command(dict).unwrap();
        assert_eq!(command.executable, PathBuf::from("/bin/echo"));
        assert_eq!(command.args, ["hello"]);
    }

    #[test]
    fn parses_systemd_user_service_fixture_without_running_systemctl() {
        let provider = SystemdProvider {
            unit_list: Some("auto-worker.service enabled\nnot-a-service.socket enabled\n".into()),
        };
        let sources = provider.scan().unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].source_id, "systemd:user:auto-worker.service");
        assert!(sources[0].enabled);
        assert!(sources[0].command.is_none());
    }

    #[test]
    fn parses_systemd_properties_and_interval() {
        let raw = "FragmentPath=/home/me/.config/systemd/user/auto.service\nExecStart=/usr/bin/auto --daemon\nOnUnitActiveSec=5min\nUnitFileState=enabled\n";
        let properties = parse_systemd_properties(raw);
        let command = parse_systemd_command(properties.get("ExecStart").unwrap()).unwrap();
        assert_eq!(command.executable, PathBuf::from("/usr/bin/auto"));
        assert_eq!(command.args, ["--daemon"]);
        assert_eq!(
            parse_systemd_duration(properties.get("OnUnitActiveSec").unwrap()),
            Some(300)
        );
    }

    #[test]
    fn parses_windows_task_scheduler_csv_as_read_only_observations() {
        let csv = r#""TaskName","Status","Task To Run","Scheduled Task State"
"\Taskrail\Daily","Ready","""C:\Program Files\Taskrail\taskrail.exe"" scan --source task-scheduler","Enabled"
"\Taskrail\Disabled","Disabled","C:\disabled.exe","Disabled"
"#;
        let sources = parse_task_scheduler_csv(csv).unwrap();
        assert_eq!(sources.len(), 2);
        assert_eq!(sources[0].source_id, r"task-scheduler:Taskrail\Daily");
        assert!(sources[0].enabled);
        assert_eq!(
            sources[0].command.as_ref().unwrap().executable,
            PathBuf::from(r"C:\Program Files\Taskrail\taskrail.exe")
        );
        assert!(!sources[1].enabled);
    }

    #[test]
    fn missing_systemd_user_manager_is_an_empty_observation() {
        assert!(systemd_user_manager_unavailable(
            "Failed to connect to bus: No medium found"
        ));
        assert!(!systemd_user_manager_unavailable("permission denied"));
    }

    #[test]
    fn parses_homebrew_services_json_as_observation_only_sources() {
        let provider = HomebrewProvider {
            listing: Some(
                r#"[
                  {"name":"redis","status":"started","user":"me","file":"/tmp/homebrew.mxcl.redis.plist","exit_code":null},
                  {"name":"mysql","status":"none","user":null,"file":"/tmp/homebrew.mxcl.mysql.plist","exit_code":1}
                ]"#
                .into(),
            ),
            brew_path: PathBuf::from("brew"),
        };
        let sources = provider.scan().unwrap();
        assert_eq!(sources.len(), 2);
        assert_eq!(sources[0].source_id, "homebrew:redis");
        assert!(sources[0].enabled);
        assert_eq!(sources[0].kind, "service");
        assert!(sources[0].command.is_none());
        assert!(!sources[1].enabled);
    }

    #[test]
    fn homebrew_plist_matching_enriches_launchd_without_creating_duplicate() {
        let mut launchd = vec![DiscoveredSource {
            source_id: "launchd:homebrew.mxcl.redis".into(),
            provider: "launchd".into(),
            native_id: "homebrew.mxcl.redis".into(),
            path: Some(PathBuf::from("/tmp/homebrew.mxcl.redis.plist")),
            enabled: true,
            kind: "service".into(),
            fingerprint: "sha256:launchd".into(),
            command: Some(CommandSpec::argv(
                "/opt/homebrew/bin/redis-server",
                Vec::<String>::new(),
            )),
            trigger: Trigger::Manual,
            raw: "<plist />".into(),
        }];
        let homebrew = HomebrewProvider {
            listing: Some(
                r#"[{"name":"redis","status":"started","user":"me","file":"/tmp/homebrew.mxcl.redis.plist","exit_code":null}]"#
                    .into(),
            ),
            brew_path: PathBuf::from("brew"),
        }
        .scan()
        .unwrap();
        let unmatched = merge_homebrew_sources(&mut launchd, homebrew);
        assert!(unmatched.is_empty());
        assert_eq!(launchd.len(), 1);
        assert_eq!(launchd[0].source_id, "launchd:homebrew.mxcl.redis");
        assert!(launchd[0].raw.contains("homebrew-service"));
    }

    #[test]
    fn homebrew_provider_reuses_launchd_identity_when_plist_exists() {
        let directory = tempdir().unwrap();
        let plist = directory.path().join("homebrew.mxcl.redis.plist");
        std::fs::write(
            &plist,
            br#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>Label</key><string>homebrew.mxcl.redis</string><key>ProgramArguments</key><array><string>/opt/homebrew/bin/redis-server</string></array></dict></plist>"#,
        )
        .unwrap();
        let provider = HomebrewProvider {
            listing: Some(
                serde_json::json!([{
                    "name": "redis",
                    "status": "started",
                    "user": "me",
                    "file": plist,
                    "exit_code": null
                }])
                .to_string(),
            ),
            brew_path: PathBuf::from("brew"),
        };
        let sources = provider.scan().unwrap();
        assert_eq!(sources[0].source_id, "launchd:homebrew.mxcl.redis");
        assert_eq!(sources[0].provider, "launchd");
        assert!(sources[0].raw.contains("homebrew-service"));
    }

    #[test]
    fn launchd_scan_uses_fixture_runtime_state_only_for_user_agents() {
        let directory = tempdir().unwrap();
        let user_root = directory.path().join("Library/LaunchAgents");
        let system_root = directory.path().join("Library/LaunchDaemons");
        std::fs::create_dir_all(&user_root).unwrap();
        std::fs::create_dir_all(&system_root).unwrap();
        let plist = br#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>Label</key><string>com.example.state</string></dict></plist>"#;
        std::fs::write(user_root.join("com.example.state.plist"), plist).unwrap();
        std::fs::write(system_root.join("com.example.state.plist"), plist).unwrap();
        let provider = LaunchdProvider {
            roots: vec![user_root, system_root],
            runtime_states: Some(BTreeMap::from([("com.example.state".into(), false)])),
            user_domain: Some("gui/501".into()),
            home: Some(directory.path().to_path_buf()),
        };
        let sources = provider.scan().unwrap();
        assert_eq!(sources.len(), 2);
        assert!(sources.iter().any(|source| !source.enabled
            && source.path.as_ref().is_some_and(|path| {
                path.starts_with(directory.path().join("Library/LaunchAgents"))
            })));
        assert!(sources.iter().any(|source| source.enabled
            && source.path.as_ref().is_some_and(|path| {
                path.starts_with(directory.path().join("Library/LaunchDaemons"))
            })));
    }
}
