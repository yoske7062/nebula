// Nebula, inside the League client: pick a skin in champion select.
// The panel talks to the Nebula app on 127.0.0.1, which downloads and applies the skin.

const API = "http://127.0.0.1:47831";
const POS_KEY = "nebula:panel-pos";
const COLLAPSED_KEY = "nebula:panel-collapsed";

const PATCHER_TEXT = {
  Importing: "Preparing skin...",
  BuildingOverlay: "Building skin...",
  WaitingForGame: "Ready. It loads when the game starts.",
  FoundGame: "Game found...",
  Scanning: "Applying...",
  Patching: "Applied",
  InGame: "Applied",
  GameExited: "Ready for the next game",
};

let token = window.__NEBULA_TOKEN__ || "";

// Diagnostics: Pengu's datastore file is readable from outside the client.
function report(msg) {
  try { window.DataStore.set("nebula_probe", new Date().toISOString() + " " + msg); } catch (e) {}
}

async function api(path, init = {}) {
  const res = await fetch(API + path, {
    ...init,
    headers: { "X-Nebula-Token": token, "Content-Type": "application/json" },
  });
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error(data.error || "Nebula answered " + res.status);
  return data;
}

async function currentPick() {
  try {
    const res = await fetch("/lol-champ-select/v1/session");
    if (!res.ok) return 0;
    const session = await res.json();
    const me = (session.myTeam || []).find((p) => p.cellId === session.localPlayerCellId);
    return me ? me.championId || me.championPickIntent || 0 : 0;
  } catch (e) {
    return 0;
  }
}

const css = `
  :host { all: initial; }
  .panel { position: fixed; left: 50%; bottom: 96px; transform: translateX(-50%); z-index: 99999;
    width: min(760px, 70vw); color: #f0e6d2;
    font: 12px/1.3 "Spiegel", "Segoe UI", system-ui, sans-serif;
    background: linear-gradient(#0a1428ee, #010a13f2); border: 1px solid #785a28;
    box-shadow: 0 6px 24px #000a; user-select: none; }
  .head { display: flex; align-items: center; gap: 10px; padding: 6px 10px; cursor: move;
    border-bottom: 1px solid #463714; letter-spacing: .06em; text-transform: uppercase; }
  .brand { color: #c8aa6e; font-weight: 700; }
  .champ { color: #f0e6d2; }
  .status { margin-left: auto; color: #a09b8c; text-transform: none; letter-spacing: 0; }
  .status.error { color: #e84057; }
  button { all: unset; cursor: pointer; padding: 3px 9px; border: 1px solid #785a28; color: #c8aa6e;
    text-transform: uppercase; letter-spacing: .06em; font-size: 11px; }
  button:hover { background: #1e2328; color: #f0e6d2; }
  .body { padding: 8px 10px 10px; }
  .strip { display: flex; gap: 8px; overflow-x: auto; padding-bottom: 6px; }
  .strip::-webkit-scrollbar { height: 6px; }
  .strip::-webkit-scrollbar-thumb { background: #463714; }
  .card { flex: 0 0 auto; width: 92px; cursor: pointer; border: 1px solid #3c3c41; background: #010a13; }
  .card:hover { border-color: #c8aa6e; }
  .card.selected { border-color: #f0e6d2; box-shadow: 0 0 0 1px #c8aa6e; }
  .card.unavailable { opacity: .35; cursor: not-allowed; }
  .tile { height: 120px; background: #1e2328 center / cover no-repeat; }
  .name { padding: 4px 5px; height: 30px; overflow: hidden; color: #cdbe91; font-size: 11px; }
  .chromas { display: flex; gap: 6px; align-items: center; padding-top: 2px; min-height: 18px; color: #a09b8c; }
  .dot { width: 14px; height: 14px; border-radius: 50%; border: 1px solid #785a28; cursor: pointer; }
  .dot.selected { outline: 2px solid #f0e6d2; }
  .empty { padding: 14px; color: #a09b8c; }
`;

