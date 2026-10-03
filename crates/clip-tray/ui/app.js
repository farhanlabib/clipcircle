const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const $ = (id) => document.getElementById(id);

/** Which system's look to use (see style.css). */
const os = /Mac/.test(navigator.userAgent) ? "macos" : /Windows/.test(navigator.userAgent) ? "windows" : "linux";
document.documentElement.dataset.os = os;
const thisDevice = { macos: "this Mac", windows: "this PC", linux: "this computer" }[os];
if (os === "windows") $("me-glyph").setAttribute("href", "#i-desktop");

function el(tag, props = {}, ...children) {
  const node = Object.assign(document.createElement(tag), props);
  node.append(...children.filter((c) => c != null));
  return node;
}

function icon(name, cls = "icon") {
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("class", cls);
  svg.setAttribute("aria-hidden", "true");
  const use = document.createElementNS("http://www.w3.org/2000/svg", "use");
  use.setAttribute("href", `#i-${name}`);
  svg.append(use);
  return svg;
}

function showError(message) {
  const box = $("error");
  box.textContent = message || "";
  box.hidden = !message;
}

function ago(secs) {
  if (secs < 60) return "just now";
  if (secs < 3600) return `${Math.floor(secs / 60)} min ago`;
  if (secs < 86400) return `${Math.floor(secs / 3600)} h ago`;
  return `${Math.floor(secs / 86400)} d ago`;
}

// --- views: home, pairing, joining ------------------------------------------

function showView(name) {
  $("home-view").hidden = name !== "home";
  $("pair-view").hidden = name !== "pair";
  $("join-view").hidden = name !== "join";
}

document.querySelectorAll(".back").forEach((b) => (b.onclick = () => showView("home")));

// --- this device and the circle ---------------------------------------------

/** Results of the last "Check connections", by device id. */
let checks = {};
let deviceName = "";

async function refreshStatus() {
  try {
    const s = await invoke("status");
    deviceName = s.device.name;
    $("device-name").textContent = s.device.name;
    $("sync-toggle").checked = !s.paused;
    const nearby = s.members.filter((m) => m.online).length;
    $("sync-state").textContent = s.paused
      ? "Syncing paused"
      : s.members.length === 0
        ? "Ready to sync on this network"
        : `Syncing · ${nearby} of ${s.members.length} ${s.members.length === 1 ? "device" : "devices"} nearby`;
    $("devices-heading").textContent = s.members.length ? `Devices · ${s.members.length}` : "Devices";
    $("members").hidden = s.members.length === 0;
    $("empty-circle").hidden = s.members.length > 0;
    $("members").replaceChildren(...s.members.map(memberRow));
    showError("");
  } catch (e) {
    $("sync-state").textContent = "Not syncing";
    showError(e);
  }
}

function memberRow(m) {
  const check = checks[m.id];
  const name = el("div", { className: "t ellipsis" }, m.name, el("span", { className: m.online ? "dot on" : "dot", title: m.online ? "On this network" : "Not seen on this network" }));
  const sub = el("div", { className: "s" }, m.online ? "On this network" : "Not seen on this network");
  if (check) sub.append(el("span", { className: check.ok ? "detail" : "detail bad", textContent: check.detail }));
  const more = el("button", { className: "ghost icon-only muted", ariaLabel: `More for ${m.name}`, title: "More" }, icon("more"));
  more.setAttribute("aria-haspopup", "menu");
  more.onclick = (ev) => openDeviceMenu(ev.currentTarget, m);
  const lead = icon("laptop", `icon lead ${m.online ? "on" : "off"}`);
  return el("li", { className: "row" }, lead, el("div", { className: "grow" }, name, sub), more);
}

// The "…" menu on a device row.
let menuDevice = null;
function openDeviceMenu(button, device) {
  const menu = $("device-menu");
  menuDevice = device;
  const r = button.getBoundingClientRect();
  menu.hidden = false;
  menu.style.top = `${Math.min(r.bottom + 4, window.innerHeight - menu.offsetHeight - 8)}px`;
  menu.style.left = `${Math.max(8, r.right - menu.offsetWidth)}px`;
  $("remove-btn").focus();
}
function closeDeviceMenu() {
  $("device-menu").hidden = true;
  menuDevice = null;
}
document.addEventListener("click", (ev) => {
  if (!$("device-menu").hidden && !ev.target.closest("#device-menu, [aria-haspopup]")) closeDeviceMenu();
});
document.addEventListener("keydown", (ev) => {
  if (ev.key === "Escape") closeDeviceMenu();
});
$("remove-btn").onclick = async () => {
  const device = menuDevice;
  closeDeviceMenu();
  if (!device || !confirm(`Remove ${device.name} from your circle?`)) return;
  try {
    await invoke("remove_device", { id: device.id });
    await refreshStatus();
  } catch (e) {
    showError(e);
  }
};

$("check-btn").onclick = async () => {
  const btn = $("check-btn");
  const label = btn.querySelector("span");
  btn.disabled = true;
  label.textContent = "Checking…";
  try {
    const results = await invoke("check_devices");
    checks = Object.fromEntries(results.map((r) => [r.id, r]));
  } catch (e) {
    showError(e);
  }
  btn.disabled = false;
  label.textContent = "Check connections";
  refreshStatus();
};

$("sync-toggle").onchange = async (ev) => {
  try {
    await invoke("set_paused", { paused: !ev.target.checked });
  } catch (e) {
    showError(e);
  }
  refreshStatus();
};

// --- recent clips -------------------------------------------------------------

/** Index of the entry just copied back, shown as "Copied" for a moment. */
let copied = null;

