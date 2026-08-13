use crate::core::{CommandSpec, DiscoveredSource, Trigger, fingerprint_bytes};
use anyhow::{Context, Result};
use plist::Value;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Cursor,
    path::{Path, PathBuf},
    process::Command,
};
use uuid::Uuid;

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

/// Read-only discovery of Apple Shortcuts exposed by the supported `shortcuts`
/// command-line utility. Shortcuts are kept observed-only: although a direct
/// argv invocation is known, the shortcut itself may require GUI permissions,
/// prompts, or contextual input that Taskrail cannot safely infer.
#[derive(Debug, Clone)]
pub struct ShortcutsProvider {
    /// Fixture-friendly output from `shortcuts list --show-identifiers`.
    pub listing: Option<String>,
    pub executable: PathBuf,
}

impl Default for ShortcutsProvider {
    fn default() -> Self {
        Self {
            listing: None,
            executable: PathBuf::from("shortcuts"),
        }
    }
}

impl DiscoveryProvider for ShortcutsProvider {
    fn name(&self) -> &'static str {
        "shortcuts"
    }

    fn scan(&self) -> Result<Vec<DiscoveredSource>> {
        let listing = match &self.listing {
            Some(listing) => listing.clone(),
            None => {
                let output = match Command::new(&self.executable)
                    .args(["list", "--show-identifiers"])
                    .output()
                {
                    Ok(output) => output,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        return Ok(Vec::new());
                    }
                    Err(error) => return Err(error).context("run shortcuts list"),
                };
                if !output.status.success() {
                    anyhow::bail!(
                        "shortcuts list failed: {}",
                        String::from_utf8_lossy(&output.stderr).trim()
                    );
                }
                String::from_utf8(output.stdout).context("shortcuts output is not UTF-8")?
            }
        };
        parse_shortcuts_list(&listing)
    }
}

pub fn parse_shortcuts_list(content: &str) -> Result<Vec<DiscoveredSource>> {
    let mut sources = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        let Some(identifier) = line.strip_suffix(')') else {
            continue;
        };
        let Some((name, identifier)) = identifier.rsplit_once(" (") else {
            continue;
        };
        if name.trim().is_empty() {
            continue;
        }
        if Uuid::parse_str(identifier).is_err() {
            continue;
        }
        sources.push(DiscoveredSource {
            source_id: format!("shortcuts:{identifier}"),
            provider: "shortcuts".into(),
            native_id: bounded_label(name.trim()),
            path: None,
            enabled: true,
            kind: "shortcut".into(),
            fingerprint: fingerprint_bytes(line.as_bytes()),
            // The shortcut may prompt for input, access GUI state, or perform
            // arbitrary third-party actions. It is therefore discoverable but
            // not represented as a directly runnable Taskrail command.
            command: None,
            trigger: Trigger::Manual,
            raw: format!("shortcut_id={identifier}"),
        });
    }
    Ok(sources)
}

/// Discover Automator workflow bundles from the user and system workflow
/// locations. The workflow bundle is not parsed or opened; its path and
/// fingerprint are enough for safe observation and drift reporting.
#[derive(Debug, Clone)]
pub struct AutomatorProvider {
    pub roots: Vec<PathBuf>,
}

impl Default for AutomatorProvider {
    fn default() -> Self {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let mut roots = Vec::new();
        if let Some(home) = home {
            roots.extend([
                home.join("Library/Services"),
                home.join("Library/Workflows"),
                home.join("Library/Automator"),
            ]);
        }
        roots.extend([
            PathBuf::from("/Library/Services"),
            PathBuf::from("/Library/Workflows"),
            PathBuf::from("/Library/Automator"),
            PathBuf::from("/System/Library/Services"),
            PathBuf::from("/System/Library/Workflows"),
        ]);
        Self { roots }
    }
}

impl DiscoveryProvider for AutomatorProvider {
    fn name(&self) -> &'static str {
        "automator"
    }

    fn scan(&self) -> Result<Vec<DiscoveredSource>> {
        let mut bundles = Vec::new();
        for root in &self.roots {
            collect_bundles(root, 6, &mut bundles);
        }
        bundles.sort();
        bundles.dedup();
        bundles.into_iter().map(automator_source).collect()
    }
}

fn collect_bundles(root: &Path, depth: usize, result: &mut Vec<PathBuf>) {
    if depth == 0 || !root.is_dir() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) == Some("workflow") {
            result.push(path);
        } else if path.is_dir() {
            collect_bundles(&path, depth - 1, result);
        }
    }
}

