const navItems = [
  ["dashboard", "⌂", "Dashboard"],
  ["automations", "◆", "Automations"],
  ["discovery", "⌁", "Discovery"],
  ["runs", "↻", "Runs"],
  ["inbox", "!", "Inbox"],
  ["integrations", "◇", "Integrations"],
  ["approvals", "✓", "Approvals"],
  ["metrics", "∿", "Metrics"],
  ["events", "≡", "Events"],
];

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

const app = document.querySelector("#app");
const escapeHtml = value => String(value ?? "")
  .replaceAll("&", "&amp;").replaceAll("<", "&lt;")
  .replaceAll(">", "&gt;").replaceAll('"', "&quot;");
const json = value => JSON.stringify(value ?? {}, null, 2);
const label = value => String(value ?? "").replaceAll("_", " ");
const pill = (value, kind = "") => `<span class="pill ${kind}">${escapeHtml(label(value))}</span>`;
const empty = message => `<div class="empty">${escapeHtml(message)}</div>`;

const wait = milliseconds => new Promise(resolve => setTimeout(resolve, milliseconds));

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
  finally { state.busy.delete(key); }
}

function automationRows() {
  if (!state.automations.length) return empty("No automations registered.");
  return `<div class="table-wrap"><table><thead><tr><th>Name</th><th>Ownership</th><th>State</th><th>Next run</th><th></th></tr></thead><tbody>${state.automations.map(item => {
    const id = escapeHtml(item.id);
    const paused = item.runtime_state === "paused";
    const attention = item.runtime_state === "needs_attention";
    return `<tr><td><strong>${escapeHtml(item.name)}</strong><br><code>${id}</code></td><td>${pill(item.ownership)}</td><td>${pill(item.runtime_state, attention ? "bad" : paused ? "warn" : "ok")}</td><td class="mono">${escapeHtml(item.next_run_at || "manual")}</td><td><div class="actions"><button class="mini" data-action="run" data-id="${id}" ${item.ownership === "observed" ? "disabled" : ""}>Run</button><button class="mini" data-action="${paused ? "resume" : "pause"}" data-id="${id}" ${item.ownership === "observed" || attention ? "disabled" : ""}>${paused ? "Resume" : "Pause"}</button></div></td></tr>`;
  }).join("")}</tbody></table></div>`;
}

function inboxRows() {
  if (!state.inbox.length) return empty("Inbox is clear.");
  return `<div class="table-wrap"><table><thead><tr><th>Severity</th><th>Kind</th><th>Title</th><th>Status</th></tr></thead><tbody>${state.inbox.map(item => `<tr><td>${pill(item.severity, item.severity === "critical" || item.severity === "high" ? "bad" : "warn")}</td><td>${escapeHtml(item.kind)}</td><td><strong>${escapeHtml(item.title)}</strong><br><code>${escapeHtml(item.id)}</code></td><td>${pill(item.status)}</td></tr>`).join("")}</tbody></table></div>`;
}

function runRows() {
  if (!state.runs.length) return empty("No runs recorded.");
  return `<div class="table-wrap"><table><thead><tr><th>Automation</th><th>Status</th><th>Started</th><th>Exit</th><th></th></tr></thead><tbody>${state.runs.map(run => `<tr><td><strong>${escapeHtml(run.automation_id)}</strong><br><code>${escapeHtml(run.id)}</code></td><td>${pill(run.status, run.status === "succeeded" ? "ok" : run.status === "running" ? "warn" : "bad")}</td><td class="mono">${escapeHtml(run.started_at)}</td><td>${escapeHtml(run.exit_code ?? "—")}</td><td><div class="actions"><button class="mini" data-action="logs" data-id="${escapeHtml(run.id)}">Logs</button>${run.status === "running" ? `<button class="mini danger" data-action="cancel" data-id="${escapeHtml(run.id)}">Cancel</button>` : ""}</div></td></tr>`).join("")}</tbody></table></div>`;
}