async function refreshHistory() {
  let entries = [];
  try {
    entries = await invoke("history");
  } catch {
    return;
  }
  const list = $("history");
  if (entries.length === 0) {
    list.replaceChildren(el("li", { className: "row empty-row", textContent: "Copy something and it shows up here." }));
    return;
  }
  list.replaceChildren(
    ...entries.slice(0, 20).map((e, index) => {
      const from = e.from === deviceName ? `From ${thisDevice}` : `From ${e.from}`;
      const body = el("div", { className: "grow" }, el("div", { className: "t ellipsis", textContent: e.preview }), el("div", { className: "s", textContent: `${from} · ${ago(e.secs_ago)}` }));
      const kind = { link: "link", image: "image", files: "files" }[e.kind] || "text";
      const tail = copied === index ? el("span", { className: "s meta-copied" }, icon("check", "icon small"), "Copied") : null;
      if (!e.can_copy) return el("li", { className: "row" }, icon(kind, "icon lead"), body);
      const button = el("button", { className: "row", title: "Copy again" }, icon(kind, "icon lead"), body, tail);
      button.onclick = async () => {
        try {
          await invoke("copy_history", { index });
          copied = index;
          refreshHistory();
          setTimeout(() => {
            if (copied === index) copied = null;
          }, 1500);
        } catch (err) {
          showError(err);
        }
      };
      return el("li", {}, button);
    }),
  );
}

// --- adding a device ------------------------------------------------------------

function setPairStatus(text, waiting) {
  const status = $("pair-status");
  status.replaceChildren(...(waiting ? [el("span", { className: "spinner", ariaHidden: "true" })] : []), el("span", { textContent: text }));
}

$("pair-btn").onclick = async () => {
  showView("pair");
  $("pair-code").replaceChildren();
  setPairStatus("Getting a code…", true);
  try {
    const code = await invoke("start_pairing");
    $("pair-code").replaceChildren(...[...code].map((d) => el("div", { className: "digit", textContent: d })));
    $("pair-code").setAttribute("aria-label", `Pairing code ${[...code].join(" ")}`);
    setPairStatus("Waiting for the other device…", true);
  } catch (e) {
    setPairStatus(`Could not show a code: ${e}`, false);
  }
};

listen("paired", (ev) => {
  setPairStatus(`${ev.payload} joined your circle.`, false);
  refreshStatus();
});
listen("pairing-failed", (ev) => setPairStatus(`Pairing failed: ${ev.payload}`, false));

// --- joining a circle ---------------------------------------------------------

const digitInputs = Array.from({ length: 6 }, (_, i) =>
  el("input", { className: "digit", inputMode: "numeric", maxLength: 1, autocomplete: "off", ariaLabel: `Digit ${i + 1}` }),
);
$("join-digits").append(...digitInputs);

function joinCode() {
  return digitInputs.map((d) => d.value).join("");
}

function fillFrom(start, text) {
  const digits = text.replace(/\D/g, "").slice(0, 6 - start);
  [...digits].forEach((d, i) => (digitInputs[start + i].value = d));
  const next = Math.min(start + digits.length, 5);
  digitInputs[next].focus();
  $("join-submit").disabled = joinCode().length !== 6;
}

digitInputs.forEach((input, i) => {
  input.addEventListener("input", () => {
    const typed = input.value;
    input.value = "";
    fillFrom(i, typed);
  });
  input.addEventListener("keydown", (ev) => {
    if (ev.key === "Backspace" && !input.value && i > 0) {
      digitInputs[i - 1].value = "";
      digitInputs[i - 1].focus();
      $("join-submit").disabled = true;
      ev.preventDefault();
    }
  });
  input.addEventListener("paste", (ev) => {
    ev.preventDefault();
    fillFrom(i, ev.clipboardData.getData("text"));
  });
});

$("join-btn").onclick = () => {
  showView("join");
  digitInputs.forEach((d) => (d.value = ""));
  $("join-submit").disabled = true;
  $("join-status").textContent = "Both devices need to be on the same network.";
  digitInputs[0].focus();
};

$("join-view").onsubmit = async (ev) => {
  ev.preventDefault();
  const code = joinCode();
  if (!/^\d{6}$/.test(code)) return;
  $("join-status").textContent = "Looking for the device…";
  $("join-submit").disabled = true;
  try {
    const n = await invoke("join", { code });
    $("join-status").textContent = `Joined. ${n} other ${n === 1 ? "device" : "devices"} in the circle.`;
    refreshStatus();
    setTimeout(() => showView("home"), 1200);
  } catch (e) {
    $("join-status").textContent = String(e);
    $("join-submit").disabled = false;
  }
};

// --- settings -----------------------------------------------------------------

$("settings-btn").onclick = () => {
  const open = $("settings").hidden;
  $("settings").hidden = !open;
  $("settings-btn").setAttribute("aria-expanded", String(open));
  if (open) $("settings").scrollIntoView({ block: "nearest", behavior: "smooth" });
};

async function refreshAutostart() {
  try {
    $("autostart-toggle").checked = await invoke("autostart");
  } catch (e) {
    showError(e);
  }
}

$("autostart-toggle").onchange = async (ev) => {
  try {
    await invoke("set_autostart", { enabled: ev.target.checked });
  } catch (e) {
    showError(e);
  }
  refreshAutostart();
};

$("quit-btn").onclick = () => invoke("quit");

listen("paused-changed", refreshStatus);
listen("service-error", (ev) => showError(ev.payload));
listen("autostart-changed", refreshAutostart);

refreshStatus().then(refreshHistory);
refreshAutostart();
setInterval(refreshHistory, 2000);
setInterval(refreshStatus, 5000);