fn automator_source(path: PathBuf) -> Result<DiscoveredSource> {
    let fingerprint = fingerprint_path(&path)?;
    let native_id = path
        .file_stem()
        .map(|value| bounded_label(&value.to_string_lossy()))
        .unwrap_or_else(|| "workflow".into());
    Ok(DiscoveredSource {
        source_id: format!("automator:{}", stable_source_key(&path)),
        provider: "automator".into(),
        native_id,
        path: Some(path.clone()),
        enabled: true,
        kind: "workflow".into(),
        fingerprint,
        command: None,
        trigger: Trigger::Manual,
        raw: format!("workflow={}", stable_relative_path(&path)),
    })
}

/// Keyboard Maestro's macro database is a private plist. We intentionally
/// inspect only stable names, UIDs, enabled flags, and trigger/action counts;
/// action bodies and variables are never copied into the Registry or MCP.
#[derive(Debug, Clone)]
pub struct KeyboardMaestroProvider {
    pub files: Vec<PathBuf>,
}

impl Default for KeyboardMaestroProvider {
    fn default() -> Self {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let mut files = Vec::new();
        if let Some(home) = home {
            files.extend([
                home.join(
                    "Library/Application Support/Keyboard Maestro/Keyboard Maestro Macros.plist",
                ),
                home.join("Library/Preferences/Keyboard Maestro Macros.plist"),
                home.join("Library/Preferences/Keyboard Maestro/Keyboard Maestro Macros.plist"),
            ]);
        }
        Self { files }
    }
}

impl DiscoveryProvider for KeyboardMaestroProvider {
    fn name(&self) -> &'static str {
        "keyboard-maestro"
    }

    fn scan(&self) -> Result<Vec<DiscoveredSource>> {
        let mut result = Vec::new();
        for file in &self.files {
            if !file.is_file() {
                continue;
            }
            let bytes = std::fs::read(file)
                .with_context(|| format!("read Keyboard Maestro plist {}", file.display()))?;
            let value = plist::Value::from_reader(Cursor::new(&bytes))
                .with_context(|| format!("parse Keyboard Maestro plist {}", file.display()))?;
            let mut ordinal = 0usize;
            collect_keyboard_maestro_macros(&value, None, file, &bytes, &mut ordinal, &mut result);
        }
        Ok(result)
    }
}

fn collect_keyboard_maestro_macros(
    value: &plist::Value,
    group: Option<&str>,
    file: &Path,
    bytes: &[u8],
    ordinal: &mut usize,
    result: &mut Vec<DiscoveredSource>,
) {
    match value {
        plist::Value::Dictionary(dict) => {
            let name = dict.get("Name").and_then(plist::Value::as_string);
            let is_group = dict.contains_key("Macros") || dict.contains_key("MacroGroupUID");
            let current_group = if is_group { name.or(group) } else { group };
            let triggers = dict.get("Triggers").and_then(plist::Value::as_array);
            let actions = dict.get("Actions").and_then(plist::Value::as_array);
            if let (Some(name), Some(triggers), Some(actions)) = (name, triggers, actions) {
                let uid = dict
                    .get("MacroUID")
                    .or_else(|| dict.get("UID"))
                    .and_then(plist::Value::as_string)
                    .map(str::to_owned)
                    .unwrap_or_else(|| {
                        format!(
                            "generated-{}",
                            fingerprint_bytes(
                                format!("{}:{name}:{ordinal}", file.display()).as_bytes()
                            )
                        )
                    });
                let enabled = dict
                    .get("IsActive")
                    .or_else(|| dict.get("Enabled"))
                    .and_then(plist::Value::as_boolean)
                    .unwrap_or(true);
                result.push(DiscoveredSource {
                    source_id: format!("keyboard-maestro:{}", safe_source_component(&uid)),
                    provider: "keyboard-maestro".into(),
                    native_id: bounded_label(name),
                    path: Some(file.to_path_buf()),
                    enabled,
                    kind: "macro".into(),
                    fingerprint: fingerprint_bytes(bytes),
                    command: None,
                    trigger: Trigger::Manual,
                    raw: format!(
                        "macro_group={}; trigger_count={}; action_count={}",
                        bounded_label(current_group.unwrap_or("unknown")),
                        triggers.len(),
                        actions.len()
                    ),
                });
                *ordinal += 1;
            }
            for child in dict.values() {
                collect_keyboard_maestro_macros(child, current_group, file, bytes, ordinal, result);
            }
        }
        plist::Value::Array(values) => {
            for child in values {
                collect_keyboard_maestro_macros(child, group, file, bytes, ordinal, result);
            }
        }
        _ => {}
    }
}

