#!/usr/bin/env python3
"""Validate the checked-in ChatGPT app submission pack."""

from __future__ import annotations

import json
import re
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SUBMISSION = ROOT / "chatgpt-app-submission.json"
MANIFEST = ROOT / ".codex-plugin" / "plugin.json"
MCP_MANIFEST = ROOT / ".mcp.json"

PUBLIC_TOOLS = {
    "taskrail_status",
    "taskrail_overview",
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
}
OPEN_WORLD_TOOLS = {
    "taskrail_github",
    "taskrail_mas",
    "taskrail_osv_scanner",
    "taskrail_trivy",
}
TOOL_NAME_RE = re.compile(r"\btaskrail_[a-z0-9_]+\b")


def load_object(path: Path) -> dict:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise ValueError(f"{path} must contain a JSON object")
    return value


def main() -> None:
    errors: list[str] = []
    try:
        submission = load_object(SUBMISSION)
        manifest = load_object(MANIFEST)
        mcp_manifest = load_object(MCP_MANIFEST)
    except (OSError, json.JSONDecodeError, ValueError) as error:
        raise SystemExit(f"OpenAI submission validation failed: {error}") from error

    if submission.get("schema_version") != 1:
        errors.append("schema_version must be 1")
    app_info = submission.get("app_info")
    if not isinstance(app_info, dict):
        errors.append("app_info must be an object")
    else:
        if app_info.get("display_name") != "Taskrail":
            errors.append("app_info.display_name must be Taskrail")
        if not isinstance(app_info.get("subtitle"), str) or len(app_info["subtitle"]) > 30:
            errors.append("app_info.subtitle must be a string of at most 30 characters")
        if app_info.get("category") != "DEVELOPER_TOOLS":
            errors.append("app_info.category must be DEVELOPER_TOOLS")

    tools = submission.get("tools")
    if not isinstance(tools, dict):
        errors.append("tools must be an object")
        tools = {}
    if set(tools) != PUBLIC_TOOLS:
        errors.append(
            "submission tools must exactly match the public read-only allowlist: "
            f"missing={sorted(PUBLIC_TOOLS - set(tools))}, "
            f"extra={sorted(set(tools) - PUBLIC_TOOLS)}"
        )
    for name, definition in tools.items():
        if not isinstance(definition, dict):
            errors.append(f"{name} must be an object")
            continue
        annotations = definition.get("annotations")
        expected_annotations = {
            "readOnlyHint": True,
            "openWorldHint": name in OPEN_WORLD_TOOLS,
            "destructiveHint": False,
        }
        if annotations != expected_annotations:
            errors.append(f"{name} annotations do not match the public profile behavior")
        justifications = definition.get("justifications")
        if not isinstance(justifications, dict) or set(justifications) != {
            "read_only_justification",
            "open_world_justification",
            "destructive_justification",
        }:
            errors.append(f"{name} must have all three review justifications")
        elif any(
            not isinstance(value, str) or len(value.split(".")) < 2
            for value in justifications.values()
        ):
            errors.append(f"{name} has an implausibly short justification")

    positive = submission.get("test_cases")
    negative = submission.get("negative_test_cases")
    if not isinstance(positive, list) or len(positive) != 5:
        errors.append("test_cases must contain exactly five cases")
        positive = []
    if not isinstance(negative, list) or len(negative) != 3:
        errors.append("negative_test_cases must contain exactly three cases")
        negative = []

    for index, case in enumerate(positive):
        if not isinstance(case, dict):
            errors.append(f"test_cases[{index}] must be an object")
            continue
        triggered = case.get("tools_triggered")
        names = set(TOOL_NAME_RE.findall(triggered or "")) if isinstance(triggered, str) else set()
        if not names or not names <= PUBLIC_TOOLS:
            errors.append(f"test_cases[{index}] uses an unknown or empty public tool name")
        if case.get("file_attachment_urls") is not None:
            errors.append(f"test_cases[{index}] must not require file attachments")
        if case.get("expected_output_url") is not None:
            errors.append(f"test_cases[{index}] must not require an external expected-output URL")

    for index, case in enumerate(negative):
        if not isinstance(case, dict):
            errors.append(f"negative_test_cases[{index}] must be an object")
            continue
        if case.get("tools_triggered") is not None:
            errors.append(f"negative_test_cases[{index}] must not trigger a tool")

    if manifest.get("name") != "taskrail":
        errors.append("plugin manifest name must be taskrail")
    if manifest.get("mcpServers") != "./.mcp.json":
        errors.append("plugin manifest must point to ./.mcp.json")
    server = mcp_manifest.get("mcpServers", {}).get("taskrail", {})
    if server.get("env", {}).get("TASKRAIL_MCP_PROFILE") != "public":
        errors.append(".mcp.json must default to TASKRAIL_MCP_PROFILE=public")
    interface = manifest.get("interface")
    if not isinstance(interface, dict):
        errors.append("plugin manifest interface must be present")
    else:
        for key in ("websiteURL", "privacyPolicyURL", "termsOfServiceURL"):
            if not str(interface.get(key, "")).startswith("https://"):
                errors.append(f"plugin manifest interface.{key} must be an HTTPS URL")

    if errors:
        print("OpenAI submission validation failed:")
        for error in errors:
            print(f"- {error}")
        raise SystemExit(1)

    print(
        "OpenAI submission validation passed: "
        f"{len(tools)} public tools, {len(positive)} positive tests, {len(negative)} negative tests"
    )


if __name__ == "__main__":
    main()
