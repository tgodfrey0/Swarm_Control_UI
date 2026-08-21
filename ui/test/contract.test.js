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
  "log-copy": "button",
  "log-view": "div",
  "log-robot": "span",
  "conn": "div",
  "controller-name": "span",
  "modal": "div",
  "modal-title": "h3",
  "modal-text": "p",
  "modal-close": "button",
  "modal-cancel": "button",
  "theme-toggle": "button",
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
// The real app.js touches these (theme toggle); provide minimal stubs.
global.document.documentElement = { dataset: {} };
global.localStorage = {
  _store: {},
  getItem(k) { return this._store[k] ?? null; },
  setItem(k, v) { this._store[k] = String(v); },
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
  {
    id: "sim-9",
    name: "sim-robot-9",
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
    id: "sim-10",
    name: "sim-robot-10",
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

let lastRunPost = null;
let lastStopPost = null;
global.fetch = async (url, opts) => {
  if (opts && opts.method === "POST" && url === "/api/run") {
    lastRunPost = JSON.parse(opts.body);
    if (lastRunPost.confirm !== true) {
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
  if (opts && opts.method === "POST" && url === "/api/stop") {
    lastStopPost = JSON.parse(opts.body);
    return { ok: true, status: 200, json: async () => ["sim-01"], text: async () => "{}" };
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
    assert(targets.includes("robot:sim-10"), "two-digit robot target present");
    const types = getEl("run-type-opts").children.map((o) => o.value);
    assert(types.includes("type:sim"), "robot-type target present");

    // Natural sort: sim-9 before sim-10 (lexicographic would put 10 first).
    assertEq(
      targets.join(","),
      "robot:sim-01,robot:sim-02,robot:sim-9,robot:sim-10",
      "natural sort of robot ids"
    );

    // Robot grid rendered (4 cards).
    assertEq(getEl("robots").children.length, 4, "robot cards");

    // Runs rendered, newest first, stamped with creation time (no run id).
    assert(getEl("runs").children.length >= 1, "runs rendered");
    const runHtml = getEl("runs").children[0].innerHTML;
    assert(
      /\d{2}:\d{2}:\d{2}/.test(runHtml),
      `run entry carries HH:MM:SS timestamp (got ${runHtml})`
    );
    assert(!runHtml.includes("run-abc"), "run id hash not shown");

    // Log copy button is wired up.
    assert(typeof getEl("log-copy").onclick === "function", "log-copy bound");
    assert(typeof getEl("log-follow").onclick === "function", "log-follow bound");

    // ANSI SGR codes become styled spans; raw escapes never reach the DOM.
    // (app.js is eval'd in strict mode above, so its declarations don't leak
    // into this scope — re-instantiate without the boot call to grab helpers.)
    const ui = new Function(
      SRC.replace(/^"use strict";/, "").replace(/init\(\);\s*$/, "") +
      "\nreturn { ansiToHtml };"
    )();
    assertEq(ui.ansiToHtml("plain text"), "plain text", "plain log line untouched");
    const green = ui.ansiToHtml("\x1b[32mINFO\x1b[0m ok");
    assert(!green.includes("\x1b"), "escape bytes stripped");
    assert(green.includes('class="ansi-fg2"'), "green SGR mapped to class");
    assert(green.endsWith("ok"), "text after reset emitted outside span");
    assert(
      ui.ansiToHtml("<script>&</script>").includes("&lt;script&gt;"),
      "html stays escaped inside ansi conversion"
    );
    const gray = ui.ansiToHtml("\x1b[90m[\x1b[0m2026\x1b[90m]\x1b[0m");
    assert(gray.includes('class="ansi-fg8"'), "bright-black SGR mapped to class");

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
    assertEq(getEl("robots").children.length, 4, "grid survives live delta");
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
        // Confirmation mode: Proceed + visible Cancel, not a lone OK.
        assertEq(getEl("modal-close").textContent, "Proceed", "confirm button says Proceed");
        assert(!getEl("modal-cancel").hidden, "Cancel button shown for confirmation");
        // "-- all robots --" must dispatch only to ONLINE robots: sim-02 was
        // offline in the fixture and sim-01 flipped offline via the WS delta
        // above, so neither may be targeted.
        assertEq(
          JSON.stringify(lastRunPost.targets),
          JSON.stringify({ robots: ["sim-9", "sim-10"] }),
          "'all' targets only online robots"
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
            assertEq(lastRunPost.confirm, true, "resubmit carried confirm=true");

            // Kill flow: the still-running fixture run shows a Kill button;
            // confirming it POSTs /api/stop scoped to that run's robots.
            const killBtn = getEl("runs")
              .children[0].children.find((c) => c.className === "kill");
            assert(killBtn && typeof killBtn.onclick === "function", "kill button on running run");
            killBtn.onclick();
            assert(
              !getEl("modal").classList.contains("hidden"),
              "kill asks for confirmation first"
            );
            assert(
              getEl("modal-text").textContent.includes("sim-01"),
              "kill confirm names the targeted robot"
            );
            lastStopPost = null;
            getEl("modal-close").onclick(); // Proceed
            setTimeout(() => {
              try {
                assert(
                  getEl("modal").classList.contains("hidden"),
                  "modal closed after kill"
                );
                assert(lastStopPost, "stop request sent");
                assertEq(
                  JSON.stringify(lastStopPost.targets),
                  JSON.stringify({ robots: ["sim-01"] }),
                  "stop scoped to the run's running robots"
                );
                assertEq(lastStopPost.confirm, true, "stop carried confirm=true");
                console.log(
                  "PASS: webui contract test (actions, targets, render, WS, online-only all, confirm, ansi, kill)"
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