/// Discover Alfred workflows from the user's configured Alfred preferences
/// store. Alfred workflows can contain arbitrary scripts and application
/// actions, so Taskrail records metadata and a fingerprint but does not make
/// them directly runnable.
#[derive(Debug, Clone)]
pub struct AlfredProvider {
    pub directories: Vec<PathBuf>,
}

impl Default for AlfredProvider {
    fn default() -> Self {
        Self {
            directories: alfred_workflow_directories(),
        }
    }
}

impl DiscoveryProvider for AlfredProvider {
    fn name(&self) -> &'static str {
        "alfred"
    }

    fn scan(&self) -> Result<Vec<DiscoveredSource>> {
        let mut workflows = Vec::new();
        for directory in &self.directories {
            collect_workflow_directories(directory, 2, &mut workflows);
        }
        workflows.sort();
        workflows.dedup();
        workflows.into_iter().map(alfred_source).collect()
    }
}

fn collect_workflow_directories(root: &Path, depth: usize, result: &mut Vec<PathBuf>) {
    if depth == 0 || !root.is_dir() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if path.join("info.plist").is_file() {
            result.push(path);
        } else {
            collect_workflow_directories(&path, depth - 1, result);
        }
    }
}

fn alfred_source(path: PathBuf) -> Result<DiscoveredSource> {
    let metadata = path.join("info.plist");
    let value = std::fs::read(&metadata)
        .ok()
        .and_then(|bytes| plist::Value::from_reader(Cursor::new(bytes)).ok());
    let name = value
        .as_ref()
        .and_then(plist_string)
        .or_else(|| {
            path.file_name()
                .map(|name| bounded_label(&name.to_string_lossy()))
        })
        .unwrap_or_else(|| "workflow".into());
    let uid = value
        .as_ref()
        .and_then(|value| plist_string_for(value, &["uid", "bundleid", "id"]))
        .unwrap_or_else(|| stable_relative_path(&path));
    let version = value
        .as_ref()
        .and_then(|value| plist_string_for(value, &["version"]))
        .unwrap_or_else(|| "unknown".into());
    let description_present = value
        .as_ref()
        .and_then(|value| plist_string_for(value, &["description"]))
        .is_some_and(|description| !description.is_empty());
    Ok(DiscoveredSource {
        source_id: format!(
            "alfred:{}",
            if uid == stable_relative_path(&path) {
                stable_source_key(&path)
            } else {
                safe_source_component(&uid)
            }
        ),
        provider: "alfred".into(),
        native_id: bounded_label(&name),
        path: Some(path.clone()),
        enabled: true,
        kind: "workflow".into(),
        fingerprint: fingerprint_path(&path)?,
        command: None,
        trigger: Trigger::Manual,
        raw: format!(
            "workflow_path={}; version={}; description_present={description_present}",
            stable_relative_path(&path),
            bounded_label(&version)
        ),
    })
}

fn plist_string(value: &plist::Value) -> Option<String> {
    value
        .as_dictionary()
        .and_then(|dict| {
            ["name", "title", "workflow_name"]
                .iter()
                .find_map(|key| dict.get(key).and_then(plist::Value::as_string))
        })
        .map(str::to_owned)
}

fn plist_string_for(value: &plist::Value, keys: &[&str]) -> Option<String> {
    let dict = value.as_dictionary()?;
    keys.iter().find_map(|key| {
        dict.iter()
            .find(|(candidate, _)| candidate.eq_ignore_ascii_case(key))
            .and_then(|(_, value)| value.as_string())
            .map(str::to_owned)
    })
}

fn alfred_workflow_directories() -> Vec<PathBuf> {
    let mut directories = Vec::new();
    if let Some(value) = std::env::var_os("TASKRAIL_ALFRED_WORKFLOW_DIRS") {
        directories.extend(std::env::split_paths(&value));
    }
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        directories.push(
            home.join("Library/Application Support/Alfred/Alfred.alfredpreferences/workflows"),
        );
    }
    if let Ok(output) = Command::new("defaults")
        .args(["export", "com.runningwithcrayons.Alfred", "-"])
        .output()
        && output.status.success()
        && let Ok(value) = plist::Value::from_reader(Cursor::new(output.stdout))
    {
        collect_alfred_preference_paths(&value, false, &mut directories);
    }
    directories.retain(|path| path.is_dir());
    directories.sort();
    directories.dedup();
    directories
}

