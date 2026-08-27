"use strict";

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------
const state = {
  robots: new Map(),   // id -> RobotView
  runs: [],            // RunView[]
  types: [],           // (kept for future use; actions come from /api/actions)
  actions: { robotType: [], swarm: [], workflows: [], typeWorkflows: [] }, // dispatchable refs from /api/actions
  selectedRobot: null, // id currently shown in the log panel
  follow: true,
  modalOk: null,       // callback to run when the modal's OK is pressed
  prevConnected: new Map(), // id -> previous connected state (for change detection)
  connectEvents: new Map(), // id -> [{ts_ms, stderr, text}] synthetic log lines
};

// ---------------------------------------------------------------------------
// DOM helpers
// ---------------------------------------------------------------------------
const $ = (id) => document.getElementById(id);

// ---------------------------------------------------------------------------
// Initial load
// ---------------------------------------------------------------------------
async function init() {
  const results = await Promise.allSettled([fetchConfig(), fetchRobots(), fetchRuns(), fetchActions()]);
  const failed = results.filter((r) => r.status === "rejected");
  if (failed.length) {
    console.warn("initial fetch failed", failed.map((r) => r.reason));
  }
  // Seed connection state tracker so the first WS update doesn't fire
  // spurious connect/disconnect log lines.
  for (const [id, r] of state.robots) {
    state.prevConnected.set(id, r.connected);
  }
  renderRobots();
  renderRuns();
  populateActionSelect();
  bindRunForm();
  bindModal();
  bindLogFollow();
  bindLogCopy();
  bindLogDownload();
  bindThemeToggle();
  bindClearAll();
  connectWs();
}

async function fetchJson(url, opts) {
  const res = await fetch(url, opts);
  if (!res.ok) {
    const text = await res.text();
    throw new Error(text || `${res.status}`);
  }
  return res.json();
}

async function fetchRobots() {
  const list = await fetchJson("/api/robots");
  state.robots = new Map(list.map((r) => [r.id, r]));
}

async function fetchRuns() {
  state.runs = await fetchJson("/api/runs");
}

async function fetchActions() {
  // The wire contract uses snake_case ({"robot_type": [...], "swarm": [...]});
  // normalize into the camelCase shape the rest of this file uses.
  const view = await fetchJson("/api/actions");
  state.actions = {
    robotType: view.robot_type || [],
    swarm: view.swarm || [],
    workflows: view.workflows || [],
    typeWorkflows: view.type_workflows || [],
  };
}

async function fetchConfig() {
  const cfg = await fetchJson("/api/config");
  const name = cfg && cfg.controller ? cfg.controller : "";
  const el = $("controller-name");
  if (el) el.textContent = name;
  if (name) document.title = `SwarmDeck — ${name}`;
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------
function renderRobots() {
  const grid = $("robots");
  grid.innerHTML = "";
  const robots = [...state.robots.values()].sort((a, b) => naturalCompare(a.id, b.id));
  if (robots.length === 0) {
    grid.innerHTML = '<p class="muted">No robots yet. Start an agent (or run <code>swarmdeck-cli sim</code>).</p>';
    return;
  }
  for (const r of robots) {
    const cls = r.connected
      ? (r.simulated ? "dot sim" : "dot on")
      : (r.connected_since_ms > 0 ? "dot dropped" : "dot never");
    const active = r.active ? `<div class="active" title="${escapeHtml(r.active.action_name)}">▶ ${escapeHtml(r.active.action_name)}</div>` : "";
    const card = document.createElement("div");
    card.className = "card" + (state.selectedRobot === r.id ? " selected" : "");
    card.innerHTML = `
      <div class="row">
        <span class="name">${escapeHtml(r.name)}</span>
        <span class="dot ${cls}"></span>
      </div>
      <div class="kind">${escapeHtml(r.kind)} · ${escapeHtml(r.id)}</div>
      <div class="meta">
        <span>${r.connected ? `@ ${r.hostname || r.address || "?"}` : (r.connected_since_ms > 0 ? "dropped" : "never seen")}</span>
      </div>
      ${active}`;
    card.onclick = () => selectRobot(r.id);
    grid.appendChild(card);
  }
  if (state.selectedRobot && !state.robots.has(state.selectedRobot)) {
    state.selectedRobot = null;
    updateLogHeader();
  }
  populateTargetSelect();
}

function renderRuns() {
  const box = $("runs");
  box.innerHTML = "";
  for (const run of state.runs.slice(0, 20)) {
    const statuses = run.robots
      .map(([robot, st]) => `<span class="rs ${st.status}">${escapeHtml(robot)}: ${st.status}</span>`)
      .join("");
    const el = document.createElement("div");
    el.className = "run";
    let header = `<div class="action">${escapeHtml(run.action)}</div>`;
    if (run.workflow) {
      const wf = run.workflow;
      const pct = wf.total_steps > 0 ? Math.round((wf.current_step / wf.total_steps) * 100) : 0;
      header += `<div class="muted">step ${wf.current_step}/${wf.total_steps}: ${escapeHtml(wf.step_action || "…")}</div>`;
      header += `<div class="wf-progress"><div class="wf-bar" style="width:${pct}%"></div></div>`;
    }
    el.innerHTML = header +
      (run.created_ms != null ? `<div class="muted">${formatTime(run.created_ms)}</div>` : "") +
      `<div class="status">${statuses}</div>`;
    // Still in flight somewhere → offer a kill switch for exactly those robots.
    const running = run.robots.filter(([, st]) => st.status === "running").map(([robot]) => robot);
    if (running.length) {
      const btn = document.createElement("button");
      btn.className = "kill";
      btn.textContent = "Kill";
      btn.onclick = () => killRun(run.run_id, running, btn);
      el.appendChild(btn);
    }
    box.appendChild(el);
  }
}

// Ask for confirmation, then POST /api/stop scoped to this run's robots.
async function killRun(runId, robots, btn) {
  showModal(
    "Kill running action",
    `Send stop to: ${robots.join(", ")}\n(run ${runId})`,
    async () => {
      btn.disabled = true;
      btn.textContent = "stopping…";
      try {
        await fetchJson("/api/stop", {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ targets: { robots }, confirm: true }),
        });
        // The WS `run` event re-renders the entry once statuses flip.
      } catch (e) {
        showModal("Stop failed", String(e.message || e));
      } finally {
        btn.disabled = false;
        btn.textContent = "Kill";
      }
    }
  );
}