function pageBody() {
  switch (state.page) {
    case "automations": return `<h2 class="section-title">Automations</h2><p class="section-subtitle">Managed commands and observed native jobs.</p><section class="panel"><div class="panel-head"><h2>Registry</h2><span class="muted">${state.automations.length} item(s)</span></div>${automationRows()}</section>`;
    case "discovery": return `<h2 class="section-title">Native discovery</h2><p class="section-subtitle">Read-only inventory of launchd, cron, systemd, and Homebrew sources.</p><div class="toolbar"><button class="button primary" data-action="discover">Scan now</button><span class="muted">A scan never changes the native definition.</span></div><section class="panel">${state.discovery.length ? `<div class="table-wrap"><table><thead><tr><th>Native ID</th><th>Provider</th><th>Kind</th><th>State</th><th>Path</th></tr></thead><tbody>${state.discovery.map(item => `<tr><td><code>${escapeHtml(item.native_id)}</code></td><td>${escapeHtml(item.provider)}</td><td>${escapeHtml(item.kind)}</td><td>${pill(item.enabled ? "enabled" : "paused", item.enabled ? "ok" : "warn")}</td><td class="mono">${escapeHtml(item.path || "—")}</td></tr>`).join("")}</tbody></table></div>` : empty("No native scan has run yet.")}</section>`;
    case "runs": return `<h2 class="section-title">Runs</h2><p class="section-subtitle">Immutable run records and bounded stdout/stderr logs.</p><section class="panel">${runRows()}</section><div id="log-detail"></div>`;
    case "inbox": return `<h2 class="section-title">Inbox</h2><p class="section-subtitle">Failures, drift, missing sources, and recovery items.</p><section class="panel">${inboxRows()}</section>`;
    case "integrations": return `<h2 class="section-title">Integrations</h2><p class="section-subtitle">Typed semantic adapters detected on this host.</p><section class="panel">${integrationBody()}</section>`;
    case "approvals": return `<h2 class="section-title">Approvals</h2><p class="section-subtitle">Plan-bound, expiring, one-time requests for native writes.</p><section class="panel">${approvalRows()}</section>`;
    case "metrics": return `<h2 class="section-title">Metrics</h2><p class="section-subtitle">Recorded provider and operational measurements.</p><section class="panel">${metricRows()}</section>`;
    case "events": return `<h2 class="section-title">Events</h2><p class="section-subtitle">Audit history for runs, adoption, discovery, and approvals.</p><section class="panel">${eventRows()}</section>`;
    default: return dashboardBody();
  }
}

function dashboardBody() {
  const status = state.status || {};
  const discovery = status.native_discovery || {};
  return `<div class="cards"><div class="card"><div class="card-label">Automations</div><div class="card-value">${state.automations.length}</div><div class="card-detail">${status.managed_count || 0} managed · ${status.observed_count || 0} observed</div></div><div class="card"><div class="card-label">Recent runs</div><div class="card-value">${state.runs.length}</div><div class="card-detail">${state.runs.filter(run => run.status === "succeeded").length} succeeded in current window</div></div><div class="card"><div class="card-label">Needs attention</div><div class="card-value">${state.inbox.length}</div><div class="card-detail">Failures and drift stay visible</div></div><div class="card"><div class="card-label">Pending approvals</div><div class="card-value">${state.approvals.filter(item => item.status === "pending").length}</div><div class="card-detail">Typed plans only · no shell access</div></div></div><div class="grid-2"><section class="panel"><div class="panel-head"><h2>Automation overview</h2><span class="muted">${escapeHtml(status.host?.label || "local host")}</span></div>${automationRows()}</section><section class="panel"><div class="panel-head"><h2>Needs attention</h2><span class="muted">${discovery.source_count || 0} native source(s)</span></div>${inboxRows()}</section></div>`;
}

function integrationBody() {
  if (!state.integrations) return empty("Integration status unavailable.");
  const descriptors = state.integrations.descriptors || [];
  const detection = state.integrations.detection || [];
  const doctor = state.integrations.doctor || [];
  if (!descriptors.length) return empty("No integrations registered.");
  return `<div class="table-wrap"><table><thead><tr><th>Integration</th><th>Detection</th><th>Doctor</th><th>Capabilities</th></tr></thead><tbody>${descriptors.map(item => { const d = detection.find(row => row.integration === item.id); const health = doctor.find(row => row.integration === item.id); return `<tr><td><strong>${escapeHtml(item.display_name)}</strong><br><code>${escapeHtml(item.id)}</code></td><td>${pill(d?.status || "unknown", d?.status === "available" ? "ok" : "warn")}</td><td>${pill(health?.status || "unknown", health?.status === "ready" ? "ok" : "warn")}</td><td class="muted">${escapeHtml((item.capabilities || []).map(cap => `${cap.action} (${cap.risk})`).join(", "))}</td></tr>`; }).join("")}</tbody></table></div>`;
}

function approvalRows() {
  if (!state.approvals.length) return empty("Approval queue is clear.");
  return `<div class="table-wrap"><table><thead><tr><th>Action</th><th>Risk</th><th>Status</th><th>Expires</th><th></th></tr></thead><tbody>${state.approvals.map(item => `<tr><td><strong>${escapeHtml(item.integration)} · ${escapeHtml(item.action)}</strong><br><span class="muted">${escapeHtml(item.reason)}</span></td><td>${pill(item.risk, item.risk === "destructive" || item.risk === "system_write" ? "bad" : "warn")}</td><td>${pill(item.status, item.status === "pending" ? "warn" : item.status === "approved" ? "ok" : "")}</td><td class="mono">${escapeHtml(item.expires_at)}</td><td>${item.status === "pending" ? `<div class="actions"><button class="mini" data-action="approve" data-id="${escapeHtml(item.id)}">Approve</button><button class="mini danger" data-action="reject" data-id="${escapeHtml(item.id)}">Reject</button></div>` : ""}</td></tr>`).join("")}</tbody></table></div>`;
}

