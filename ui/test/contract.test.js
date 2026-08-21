#!/usr/bin/env node
"use strict";

// Contract test for the WebUI: loads the REAL ui/static/app.js against fixture
// JSON matching docs/api.md (same wire shapes the host serves) and asserts the
// UI renders, the action list populates, and the WebSocket connects. This is
// the regression guard for the `robot_type`/`robotType` class of bug.
//
// Run: node ui/test/contract.test.js   (or `just test-webui`)

const fs = require("fs");
const path = require("path");

const SRC = fs.readFileSync(
  path.join(__dirname, "..", "static", "app.js"),
  "utf8"
);

// --- minimal DOM ------------------------------------------------------------
class El {
  constructor(tag, id) {
    this.tag = tag;
    this.id = id || "";
    this._html = "";
    this.textContent = "";
    this.className = "";
    this.value = "";
    this.hidden = false;
    this.disabled = false;
    this.checked = false;
    this.label = "";
    this.onclick = null;
    this.onchange = null;
    this.onsubmit = null;
    this.onmessage = null;
    this.scrollHeight = 0;
    this.scrollTop = 0;
    this.clientHeight = 0;
    this.children = [];
    this._cls = new Set();
    this.classList = {
      add: (c) => this._cls.add(c),
      remove: (c) => this._cls.delete(c),
      contains: (c) => this._cls.has(c),
    };
  }
  // Only <select> has `.options` (real DOM; <optgroup> does not). For selects,
  // flatten optgroup-nested options since run-action lives inside <optgroup>s.
  get options() {
    if (this.tag !== "select") return undefined;
    const out = [];
    const walk = (n) => {
      for (const c of n.children) {
        if (c.tag === "option") out.push(c);
        else if (c.children) walk(c);
      }
    };
    walk(this);
    return out;
  }
  set innerHTML(v) {
    this._html = String(v);
    this.children = [];
    if (this.tag === "select" && String(v).includes("<option")) {
      // Rebuild from inline markup so `<option value="x">` works.
      const re = /<option value="([^"]*)"[^>]*>([^<]*)<\/option>/g;
      let m;
      while ((m = re.exec(String(v)))) {
        const o = new El("option");
        o.value = m[1];
        o.textContent = m[2];
        this.children.push(o);
      }
    }
  }
  get innerHTML() {
    return this._html;
  }
  appendChild(c) {
    this.children.push(c);
  }
  closest() {
    return new El("label");
  }
  focus() {}
}

const els = {};
// Element tags as declared in ui/index.html (matters: only <select> has
// `.options`, <optgroup> does not).
const TAGS = {
  "run-action": "select",
  "run-targets": "select",
  "run-type-opts": "optgroup",
  "run-robot-opts": "optgroup",
  "run-robots": "input",
  "run-timeout": "input",
  "run-submit": "button",
  "run-robots-label": "label",
  "log-follow": "button",
  "log-view": "div",
  "log-robot": "span",
  "conn": "div",
  "controller-name": "span",
  "modal": "div",
  "modal-title": "h3",
  "modal-text": "p",
  "modal-close": "button",
  "robots": "div",
  "runs": "div",
  "run-result": "div",
};
const getEl = (id) => {
  if (!els[id]) els[id] = new El(TAGS[id] || "div", id);
  return els[id];
};

global.document = {
  getElementById: getEl,
  createElement: (t) => new El(t),
  title: "",
};
global.location = { protocol: "http:", host: "127.0.0.1:18082" };

// --- fixtures (docs/api.md shapes) -----------------------------------------
const ROBOTS = [
  {
    id: "sim-01",
    name: "sim-robot-1",
    kind: "sim",
    address: null,
    simulated: true,
    adopted: false,
    connected: true,
    agent_version: "0.1.0",
    hostname: "uos-24rjd44",
    last_seen_ms: 1,
    active: null,
  },
  {
    id: "sim-02",
    name: "sim-robot-2",
    kind: "sim",
    address: null,
    simulated: true,
    adopted: false,
    connected: false,
    agent_version: "0.1.0",
    hostname: "uos-24rjd44",
    last_seen_ms: 0,
    active: null,
  },
];
const ACTIONS = {
  robot_type: ["sim.echo", "sim.slow_echo", "turtlebot3.bringup"],
  swarm: ["trial", "trial_danger"],
};
const CONFIG = {
  controller: "lab",
  robot_types: ["sim", "turtlebot3"],
  robot_count: 2,
  grpc_listen: "0.0.0.0:50051",
  ui_bind: "0.0.0.0:18082",
};
const RUNS = [
  {
    run_id: "run-abc",
    action: "sim.echo",
    created_ms: 0,
    robots: [["sim-01", { status: "running", action_id: "a", started_ms: 0 }]],
  },
];