function populateActionSelect() {
  const sel = $("run-action");
  const groups = [
    ["Workflows", state.actions.workflows, true],
    ["Type Workflows", state.actions.typeWorkflows, true],
    ["Swarm Tasks", state.actions.swarm, false],
    ["Robot Tasks", state.actions.robotType, false],
  ];
  sel.innerHTML = `<option value="">choose an action…</option>`;
  for (const [label, items, isWorkflow] of groups) {
    if (!items.length) continue;
    const og = document.createElement("optgroup");
    og.label = label;
    for (const a of items) {
      const opt = document.createElement("option");
      opt.value = isWorkflow ? `workflow:${a}` : a;
      opt.textContent = a;
      og.appendChild(opt);
    }
    sel.appendChild(og);
  }
}

function populateTargetSelect() {
  const typeGroup = $("run-type-opts");
  const robotGroup = $("run-robot-opts");
  const prev = $("run-targets").value;
  typeGroup.innerHTML = "";
  robotGroup.innerHTML = "";
  const types = [...new Set([...state.robots.values()].map((r) => r.kind))].sort();
  for (const t of types) {
    const opt = document.createElement("option");
    opt.value = `type:${t}`;
    opt.textContent = `all ${t}`;
    typeGroup.appendChild(opt);
  }
  const robots = [...state.robots.values()].sort((a, b) => naturalCompare(a.id, b.id));
  for (const r of robots) {
    const opt = document.createElement("option");
    opt.value = `robot:${r.id}`;
    opt.textContent = r.id + (r.name !== r.id ? ` (${r.name})` : "") + (r.connected ? "" : " — offline");
    robotGroup.appendChild(opt);
  }
  // optgroups have no `.options`; collect their <option> children directly.
  const typeOpts = [...typeGroup.children].filter((n) => n.tagName === "OPTION");
  const robotOpts = [...robotGroup.children].filter((n) => n.tagName === "OPTION");
  if ([...typeOpts, ...robotOpts].some((o) => o.value === prev)) {
    $("run-targets").value = prev;
  }
}