fn collect_alfred_preference_paths(value: &plist::Value, hinted: bool, result: &mut Vec<PathBuf>) {
    match value {
        plist::Value::Dictionary(dict) => {
            for (key, child) in dict {
                let key = key.to_ascii_lowercase();
                collect_alfred_preference_paths(
                    child,
                    hinted || key.contains("preference") || key.contains("sync"),
                    result,
                );
            }
        }
        plist::Value::Array(values) => {
            for child in values {
                collect_alfred_preference_paths(child, hinted, result);
            }
        }
        plist::Value::String(path) if hinted && path.contains(".alfredpreferences") => {
            let path = PathBuf::from(path).join("workflows");
            if path.is_dir() {
                result.push(path);
            }
        }
        _ => {}
    }
}

/// Discover Hazel rule bundles without parsing or copying rule actions. Hazel
/// rules are proprietary and can contain arbitrary file operations, scripts,
/// and secrets, so they remain observe-only and unrunnable by Taskrail.
#[derive(Debug, Clone)]
pub struct HazelProvider {
    pub roots: Vec<PathBuf>,
}

impl Default for HazelProvider {
    fn default() -> Self {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let mut roots = Vec::new();
        if let Some(home) = home {
            roots.push(home.join("Library/Application Support/Hazel"));
        }
        roots.push(PathBuf::from("/Library/Application Support/Hazel"));
        Self { roots }
    }
}

impl DiscoveryProvider for HazelProvider {
    fn name(&self) -> &'static str {
        "hazel"
    }

    fn scan(&self) -> Result<Vec<DiscoveredSource>> {
        let mut files = Vec::new();
        for root in &self.roots {
            collect_files_with_extension(root, "hazelrules", 3, &mut files);
        }
        files.sort();
        files.dedup();
        files
            .into_iter()
            .map(|path| {
                let bytes = std::fs::read(&path)
                    .with_context(|| format!("read Hazel rule {}", path.display()))?;
                let native_id = path
                    .file_stem()
                    .map(|value| bounded_label(&value.to_string_lossy()))
                    .unwrap_or_else(|| "rule".into());
                Ok(DiscoveredSource {
                    source_id: format!("hazel:{}", stable_source_key(&path)),
                    provider: "hazel".into(),
                    native_id,
                    path: Some(path.clone()),
                    enabled: true,
                    kind: "rule".into(),
                    fingerprint: fingerprint_bytes(&bytes),
                    command: None,
                    trigger: Trigger::Manual,
                    raw: format!("rule_file={}", stable_relative_path(&path)),
                })
            })
            .collect()
    }
}

/// Raycast script commands are stored in user-selected directories rather than
/// one mandatory location. Taskrail accepts an explicit environment override,
/// a small set of conventional folders, and path values from Raycast's own
/// exported preferences. Only metadata comments from each script are read.
#[derive(Debug, Clone)]
pub struct RaycastProvider {
    pub directories: Vec<PathBuf>,
}

impl Default for RaycastProvider {
    fn default() -> Self {
        Self {
            directories: raycast_script_directories(),
        }
    }
}

impl DiscoveryProvider for RaycastProvider {
    fn name(&self) -> &'static str {
        "raycast"
    }

    fn scan(&self) -> Result<Vec<DiscoveredSource>> {
        let mut files = Vec::new();
        for directory in &self.directories {
            collect_files_bounded(directory, 8, &mut files);
        }
        files.sort();
        files.dedup();
        files.retain(|path| raycast_script_extension(path));
        files.into_iter().map(raycast_source).collect()
    }
}

fn raycast_source(path: PathBuf) -> Result<DiscoveredSource> {
    let bytes =
        std::fs::read(&path).with_context(|| format!("read Raycast script {}", path.display()))?;
    let (title, description) = raycast_metadata(&bytes);
    let native_id = title.map(|value| bounded_label(&value)).unwrap_or_else(|| {
        path.file_stem()
            .map(|value| bounded_label(&value.to_string_lossy()))
            .unwrap_or_else(|| "script-command".into())
    });
    Ok(DiscoveredSource {
        source_id: format!("raycast:{}", stable_source_key(&path)),
        provider: "raycast".into(),
        native_id,
        path: Some(path.clone()),
        enabled: true,
        kind: "script-command".into(),
        fingerprint: fingerprint_bytes(&bytes),
        command: None,
        trigger: Trigger::Manual,
        raw: format!(
            "script_command={}; description_present={}",
            stable_relative_path(&path),
            description.is_some()
        ),
    })
}