class Panel {
  constructor() {
    this.host = document.createElement("div");
    this.root = this.host.attachShadow({ mode: "open" });
    this.champion = 0;
    this.data = null;
    this.status = { text: "", error: false };
    this.collapsed = localStorage.getItem(COLLAPSED_KEY) === "1";
    this.cache = new Map();
    document.body.appendChild(this.host);
  }

  hide() {
    this.champion = 0;
    this.host.style.display = "none";
  }

  async show(champion) {
    if (champion === this.champion) return;
    this.champion = champion;
    this.host.style.display = "";
    this.data = this.cache.get(champion) || null;
    this.setStatus(this.data ? "" : "Loading skins...");
    this.render();
    try {
      const data = await api("/skins?champion=" + champion);
      this.cache.set(champion, data);
      if (this.champion !== champion) return;
      this.data = data;
      this.setStatus("");
    } catch (e) {
      if (this.champion === champion) this.setStatus(e.message || String(e), true);
    }
    this.render();
  }

  setStatus(text, error = false) {
    this.status = { text, error };
    if (error) report("error: " + text);
    const el = this.root.querySelector(".status");
    if (el) { el.textContent = text; el.classList.toggle("error", error); }
  }

  async choose(id) {
    if (!this.data) return;
    const data = this.data;
    const previous = data.selected;
    data.selected = id;
    this.setStatus("Preparing skin...");
    this.render();
    try {
      await api("/apply", { method: "POST", body: JSON.stringify({ skin: id }) });
      this.watchApply(id);
    } catch (e) {
      if (data.selected === id) data.selected = previous;
      this.render();
      this.setStatus(e.message || String(e), true);
    }
  }

  // Follows the patcher until the skin is ready, then leaves the final text up.
  async watchApply(id) {
    for (let i = 0; i < 120; i++) {
      await new Promise((r) => setTimeout(r, 1000));
      if (!this.data || this.data.selected !== id) return;
      try {
        const { patcher } = await api("/status");
        if (patcher && patcher.Error) return this.setStatus(patcher.Error, true);
        const text = PATCHER_TEXT[patcher];
        if (text) this.setStatus(text);
        if (patcher === "WaitingForGame" || patcher === "InGame" || patcher === "Patching") return;
      } catch (e) {
        return this.setStatus(e.message || String(e), true);
      }
    }
  }

  random() {
    const usable = (this.data?.skins || []).filter((s) => s.available);
    if (usable.length) this.choose(usable[Math.floor(Math.random() * usable.length)].id);
  }

  // Default placement is pure CSS; pixels are only stored once the user drags the panel.
  applySavedPosition(panel) {
    const saved = JSON.parse(localStorage.getItem(POS_KEY) || "null");
    if (saved) this.place(panel, saved.x, saved.y);
  }

  place(panel, x, y) {
    panel.style.left = Math.min(Math.max(0, x), innerWidth - 80) + "px";
    panel.style.top = Math.min(Math.max(0, y), innerHeight - 30) + "px";
    panel.style.bottom = "auto";
    panel.style.transform = "none";
  }

  enableDrag(head, panel) {
    head.addEventListener("mousedown", (down) => {
      if (down.target.closest("button")) return;
      const rect = panel.getBoundingClientRect();
      const dx = down.clientX - rect.left;
      const dy = down.clientY - rect.top;
      const move = (e) => this.place(panel, e.clientX - dx, e.clientY - dy);
      const up = () => {
        removeEventListener("mousemove", move);
        removeEventListener("mouseup", up);
        const r = panel.getBoundingClientRect();
        localStorage.setItem(POS_KEY, JSON.stringify({ x: r.left, y: r.top }));
      };
      addEventListener("mousemove", move);
      addEventListener("mouseup", up);
    });
  }