// ---------------------------------------------------------------------------
// WebSocket
// ---------------------------------------------------------------------------
function connectWs() {
  const proto = location.protocol === "https:" ? "wss" : "ws";
  const ws = new WebSocket(`${proto}://${location.host}/api/ws`);
  let opened = false;
  // If the socket never opens (host down), stop showing "connecting…" and
  // fall back to "offline" until the retry succeeds.
  const openedTimeout = setTimeout(() => { if (!opened) setConn(false); }, 3000);
  ws.onopen = () => { opened = true; clearTimeout(openedTimeout); setConn(true); };
  ws.onclose = () => { clearTimeout(openedTimeout); setConn(false); setTimeout(connectWs, 2000); };
  ws.onerror = () => ws.close();
  ws.onmessage = (ev) => {
    let msg;
    try { msg = JSON.parse(ev.data); } catch { return; }
    switch (msg.type) {
      case "robots":
        state.robots = new Map(msg.robots.map((r) => [r.id, r]));
        renderRobots();
        break;
      case "robot":
        detectConnectChange(msg.robot);
        state.robots.set(msg.robot.id, msg.robot);
        renderRobots();
        break;
      case "runs":
        state.runs = msg.runs;
        renderRuns();
        break;
      case "run":
        upsertRun(msg.run);
        renderRuns();
        break;
      case "logs":
        if (msg.robot === state.selectedRobot) appendLogs(msg.lines);
        break;
    }
  };
}

function upsertRun(run) {
  const i = state.runs.findIndex((r) => r.run_id === run.run_id);
  if (i >= 0) state.runs[i] = run;
  else state.runs.unshift(run);
}

function detectConnectChange(robot) {
  const prev = state.prevConnected.get(robot.id);
  const now = robot.connected;
  state.prevConnected.set(robot.id, now);
  if (prev === undefined) return; // first time seeing this robot
  if (prev === now) return;
  const ts = Date.now();
  const line = {
    ts_ms: ts,
    stderr: !now,
    text: now
      ? `robot ${robot.id} connected`
      : `robot ${robot.id} disconnected`,
  };
  if (!state.connectEvents.has(robot.id)) {
    state.connectEvents.set(robot.id, []);
  }
  state.connectEvents.get(robot.id).push(line);
  // If this robot's log panel is open, append directly.
  if (state.selectedRobot === robot.id) appendLogs([line]);
}

function setConn(up) {
  const el = $("conn");
  el.className = "conn " + (up ? "online" : "offline");
  el.textContent = up ? "online" : "offline";
}

// ---------------------------------------------------------------------------
// Dispatch form
// ---------------------------------------------------------------------------
function bindRunForm() {
  $("run-targets").onchange = (e) => {
    $("run-robots-label").hidden = e.target.value !== "custom";
  };
  $("run-form").onsubmit = (ev) => {
    ev.preventDefault();
    dispatch(false);
  };

  async function dispatch(confirm) {
    const out = $("run-result");
    out.className = "result";
    out.textContent = "";

    const action = $("run-action").value;
    if (!action) {
      showModal("No action", "Choose an action first, then submit again.");
      return;
    }

    // Workflow dispatch: separate endpoint, no targets needed.
    if (action.startsWith("workflow:")) {
      const workflowName = action.slice("workflow:".length);
      const btn = $("run-submit");
      btn.disabled = true;
      btn.textContent = "Sending…";
      try {
        const resp = await fetchJson("/api/workflow", {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ workflow: workflowName, confirm }),
        });
        out.className = "result ok";
        out.textContent = `workflow ${workflowName}\nrun ${resp.run_id}`;
      } catch (e) {
        const text = String(e.message || e);
        out.className = "result bad";
        out.textContent = text;
        if (text.includes("confirm with confirm=true")) {
          showModal(
            "Confirmation required",
            "This workflow contains dangerous actions targeting multiple robots.\nRun it anyway?",
            () => dispatch(true)
          );
        } else {
          showModal("Dispatch failed", text);
        }
      } finally {
        btn.disabled = false;
        btn.textContent = "Run";
      }
      return;
    }

    const sel = $("run-targets").value;
    const custom = $("run-robots").value.split(",").map((s) => s.trim()).filter(Boolean);
    let targets;
    if (sel === "all") {
      // The host resolves targets against the static config, so "all" would
      // include offline robots. Send an explicit online-only list instead.
      const online = [...state.robots.values()].filter((r) => r.connected).map((r) => r.id);
      if (online.length === 0) {
        out.className = "result bad";
        out.textContent = "no robots are online";
        return;
      }
      targets = { robots: online };
    }
    else if (sel.startsWith("type:")) targets = { types: [sel.slice(5)] };
    else if (sel.startsWith("robot:")) targets = { robots: [sel.slice(6)] };
    else targets = { robots: custom };
    const payload = {
      action,
      targets,
      confirm,
    };
    const timeout = parseInt($("run-timeout").value, 10);
    if (timeout > 0) payload.timeout_sec = timeout;

    const btn = $("run-submit");
    btn.disabled = true;
    btn.textContent = "Sending…";
    try {
      const resp = await fetchJson("/api/run", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(payload),
      });
      out.className = "result ok";
      out.textContent =
        `run ${resp.run_id}\n→ ${resp.targeted.join(", ") || "none"}\n` +
        (resp.busy.length ? `busy: ${resp.busy.join(", ")}\n` : "") +
        (resp.offline.length ? `offline: ${resp.offline.join(", ")}` : "");
    } catch (e) {
      const text = String(e.message || e);
      out.className = "result bad";
      out.textContent = text;
      if (text.includes("confirm with confirm=true")) {
        // The host only asks when the action is flagged dangerous and targets
        // more than one robot. Confirm via the modal, then resubmit.
        showModal(
          "Confirmation required",
          "This action is flagged dangerous and targets multiple robots.\nRun it anyway?",
          () => dispatch(true)
        );
      } else {
        showModal("Dispatch failed", text);
      }
    } finally {
      btn.disabled = false;
      btn.textContent = "Run";
    }
  }
}