global.fetch = async (url, opts) => {
  if (opts && opts.method === "POST" && url === "/api/run") {
    const body = JSON.parse(opts.body);
    if (body.confirm !== true) {
      return {
        ok: false,
        status: 400,
        json: async () => ({}),
        text: async () =>
          "action 'trial_danger' targets 2 robots and is flagged dangerous; confirm with confirm=true",
      };
    }
    return {
      ok: true,
      status: 200,
      json: async () => ({
        run_id: "run-r2",
        action: "trial_danger",
        targeted: ["sim-01", "sim-02"],
        busy: [],
        offline: [],
      }),
      text: async () => "{}",
    };
  }
  let body;
  switch (url) {
    case "/api/robots":
      body = ROBOTS;
      break;
    case "/api/actions":
      body = ACTIONS;
      break;
    case "/api/config":
      body = CONFIG;
      break;
    case "/api/runs":
      body = RUNS;
      break;
    default:
      body = [];
  }
  return { ok: true, json: async () => body, text: async () => "{}" };
};

let wsCreated = null;
global.WebSocket = class {
  constructor(url) {
    this.url = url;
    this.onopen = null;
    this.onclose = null;
    this.onerror = null;
    this.onmessage = null;
    wsCreated = this;
  }
};

// --- run the real app ------------------------------------------------------
let loadError = null;
try {
  eval(SRC);
} catch (e) {
  loadError = e;
}

setTimeout(() => {
  try {
    if (loadError) throw loadError;

    // Controller name from /api/config, not askama.
    const controller = getEl("controller-name").textContent;
    assertEq(controller, "lab", "controller name from /api/config");

    // Action list populated (was empty: robot_type vs robotType drift).
    const opts = getEl("run-action").options.map((o) => o.value);
    const named = opts.filter((v) => v !== ""); // exclude the placeholder
    assertEq(
      named.length,
      ACTIONS.robot_type.length + ACTIONS.swarm.length,
      "action options count"
    );
    assert(opts.includes("sim.echo"), "robot-type action present");
    assert(opts.includes("trial_danger"), "swarm action present");

    // Targets populated from live robots (optgroup children, no `.options`).
    const targets = getEl("run-robot-opts").children.map((o) => o.value);
    assert(targets.includes("robot:sim-01"), "robot target present");
    assert(targets.includes("robot:sim-02"), "offline robot target present");
    const types = getEl("run-type-opts").children.map((o) => o.value);
    assert(types.includes("type:sim"), "robot-type target present");

    // Robot grid rendered (2 cards).
    assertEq(getEl("robots").children.length, 2, "robot cards");

    // Runs rendered.
    assert(getEl("runs").children.length >= 1, "runs rendered");

    // WebSocket connected → live updates (was stuck on "connecting…").
    assert(wsCreated, "WebSocket created");
    assert(
      wsCreated.url.endsWith("/api/ws"),
      `ws url ends with /api/ws (got ${wsCreated.url})`
    );
    wsCreated.onopen();
    assertEq(getEl("conn").textContent, "online", "conn state after open");

    // Live delta: robot flips offline, UI keeps rendering (no throw).
    wsCreated.onmessage({
      data: JSON.stringify({
        type: "robot",
        robot: { ...ROBOTS[0], connected: false },
      }),
    });
    assertEq(getEl("robots").children.length, 2, "grid survives live delta");
    assertEq(getEl("conn").textContent, "online", "conn still online");

    // Dangerous batch: first attempt (confirm:false) is rejected by the host
    // → modal asks to confirm; OK resubmits with confirm:true. (Replaces the
    // old "I confirm this batch dispatch" checkbox.)
    getEl("run-action").value = "trial_danger";
    getEl("run-targets").value = "all";
    getEl("run-form").onsubmit({ preventDefault() {} });
    setTimeout(() => {
      try {
        assert(
          !getEl("modal").classList.contains("hidden"),
          "confirm modal shown after 400"
        );
        assert(
          getEl("modal-text").textContent.includes("dangerous"),
          "modal explains dangerous batch"
        );
        getEl("modal-close").onclick();
        setTimeout(() => {
          try {
            assertEq(
              getEl("run-result").className,
              "result ok",
              "confirmed resubmit succeeded"
            );
            assert(
              getEl("run-result").textContent.includes("run run-r2"),
              "confirmed run dispatched"
            );
            console.log(
              "PASS: webui contract test (actions, targets, render, WS, confirm)"
            );
            process.exit(0);
          } catch (e) {
            console.error("FAIL:", e.message);
            if (e.stack) console.error(e.stack.split("\n").slice(0, 4).join("\n"));
            process.exit(1);
          }
        }, 150);
      } catch (e) {
        console.error("FAIL:", e.message);
        if (e.stack) console.error(e.stack.split("\n").slice(0, 4).join("\n"));
        process.exit(1);
      }
    }, 150);
  } catch (e) {
    console.error("FAIL:", e.message);
    if (e.stack) console.error(e.stack.split("\n").slice(0, 4).join("\n"));
    process.exit(1);
  }
}, 150);

function assert(cond, msg) {
  if (!cond) throw new Error(`assertion failed: ${msg}`);
}
function assertEq(a, b, msg) {
  if (a !== b) throw new Error(`assertion failed: ${msg} (got ${JSON.stringify(a)}, want ${JSON.stringify(b)})`);
}
