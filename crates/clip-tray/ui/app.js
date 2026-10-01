const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const $ = (id) => document.getElementById(id);

function showError(message) {
  const el = $("error");
  el.textContent = message || "";
  el.hidden = !message;
}

function ago(secs) {
  if (secs < 60) return "just now";
  if (secs < 3600) return `${Math.floor(secs / 60)}m ago`;
  if (secs < 86400) return `${Math.floor(secs / 3600)}h ago`;
  return `${Math.floor(secs / 86400)}d ago`;
}

async function refreshStatus() {
  try {
    const s = await invoke("status");
    $("device-name").textContent = s.device.name;
    $("sync-toggle").checked = !s.paused;
    $("sync-state").textContent = s.paused ? "Syncing paused" : "Syncing on this network";
    const list = $("members");
    list.replaceChildren();
    if (s.members.length === 0) {
      const li = document.createElement("li");
      li.innerHTML = '<span class="muted">No other devices yet. Add one below.</span>';
      list.append(li);
    }
    for (const m of s.members) {
      const li = document.createElement("li");
      const name = document.createElement("span");
      name.textContent = m.name;
      const remove = document.createElement("button");
      remove.className = "link";
      remove.textContent = "Remove";
      remove.onclick = async () => {
        if (!confirm(`Remove ${m.name} from your circle?`)) return;
        try { await invoke("remove_device", { id: m.id }); await refreshStatus(); }
        catch (e) { showError(e); }
      };
      li.append(name, remove);
      list.append(li);
    }
    showError("");
  } catch (e) {
    $("sync-state").textContent = "Not syncing";
    showError(e);
  }
}

async function refreshHistory() {
  let entries = [];
  try { entries = await invoke("history"); } catch { return; }
  const list = $("history");
  list.replaceChildren();
  if (entries.length === 0) {
    const li = document.createElement("li");
    li.innerHTML = '<span class="muted">Copy something and it shows up here.</span>';
    list.append(li);
  }
  entries.forEach((e, index) => {
    const li = document.createElement("li");
    const text = document.createElement("span");
    text.className = "text";
    text.textContent = e.preview;
    const meta = document.createElement("span");
    meta.className = "meta";
    meta.textContent = `${e.from} · ${ago(e.secs_ago)}`;
    li.append(text, meta);
    if (e.can_copy) {
      li.classList.add("copyable");
      li.title = "Click to copy again";
      li.onclick = async () => {
        try { await invoke("copy_history", { index }); meta.textContent = "Copied"; }
        catch (err) { showError(err); }
      };
    }
    list.append(li);
  });
}

$("sync-toggle").onchange = async (ev) => {
  try { await invoke("set_paused", { paused: !ev.target.checked }); }
  catch (e) { showError(e); }
  refreshStatus();
};

$("pair-btn").onclick = async () => {
  $("join-panel").hidden = true;
  try {
    const code = await invoke("start_pairing");
    $("pair-code").textContent = code;
    $("pair-status").textContent = "Waiting for the other device…";
    $("pair-panel").hidden = false;
  } catch (e) { showError(e); }
};

$("join-btn").onclick = () => {
  $("pair-panel").hidden = true;
  $("join-panel").hidden = false;
  $("join-status").textContent = "";
  $("join-code").focus();
};

$("join-panel").onsubmit = async (ev) => {
  ev.preventDefault();
  const code = $("join-code").value.trim();
  if (!/^\d{6}$/.test(code)) { $("join-status").textContent = "The code has 6 digits."; return; }
  $("join-status").textContent = "Looking for the device…";
  try {
    const n = await invoke("join", { code });
    $("join-status").textContent = `Joined. ${n} other device(s) in the circle.`;
    $("join-code").value = "";
    refreshStatus();
  } catch (e) {
    $("join-status").textContent = String(e);
  }
};

listen("paired", (ev) => {
  $("pair-status").textContent = `${ev.payload} joined your circle.`;
  refreshStatus();
});
listen("pairing-failed", (ev) => { $("pair-status").textContent = `Pairing failed: ${ev.payload}`; });
listen("paused-changed", refreshStatus);
listen("service-error", (ev) => showError(ev.payload));

refreshStatus();
refreshHistory();
setInterval(refreshHistory, 2000);
setInterval(refreshStatus, 5000);