fn raycast_script_directories() -> Vec<PathBuf> {
    let mut directories = Vec::new();
    if let Some(value) = std::env::var_os("TASKRAIL_RAYCAST_SCRIPT_DIRS") {
        directories.extend(std::env::split_paths(&value));
    }
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        directories.extend([
            home.join("Documents/Raycast"),
            home.join("raycast"),
            home.join(".raycast"),
            home.join("Library/Application Support/Raycast/Script Commands"),
            home.join("Library/Application Support/com.raycast.macos/Script Commands"),
        ]);
    }
    if let Ok(output) = Command::new("defaults")
        .args(["export", "com.raycast.macos", "-"])
        .output()
        && output.status.success()
        && let Ok(value) = plist::Value::from_reader(Cursor::new(output.stdout))
    {
        collect_configured_script_paths(&value, false, &mut directories);
    }
    directories.retain(|path| path.is_dir());
    directories.sort();
    directories.dedup();
    directories
}

fn collect_configured_script_paths(value: &plist::Value, hinted: bool, result: &mut Vec<PathBuf>) {
    match value {
        plist::Value::Dictionary(dict) => {
            for (key, child) in dict {
                let key = key.to_ascii_lowercase();
                collect_configured_script_paths(
                    child,
                    hinted
                        || key.contains("script")
                        || key.contains("command")
                        || key.contains("directory"),
                    result,
                );
            }
        }
        plist::Value::Array(values) => {
            for child in values {
                collect_configured_script_paths(child, hinted, result);
            }
        }
        plist::Value::String(path) if hinted => {
            let path = PathBuf::from(path);
            if path.is_dir() {
                result.push(path);
            } else if path.is_file()
                && let Some(parent) = path.parent()
            {
                result.push(parent.to_path_buf());
            }
        }
        _ => {}
    }
}

fn raycast_script_extension(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|value| value.to_str()),
        Some(
            "sh" | "bash"
                | "zsh"
                | "fish"
                | "py"
                | "js"
                | "ts"
                | "rb"
                | "php"
                | "swift"
                | "applescript"
                | "scpt"
        )
    )
}

fn raycast_metadata(bytes: &[u8]) -> (Option<String>, Option<String>) {
    let prefix = String::from_utf8_lossy(&bytes[..bytes.len().min(16 * 1024)]);
    let mut title = None;
    let mut description = None;
    for line in prefix.lines().take(100) {
        let Some((_prefix, value)) = line.split_once("@raycast.") else {
            continue;
        };
        let mut parts = value.splitn(2, |character: char| character.is_whitespace());
        let field = parts.next().unwrap_or_default();
        let value = parts.next().unwrap_or_default();
        let value = value.trim().trim_matches(['#', '/', ' ', '\t']).trim();
        match field {
            "title" if !value.is_empty() => title = Some(value.to_owned()),
            "description" if !value.is_empty() => description = Some(value.to_owned()),
            _ => {}
        }
    }
    (title, description)
}

fn fingerprint_path(path: &Path) -> Result<String> {
    if path.is_file() {
        return Ok(fingerprint_bytes(
            &std::fs::read(path).with_context(|| format!("read {}", path.display()))?,
        ));
    }
    if !path.is_dir() {
        anyhow::bail!(
            "discovered path is neither a file nor directory: {}",
            path.display()
        );
    }
    let mut files = Vec::new();
    collect_files_bounded(path, 8, &mut files);
    files.sort();
    let mut material = Vec::new();
    for file in files {
        let relative = file.strip_prefix(path).unwrap_or(&file);
        material.extend_from_slice(relative.to_string_lossy().as_bytes());
        material.push(0);
        let bytes = std::fs::read(&file)
            .with_context(|| format!("read discovered file {}", file.display()))?;
        material.extend_from_slice(&bytes);
        material.push(0);
    }
    Ok(fingerprint_bytes(&material))
}

fn collect_files_with_extension(
    root: &Path,
    extension: &str,
    depth: usize,
    result: &mut Vec<PathBuf>,
) {
    let mut paths = Vec::new();
    collect_files_bounded(root, depth, &mut paths);
    result.extend(
        paths
            .into_iter()
            .filter(|path| path.extension().and_then(|value| value.to_str()) == Some(extension)),
    );
}

fn collect_files_bounded(root: &Path, depth: usize, result: &mut Vec<PathBuf>) {
    if depth == 0 || !root.exists() {
        return;
    }
    if root.is_file() {
        result.push(root.to_path_buf());
        return;
    }
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            result.push(path);
        } else if path.is_dir() {
            collect_files_bounded(&path, depth - 1, result);
        }
    }
}

fn stable_relative_path(path: &Path) -> String {
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from)
        && let Ok(relative) = path.strip_prefix(&home)
    {
        return format!("~/{}", relative.to_string_lossy());
    }
    path.to_string_lossy().into_owned()
}