  render() {
    const old = this.root.querySelector(".panel");
    const keepStyle = old ? old.getAttribute("style") : null;
    const keepScroll = old?.querySelector(".strip")?.scrollLeft || 0;

    const panel = document.createElement("div");
    panel.className = "panel";
    const head = document.createElement("div");
    head.className = "head";
    head.innerHTML = '<span class="brand">Nebula</span><span class="champ"></span><span class="status"></span>';
    head.querySelector(".champ").textContent = this.data?.champion?.name || "";
    const status = head.querySelector(".status");
    status.textContent = this.status.text;
    status.classList.toggle("error", this.status.error);

    const randomBtn = document.createElement("button");
    randomBtn.textContent = "Random";
    randomBtn.onclick = () => this.random();
    const toggle = document.createElement("button");
    toggle.textContent = this.collapsed ? "Show" : "Hide";
    toggle.onclick = () => {
      this.collapsed = !this.collapsed;
      localStorage.setItem(COLLAPSED_KEY, this.collapsed ? "1" : "0");
      this.render();
    };
    head.append(randomBtn, toggle);
    panel.append(head);

    if (!this.collapsed) panel.append(this.renderBody());

    const style = document.createElement("style");
    style.textContent = css;
    this.root.replaceChildren(style, panel);

    if (keepStyle !== null) panel.setAttribute("style", keepStyle);
    else this.applySavedPosition(panel);
    const strip = panel.querySelector(".strip");
    if (strip) strip.scrollLeft = keepScroll;
    this.enableDrag(head, panel);
  }

  renderBody() {
    const body = document.createElement("div");
    body.className = "body";
    const skins = this.data?.skins || [];
    if (!skins.length) {
      body.innerHTML = '<div class="empty"></div>';
      body.firstChild.textContent = this.data ? "No skins found for this champion." : "";
      return body;
    }

    const strip = document.createElement("div");
    strip.className = "strip";
    const selectedId = this.data.selected;
    const champKey = this.data.champion.key;
    for (const skin of skins) {
      const isSelected = skin.id === selectedId || skin.chromas.some((c) => c.id === selectedId);
      const card = document.createElement("div");
      card.className = "card" + (isSelected ? " selected" : "") + (skin.available ? "" : " unavailable");
      card.title = skin.available ? skin.name : skin.name + " (not available yet)";
      const tile = document.createElement("div");
      tile.className = "tile";
      tile.style.backgroundImage = `url("/lol-game-data/assets/v1/champion-tiles/${champKey}/${skin.id}.jpg")`;
      const name = document.createElement("div");
      name.className = "name";
      name.textContent = skin.name;
      card.append(tile, name);
      if (skin.available) card.onclick = () => this.choose(skin.id);
      strip.append(card);
    }
    body.append(strip);

    const group = skins.find((s) => s.id === selectedId || s.chromas.some((c) => c.id === selectedId));
    if (group && group.chromas.length) {
      const row = document.createElement("div");
      row.className = "chromas";
      row.append("Chromas");
      for (const chroma of [{ ...group, colors: [] }, ...group.chromas]) {
        const dot = document.createElement("div");
        dot.className = "dot" + (chroma.id === selectedId ? " selected" : "");
        dot.title = chroma.name;
        dot.style.background = chroma.colors?.length
          ? `linear-gradient(135deg, ${chroma.colors[0]} 50%, ${chroma.colors[1] || chroma.colors[0]} 50%)`
          : "#785a28";
        if (chroma.available) dot.onclick = () => this.choose(chroma.id);
        else dot.style.opacity = ".35";
        row.append(dot);
      }
      body.append(row);
    }
    return body;
  }
}

async function start() {
  if (!token) {
    try {
      token = (await import("./token.js")).default;
    } catch (e) {
      return report("token.js missing; run scripts/pengu-install.sh");
    }
  }
  report("plugin started");

  const panel = new Panel();
  panel.hide();
  const tick = async () => {
    let champion = 0;
    try {
      champion = await currentPick();
      if (champion) await panel.show(champion);
      else panel.hide();
    } catch (e) {
      report("tick failed: " + e);
    }
    setTimeout(tick, champion ? 1000 : 2500);
  };
  tick();
}

export function init() {
  report("init");
}

export function load() {
  start().catch((e) => report("start failed: " + e));
}