function showModal(title, text, onOk) {
  $("modal-title").textContent = title;
  $("modal-text").textContent = text;
  // With a callback this is a confirmation: Cancel + Proceed. Without one it
  // is a plain alert with a single OK.
  $("modal-close").textContent = onOk ? "Proceed" : "OK";
  $("modal-cancel").hidden = !onOk;
  $("modal").classList.remove("hidden");
  state.modalOk = onOk || null;
  $("modal-close").focus();
}

function bindModal() {
  const hide = () => $("modal").classList.add("hidden");
  $("modal-close").onclick = () => {
    hide();
    const cb = state.modalOk;
    state.modalOk = null;
    if (cb) cb();
  };
  $("modal-cancel").onclick = () => {
    hide();
    state.modalOk = null;
  };
  $("modal").onclick = (e) => {
    if (e.target === $("modal")) {
      hide();
      state.modalOk = null;
    }
  };
}

// ---------------------------------------------------------------------------
// Logs
// ---------------------------------------------------------------------------
function selectRobot(id) {
  state.selectedRobot = id;
  updateLogHeader();
  loadLogs();
  renderRobots();
}

function updateLogHeader() {
  $("log-robot").textContent = state.selectedRobot ? `— ${state.selectedRobot}` : "";
}

async function loadLogs() {
  const view = $("log-view");
  view.innerHTML = "";
  if (!state.selectedRobot) return;
  try {
    const lines = await fetchJson(`/api/robots/${encodeURIComponent(state.selectedRobot)}/logs`);
    // Merge in per-robot connection events, sorted by timestamp.
    const events = state.connectEvents.get(state.selectedRobot) || [];
    const merged = [...lines, ...events].sort((a, b) => a.ts_ms - b.ts_ms);
    appendLogs(merged);
  } catch (e) {
    view.innerHTML = `<div class="line stderr">failed to load logs: ${escapeHtml(String(e))}</div>`;
  }
}

function appendLogs(lines) {
  const view = $("log-view");
  const wasBottom = !state.follow || Math.abs(view.scrollHeight - view.scrollTop - view.clientHeight) < 30;
  for (const l of lines) {
    const div = document.createElement("div");
    div.className = "line" + (l.stderr ? " stderr" : "");
    div.innerHTML =
      `<span class="ts">${formatTime(l.ts_ms)}</span> ` +
      ansiToHtml(l.text);
    view.appendChild(div);
  }
  if (wasBottom) view.scrollTop = view.scrollHeight;
}

function bindLogCopy() {
  const btn = $("log-copy");
  btn.onclick = async () => {
    const lines = [...$("log-view").querySelectorAll(".line")];
    if (!lines.length) return;
    // Copy exactly what is visible, timestamps included.
    const text = lines.map((el) => el.textContent).join("\n");
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      // Clipboard API needs a secure context; fall back for plain-HTTP LANs.
      const ta = document.createElement("textarea");
      ta.value = text;
      document.body.appendChild(ta);
      ta.select();
      document.execCommand("copy");
      ta.remove();
    }
    btn.textContent = "copied!";
    setTimeout(() => { btn.textContent = "Copy"; }, 1200);
  };
}