fn stable_source_key(path: &Path) -> String {
    format!(
        "path-{}",
        fingerprint_bytes(stable_relative_path(path).as_bytes())
    )
}

fn bounded_label(value: &str) -> String {
    let label = value
        .trim()
        .chars()
        .filter(|character| !character.is_control())
        .take(256)
        .collect::<String>();
    if label.is_empty() {
        "unnamed".into()
    } else {
        label
    }
}

fn safe_source_component(value: &str) -> String {
    let component = value
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-')
        })
        .take(128)
        .collect::<String>();
    if component.is_empty() {
        format!("sha256-{}", fingerprint_bytes(value.as_bytes()))
    } else {
        component
    }
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
    if !matches!(
        source,
        "all"
            | "launchd"
            | "cron"
            | "systemd"
            | "homebrew"
            | "shortcuts"
            | "automator"
            | "keyboard-maestro"
            | "raycast"
            | "alfred"
            | "hazel"
    ) {
        anyhow::bail!(
            "unknown native source {source}; expected all, launchd, cron, systemd, homebrew, shortcuts, automator, keyboard-maestro, raycast, alfred, or hazel"
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
    if matches!(source, "all" | "shortcuts") && cfg!(target_os = "macos") {
        discovered.extend(scan_optional_provider(
            ShortcutsProvider::default(),
            source,
        )?);
    }
    if matches!(source, "all" | "automator") && cfg!(target_os = "macos") {
        discovered.extend(scan_optional_provider(
            AutomatorProvider::default(),
            source,
        )?);
    }
    if matches!(source, "all" | "keyboard-maestro") && cfg!(target_os = "macos") {
        discovered.extend(scan_optional_provider(
            KeyboardMaestroProvider::default(),
            source,
        )?);
    }
    if matches!(source, "all" | "raycast") && cfg!(target_os = "macos") {
        discovered.extend(scan_optional_provider(RaycastProvider::default(), source)?);
    }
    if matches!(source, "all" | "alfred") && cfg!(target_os = "macos") {
        discovered.extend(scan_optional_provider(AlfredProvider::default(), source)?);
    }
    if matches!(source, "all" | "hazel") && cfg!(target_os = "macos") {
        discovered.extend(scan_optional_provider(HazelProvider::default(), source)?);
    }
    Ok(discovered)
}

fn scan_optional_provider<P: DiscoveryProvider>(
    provider: P,
    source: &str,
) -> Result<Vec<DiscoveredSource>> {
    match provider.scan() {
        Ok(sources) => Ok(sources),
        Err(error) if source == "all" => {
            // An unavailable or permission-blocked optional application must
            // not hide launchd/cron/systemd discovery or turn its old entries
            // into false missing alerts. Explicit provider scans still return
            // the real error to the operator.
            let _ = error;
            Ok(Vec::new())
        }
        Err(error) => Err(error),
    }
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
    // Application-owned stores are intentionally not marked complete here.
    // Their formats and permission surfaces vary by app; retaining the last
    // observation without manufacturing a missing alert is safer than treating
    // an unavailable app database as an authoritative empty inventory.

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
        .args(["--user", "list-unit-files", "--no-legend", "--no-pager"])
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
                    .args(["--user", "list-unit-files", "--no-legend", "--no-pager"])
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
            let Some(unit) = fields
                .first()
                .filter(|unit| unit.ends_with(".service") || unit.ends_with(".timer"))
            else {
                continue;
            };
            let state = fields.get(1).copied().unwrap_or("unknown");
            let timer = unit.ends_with(".timer");
            let timer_raw = if self.unit_list.is_some() {
                line.to_owned()
            } else {
                systemd_show(unit).unwrap_or_else(|_| line.to_owned())
            };
            let timer_properties = parse_systemd_properties(&timer_raw);
            let service_raw = if timer && self.unit_list.is_none() {
                timer_properties
                    .get("Unit")
                    .filter(|service| service.ends_with(".service"))
                    .and_then(|service| systemd_show(service).ok())
            } else {
                None
            };
            let raw = match &service_raw {
                Some(service_raw) => format!("{timer_raw}\n# taskrail-service: {service_raw}"),
                None => timer_raw,
            };
            let properties = &timer_properties;
            let command = if timer {
                None
            } else {
                properties
                    .get("ExecStart")
                    .and_then(|value| parse_systemd_command(value))
            };
            let trigger = systemd_trigger(properties);
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
                kind: if timer { "timer" } else { "service" }.into(),
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
            "--property=FragmentPath,ExecStart,Unit,OnUnitActiveSec,OnCalendar,UnitFileState,ActiveState",
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

fn systemd_trigger(properties: &BTreeMap<String, String>) -> Trigger {
    if let Some(seconds) = properties
        .get("OnUnitActiveSec")
        .and_then(|value| parse_systemd_duration(value))
    {
        return Trigger::Interval { seconds };
    }
    properties
        .get("OnCalendar")
        .and_then(|value| parse_systemd_calendar(value))
        .unwrap_or(Trigger::Manual)
}

/// Convert the common, unambiguous subset of systemd calendar expressions to
/// Taskrail's portable cron representation. Unsupported calendar syntax stays
/// manual rather than being guessed into a different schedule.
fn parse_systemd_calendar(value: &str) -> Option<Trigger> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("hourly") {
        return Some(Trigger::Cron {
            expression: "0 * * * *".into(),
            timezone: "local".into(),
        });
    }
    if value.eq_ignore_ascii_case("daily") {
        return Some(Trigger::Cron {
            expression: "0 0 * * *".into(),
            timezone: "local".into(),
        });
    }
    if value.eq_ignore_ascii_case("weekly") {
        return Some(Trigger::Cron {
            expression: "0 0 * * 0".into(),
            timezone: "local".into(),
        });
    }
    let fields = value.split_whitespace().collect::<Vec<_>>();
    let (weekday, date, clock) = match fields.as_slice() {
        [clock] => ("*", "*-*-*", *clock),
        [date, clock] => ("*", *date, *clock),
        [weekday, date, clock] => (*weekday, *date, *clock),
        _ => return None,
    };
    let mut clock = clock.split(':');
    let hour = clock.next()?.parse::<u8>().ok()?;
    let minute = clock.next()?.parse::<u8>().ok()?;
    if hour > 23
        || minute > 59
        || clock.next().is_some_and(|seconds| {
            seconds
                .split_once('.')
                .map_or_else(
                    || seconds.parse::<u8>().ok(),
                    |(whole, _)| whole.parse::<u8>().ok(),
                )
                .is_none_or(|seconds| seconds > 59)
        })
    {
        return None;
    }
    let date = date.split('-').collect::<Vec<_>>();
    if date.len() != 3 {
        return None;
    }
    let (month, day) = match (date[1], date[2]) {
        ("*", "*") => ("*", "*"),
        (month, day) => {
            if date[0] != "*" {
                return None;
            }
            if month != "*"
                && month
                    .parse::<u8>()
                    .ok()
                    .is_none_or(|month| !(1..=12).contains(&month))
            {
                return None;
            }
            if day != "*"
                && day
                    .parse::<u8>()
                    .ok()
                    .is_none_or(|day| !(1..=31).contains(&day))
            {
                return None;
            }
            (month, day)
        }
    };
    let weekday = match weekday.to_ascii_lowercase().as_str() {
        "*" => "*",
        "sun" => "0",
        "mon" => "1",
        "tue" | "tues" => "2",
        "wed" => "3",
        "thu" | "thur" | "thurs" => "4",
        "fri" => "5",
        "sat" => "6",
        _ => return None,
    };
    Some(Trigger::Cron {
        expression: format!("{minute} {hour} {day} {month} {weekday}"),
        timezone: "local".into(),
    })
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
    fn parses_shortcuts_listing_as_observe_only_sources() {
        let sources =
            parse_shortcuts_list("Daily check (11111111-1111-4111-8111-111111111111)\ninvalid\n")
                .unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].provider, "shortcuts");
        assert_eq!(sources[0].kind, "shortcut");
        assert!(sources[0].command.is_none());
        assert_eq!(
            sources[0].raw,
            "shortcut_id=11111111-1111-4111-8111-111111111111"
        );
    }

    #[test]
    fn parses_alfred_workflow_metadata_without_copying_actions() {
        let directory = tempdir().unwrap();
        let workflow = directory.path().join("example.alfredworkflow");
        std::fs::create_dir_all(&workflow).unwrap();
        let info = plist::Value::Dictionary(plist::Dictionary::from_iter([
            (
                "name".to_owned(),
                plist::Value::String("Example Workflow".into()),
            ),
            (
                "uid".to_owned(),
                plist::Value::String("com.example.workflow".into()),
            ),
            ("version".to_owned(), plist::Value::String("1.0".into())),
            (
                "description".to_owned(),
                plist::Value::String("Private workflow description".into()),
            ),
        ]));
        plist::to_file_xml(workflow.join("info.plist"), &info).unwrap();
        std::fs::write(workflow.join("script.sh"), "private action body").unwrap();
        let sources = AlfredProvider {
            directories: vec![directory.path().to_path_buf()],
        }
        .scan()
        .unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].source_id, "alfred:com.example.workflow");
        assert_eq!(sources[0].native_id, "Example Workflow");
        assert!(sources[0].command.is_none());
        assert!(sources[0].raw.contains("description_present=true"));
        assert!(!sources[0].raw.contains("Private workflow description"));
        assert!(!sources[0].raw.contains("private action body"));
    }

    #[test]
    fn all_scan_skips_optional_provider_errors_but_explicit_scan_surfaces_them() {
        #[derive(Debug, Clone, Copy)]
        struct FailingProvider;

        impl DiscoveryProvider for FailingProvider {
            fn name(&self) -> &'static str {
                "fixture"
            }

            fn scan(&self) -> Result<Vec<DiscoveredSource>> {
                anyhow::bail!("fixture provider unavailable")
            }
        }

        assert!(
            scan_optional_provider(FailingProvider, "all")
                .unwrap()
                .is_empty()
        );
        assert!(scan_optional_provider(FailingProvider, "fixture").is_err());
    }

    #[test]
    fn automator_bundle_is_observe_only_and_fingerprinted() {
        let directory = tempdir().unwrap();
        let workflow = directory.path().join("Example.workflow");
        std::fs::create_dir_all(&workflow).unwrap();
        std::fs::write(workflow.join("document.wflow"), "workflow").unwrap();
        let provider = AutomatorProvider {
            roots: vec![directory.path().to_path_buf()],
        };
        let sources = provider.scan().unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].native_id, "Example");
        assert!(sources[0].command.is_none());
        assert!(sources[0].fingerprint.starts_with("sha256:"));
    }

    #[test]
    fn parses_keyboard_maestro_macro_metadata_without_action_bodies() {
        let directory = tempdir().unwrap();
        let file = directory.path().join("Keyboard Maestro Macros.plist");
        let value = plist::Value::Dictionary(plist::Dictionary::from_iter([
            (
                String::from("Name"),
                plist::Value::String("Example Macro".into()),
            ),
            (
                String::from("MacroUID"),
                plist::Value::String("macro-1".into()),
            ),
            (String::from("IsActive"), plist::Value::Boolean(true)),
            (
                String::from("Triggers"),
                plist::Value::Array(vec![plist::Value::Dictionary(plist::Dictionary::new())]),
            ),
            (
                String::from("Actions"),
                plist::Value::Array(vec![plist::Value::Dictionary(plist::Dictionary::new())]),
            ),
        ]));
        plist::to_file_xml(&file, &value).unwrap();
        let sources = KeyboardMaestroProvider { files: vec![file] }
            .scan()
            .unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].source_id, "keyboard-maestro:macro-1");
        assert!(sources[0].command.is_none());
        assert!(sources[0].raw.contains("trigger_count=1"));
        assert!(!sources[0].raw.contains("secret"));
    }

    #[test]
    fn parses_raycast_metadata_without_copying_script_body() {
        let directory = tempdir().unwrap();
        let script = directory.path().join("example.sh");
        std::fs::write(
            &script,
            "#!/bin/zsh\n# @raycast.schemaVersion 1\n# @raycast.title Build project\n# @raycast.description Build the project\necho secret-body\n",
        )
        .unwrap();
        let sources = RaycastProvider {
            directories: vec![directory.path().to_path_buf()],
        }
        .scan()
        .unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].native_id, "Build project");
        assert_eq!(sources[0].kind, "script-command");
        assert!(sources[0].command.is_none());
        assert!(!sources[0].raw.contains("secret-body"));
    }

    #[test]
    fn hazel_rules_are_observe_only() {
        let directory = tempdir().unwrap();
        let rules = directory.path().join("folder.hazelrules");
        std::fs::write(&rules, "private rule body").unwrap();
        let sources = HazelProvider {
            roots: vec![directory.path().to_path_buf()],
        }
        .scan()
        .unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].native_id, "folder");
        assert!(sources[0].command.is_none());
        assert!(!sources[0].raw.contains("private rule body"));
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
    fn parses_systemd_timer_calendar_without_making_it_runnable() {
        let provider = SystemdProvider {
            unit_list: Some("taskrail.timer enabled\n".into()),
        };
        let sources = provider.scan().unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].kind, "timer");
        assert!(sources[0].is_observe_only());
        assert!(sources[0].command.is_none());
        assert_eq!(
            parse_systemd_calendar("Mon *-*-* 09:30:00").unwrap(),
            Trigger::Cron {
                expression: "30 9 * * 1".into(),
                timezone: "local".into()
            }
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