function metricRows() {
  if (!state.metrics.length) return empty("No metrics recorded.");
  return `<div class="table-wrap"><table><thead><tr><th>Key</th><th>Value</th><th>Source</th><th>Recorded</th></tr></thead><tbody>${state.metrics.map(item => `<tr><td>${escapeHtml(item.key)}</td><td><strong>${escapeHtml(item.value)} ${escapeHtml(item.unit)}</strong></td><td>${escapeHtml(item.source)}</td><td class="mono">${escapeHtml(item.recorded_at)}</td></tr>`).join("")}</tbody></table></div>`;
}

function eventRows() {
  if (!state.events.length) return empty("No events recorded.");
  return `<div class="table-wrap"><table><thead><tr><th>Seq</th><th>Type</th><th>Occurred</th><th>Payload keys</th></tr></thead><tbody>${state.events.map(item => `<tr><td class="mono">#${escapeHtml(item.seq)}</td><td><strong>${escapeHtml(item.event_type)}</strong></td><td class="mono">${escapeHtml(item.occurred_at)}</td><td class="muted">${escapeHtml(Object.keys(item.payload || {}).join(", ") || "—")}</td></tr>`).join("")}</tbody></table></div>`;
}

function render() {
  const status = state.status;
  const connected = Boolean(status);
  app.innerHTML = `<div class="app"><aside class="sidebar" id="sidebar"><div class="brand"><span class="brand-mark">T</span><span class="brand-name">Taskrail</span><span class="brand-version">web</span></div><nav class="nav">${navItems.map(([id, icon, text]) => `<button class="${state.page === id ? "active" : ""}" data-page="${id}"><span class="nav-icon">${icon}</span>${text}</button>`).join("")}</nav><div class="sidebar-foot"><span>Local control plane</span><span class="mono">${escapeHtml(status?.host?.label || "daemon unavailable")}</span><a href="/healthz" target="_blank" rel="noreferrer">healthz ↗</a></div></aside><main class="main"><header class="topbar"><button class="button mobile-menu" data-action="menu">☰</button><div><h1>${state.page === "dashboard" ? "Local Automation Manager" : escapeHtml(navItems.find(item => item[0] === state.page)?.[2] || "Taskrail")}</h1><p>Daemon-hosted dashboard · local HTTP management API</p><div class="status"><span class="status-dot ${connected ? "ok" : ""}"></span>${connected ? `Connected · ${escapeHtml(status.host?.platform || "local")} ${escapeHtml(status.host?.architecture || "")}` : "Daemon unavailable · start taskrail daemon"}</div></div><div class="topbar-actions"><button class="button" data-action="refresh">Refresh</button></div></header>${state.error ? `<div class="notice">${escapeHtml(state.error)}</div>` : ""}${pageBody()}</main></div>`;
  bindEvents();
}

function bindEvents() {
  document.querySelectorAll("[data-page]").forEach(button => button.addEventListener("click", () => { location.hash = button.dataset.page; }));
  document.querySelectorAll("[data-action]").forEach(button => button.addEventListener("click", () => {
    const action = button.dataset.action;
    const id = button.dataset.id;
    if (action === "refresh") return refresh();
    if (action === "menu") return document.querySelector("#sidebar")?.classList.toggle("open");
    if (action === "discover") return request("/api/discovery?source=all").then(rows => { state.discovery = rows; render(); }).catch(error => { state.error = error.message; render(); });
    if (action === "logs") return request(`/api/runs/${encodeURIComponent(id)}/logs`).then(logs => { const target = document.querySelector("#log-detail"); if (target) target.innerHTML = `<section class="panel" style="margin-top:14px"><div class="panel-head"><h2>${escapeHtml(logs.automation_id)} · ${escapeHtml(logs.status)}</h2></div><div class="panel-body stack"><div><div class="muted">STDOUT</div><div class="log">${escapeHtml(logs.stdout || "(empty)")}</div></div><div><div class="muted">STDERR</div><div class="log">${escapeHtml(logs.stderr || "(empty)")}</div></div></div></section>`; }).catch(error => { state.error = error.message; render(); });
    const path = action === "run" ? `/api/automations/${encodeURIComponent(id)}/run` : action === "pause" ? `/api/automations/${encodeURIComponent(id)}/pause` : action === "resume" ? `/api/automations/${encodeURIComponent(id)}/resume` : action === "cancel" ? `/api/runs/${encodeURIComponent(id)}/cancel` : action === "approve" ? `/api/approvals/${encodeURIComponent(id)}/approve` : action === "reject" ? `/api/approvals/${encodeURIComponent(id)}/reject` : null;
    if (path) return act(`${action}:${id}`, path);
  }));
}

window.addEventListener("hashchange", () => { state.page = (location.hash.slice(1) || "dashboard").split("/")[0]; refresh({ includeIntegrations: state.page === "integrations" }); });
document.addEventListener("visibilitychange", () => { if (!document.hidden) refresh(); });
render();
refresh();
setInterval(() => { if (!document.hidden && state.page === "dashboard") refresh(); }, 5000);