function bindLogDownload() {
  const btn = $("log-download");
  btn.onclick = () => {
    const lines = [...$("log-view").querySelectorAll(".line")];
    if (!lines.length) return;
    const text = lines.map((el) => el.textContent).join("\n");
    const robot = state.selectedRobot
      ? (state.robots.get(state.selectedRobot)?.name || state.selectedRobot)
      : "logs";
    const ts = new Date().toISOString().replace(/[:.]/g, "-").slice(0, 19);
    const filename = `${robot}-${ts}.log`;
    const blob = new Blob([text], { type: "text/plain" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = filename;
    document.body.appendChild(a);
    a.click();
    a.remove();
    URL.revokeObjectURL(url);
  };
}

function bindLogFollow() {
  const btn = $("log-follow");
  btn.onclick = () => {
    state.follow = !state.follow;
    btn.textContent = state.follow ? "following…" : "not following";
  };
}

// ---------------------------------------------------------------------------
// Theme
// ---------------------------------------------------------------------------
function bindThemeToggle() {
  $("theme-toggle").onclick = () => {
    const root = document.documentElement;
    const next = root.dataset.theme === "light" ? "dark" : "light";
    root.dataset.theme = next;
    try { localStorage.setItem("theme", next); } catch {}
  };
}

// ---------------------------------------------------------------------------
// Clear logs & runs
// ---------------------------------------------------------------------------
function bindClearAll() {
  $("clear-all").onclick = () => {
    showModal(
      "Clear logs & runs",
      "Clear all logs and run history?\nThis cannot be undone.",
      async () => {
        try {
          await fetch("/api/clear", { method: "POST" });
          $("log-view").innerHTML = "";
          state.runs = [];
          state.connectEvents.clear();
          renderRuns();
        } catch (e) {
          showModal("Clear failed", String(e.message || e));
        }
      }
    );
  };
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------
function naturalCompare(a, b) {
  // Numeric-aware ordering so ids sort as sim-1, sim-2, …, sim-10.
  return a.localeCompare(b, undefined, { numeric: true });
}

function formatTime(ms) {
  const d = new Date(ms);
  const pad = (n) => String(n).padStart(2, "0");
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}

function escapeHtml(s) {
  return String(s).replace(/[&<>"']/g, (c) => ({
    "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;",
  }[c]));
}

// ---------------------------------------------------------------------------
// ANSI escape codes
// ---------------------------------------------------------------------------
// Agent output carries SGR sequences (e.g. \x1b[32m…\x1b[0m). Convert them to
// styled spans. Text is HTML-escaped FIRST and only then scanned, so no
// untrusted markup can slip through; the ESC byte itself survives escaping.

const ANSI_SGR_SPLIT = /(\x1b\[[0-9;]*m)/;

function ansiToHtml(raw) {
  // Strip other CSI sequences (cursor moves etc.), keep only …m (SGR).
  const text = escapeHtml(raw).replace(/\x1b\[[0-9;]*[A-LN-Za-ln-z]/g, "");
  let out = "";
  let cur = "";
  for (const part of text.split(ANSI_SGR_SPLIT)) {
    const m = part.match(/^\x1b\[([0-9;]*)m$/);
    if (!m) {
      out += part;
      continue;
    }
    const sig = ansiSignature(m[1]);
    if (sig !== cur) {
      if (cur) out += "</span>";
      cur = sig;
      if (sig) out += `<span class="${sig}">`;
    }
  }
  if (cur) out += "</span>";
  return out;
}

// Map SGR params to a CSS class list; returns "" when nothing is active.
function ansiSignature(params) {
  const codes = params === "" ? [0] : params.split(";").map((s) => (s === "" ? 0 : parseInt(s, 10)));
  let bold = false, italic = false, under = false, fg = null, bg = null;
  for (const c of codes) {
    if (c === 0) { bold = italic = under = false; fg = bg = null; }
    else if (c === 1 || c === 2) bold = true;
    else if (c === 3) italic = true;
    else if (c === 4) under = true;
    else if (c === 22) bold = false;
    else if (c === 23) italic = false;
    else if (c === 24) under = false;
    else if (c >= 30 && c <= 37) fg = c - 30;
    else if (c === 39) fg = null;
    else if (c >= 90 && c <= 97) fg = c - 90 + 8;
    else if (c >= 40 && c <= 47) bg = c - 40;
    else if (c === 49) bg = null;
    else if (c >= 100 && c <= 107) bg = c - 100 + 8;
    // Extended colors (38;5;n / 38;2;r;g;b) are ignored.
  }
  const cls = [];
  if (bold) cls.push("ansi-b");
  if (italic) cls.push("ansi-i");
  if (under) cls.push("ansi-u");
  if (fg !== null) cls.push(`ansi-fg${fg}`);
  if (bg !== null) cls.push(`ansi-bg${bg}`);
  return cls.join(" ");
}

init();
