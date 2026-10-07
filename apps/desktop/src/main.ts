import "./style.css";

type DeckCounts = {
  deckId: string;
  name: string;
  newCount: number;
  learningCount: number;
  reviewCount: number;
};

type DueCard = {
  cardId: string;
  deckId: string;
  deckName: string;
  front: string;
  phonetic: string;
  back: string;
  example: string;
  sounds: string[];
  template?: string;
  stability: number;
  lapses: number;
  kind: string;
};

type GradeResult = {
  nextDue: string;
  stability: number;
  difficulty: number;
  reps: number;
  lapses: number;
};

type NoteDto = {
  id: string;
  deckId: string;
  deckName: string;
  front: string;
  back: string;
  fields: string[];
  tags: string[];
};

type Mode = "home" | "session" | "add" | "browse" | "edit" | "settings" | "stats";

type Session = {
  deckName: string;
  cards: DueCard[];
  index: number;
  revealed: boolean;
  graded: number;
  lastGrade?: GradeResult;
};

/** Dual transport: Tauri IPC when available, otherwise local HTTP API. */
const api = {
  async decks(): Promise<DeckCounts[]> {
    if (native()) {
      const { invoke } = await import("@tauri-apps/api/core");
      return invoke<DeckCounts[]>("list_decks");
    }
    const res = await apiFetch("/api/decks");
    if (!res.ok) throw new Error(await res.text());
    return res.json();
  },
  async due(deck: string, limit = 50): Promise<DueCard[]> {
    if (native()) {
      const { invoke } = await import("@tauri-apps/api/core");
      return invoke<DueCard[]>("due_cards", { deck, limit });
    }
    const qs = new URLSearchParams({ deck, limit: String(limit) });
    const res = await apiFetch(`/api/due?${qs}`);
    if (!res.ok) throw new Error(await res.text());
    return res.json();
  },
  async grade(cardId: string, rating: number): Promise<GradeResult | void> {
    if (native()) {
      const { invoke } = await import("@tauri-apps/api/core");
      return invoke<GradeResult>("grade_card", { cardId, rating });
    }
    const res = await apiFetch("/api/grade", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ cardId, rating }),
    });
    if (!res.ok) throw new Error(await res.text());
    return res.json();
  },
  async createNote(deck: string, front: string, back: string, tags: string[] = []): Promise<NoteDto> {
    if (native()) {
      const { invoke } = await import("@tauri-apps/api/core");
      return invoke<NoteDto>("create_note", { deck, front, back, tags });
    }
    const res = await apiFetch("/api/notes", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ deck, front, back, tags }),
    });
    if (!res.ok) throw new Error(await res.text());
    return res.json();
  },
  async updateNote(id: string, fields: string[], tags: string[] = []): Promise<NoteDto> {
    if (native()) {
      const { invoke } = await import("@tauri-apps/api/core");
      return invoke<NoteDto>("update_note", { id, fields, tags });
    }
    const res = await apiFetch(`/api/notes/${id}`, {
      method: "PUT",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ fields, tags }),
    });
    if (!res.ok) throw new Error(await res.text());
    return res.json();
  },
  async searchNotes(q: string, limit = 30): Promise<{ total: number; items: NoteDto[] }> {
    if (native()) {
      const { invoke } = await import("@tauri-apps/api/core");
      return invoke("search_notes", { q, limit });
    }
    const qs = new URLSearchParams({ q, limit: String(limit) });
    const res = await apiFetch(`/api/notes?${qs}`);
    if (!res.ok) throw new Error(await res.text());
    return res.json();
  },
  async getNote(id: string): Promise<NoteDto> {
    if (native()) {
      const { invoke } = await import("@tauri-apps/api/core");
      return invoke<NoteDto>("get_note", { id });
    }
    const res = await apiFetch(`/api/notes/${id}`);
    if (!res.ok) throw new Error(await res.text());
    return res.json();
  },
  async aiChat(messages: { role: string; content: string }[]): Promise<string> {
    const cfg = aiConfig();
    if (native()) {
      const { invoke } = await import("@tauri-apps/api/core");
      const r = await invoke<{ text: string }>("ai_chat", {
        baseUrl: cfg.baseUrl || "",
        apiKey: cfg.apiKey || "",
        model: cfg.model || "",
        messages,
      });
      return r.text;
    }
    const res = await apiFetch("/api/ai/chat", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        baseUrl: cfg.baseUrl || null,
        apiKey: cfg.apiKey || null,
        model: cfg.model || null,
        messages,
      }),
    });
    // 服务器可能返回 200 + 纯文本错误（旧版 axum 行为），文本优先、解析兜底
    const raw = await res.text();
    try {
      const j = JSON.parse(raw) as { text?: string };
      if (typeof j.text === "string" && j.text) return j.text;
    } catch {
      /* not JSON → plain-text error detail below */
    }
    throw new Error(raw || `HTTP ${res.status}`);
  },
};

/** Saved AnkiWeb login session (hkey, not the password). */
function ankiwebSession(): { email?: string; hkey?: string } {
  try {
    return JSON.parse(localStorage.getItem("anka.aw") || "{}");
  } catch {
    return {};
  }
}

function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** Remote self-hosted server (anka-server) config; empty base = local / same-origin. */
function remoteConfig(): { base: string; token: string } {
  return {
    base: (localStorage.getItem("anka.server") || "").replace(/\/+$/, ""),
    token: localStorage.getItem("anka.token") || "",
  };
}

/** Prefer a configured remote server over the local collection (mobile thin-client mode). */
function native(): boolean {
  return isTauri() && !useRemote();
}

/** OpenAI-compatible LLM endpoint config (long-press a card to ask AI). */
function aiConfig(): { baseUrl: string; apiKey: string; model: string } {
  try {
    return JSON.parse(localStorage.getItem("anka.ai") || "{}") as {
      baseUrl: string;
      apiKey: string;
      model: string;
    };
  } catch {
    return { baseUrl: "", apiKey: "", model: "" };
  }
}

function aiConfigured(): boolean {
  const c = aiConfig();
  return Boolean(c.baseUrl && c.model);
}

function useRemote(): boolean {
  return remoteConfig().base !== "";
}

async function apiFetch(path: string, init?: RequestInit): Promise<Response> {
  const { base, token } = remoteConfig();
  const headers = new Headers(init?.headers);
  if (token) headers.set("authorization", `Bearer ${token}`);
  const res = await fetch(base + path, { ...init, headers });
  if (res.status === 401) {
    throw new Error("Token 缺失或错误：点右上角 ⚙ 重新填写连接信息");
  }
  return res;
}

const app = document.querySelector<HTMLDivElement>("#app")!;

let decks: DeckCounts[] = [];
let session: Session | null = null;
let loading = false;
let loadingMsg = "正在读取收藏…";
let renderedMode: Mode | null = null;
let syncPct: number | null = null;

if (isTauri()) {
  void (async () => {
    const { listen } = await import("@tauri-apps/api/event");
    await listen<{ pct: number; msg: string }>("sync-progress", (e) => {
      syncPct = e.payload.pct;
      loadingMsg = e.payload.msg;
      if (loading) render();
    });
    // 应用内 APK 更新：下载/安装都在 Rust 后台线程，结果经事件回传
    await listen<{ stage: string; msg: string }>("apk-install", (e) => {
      if (e.payload.stage === "error") {
        error = e.payload.msg;
      } else {
        flash = e.payload.msg;
      }
      render();
    });
  })();
}
let error: string | null = null;
let selected = 0;
let mode: Mode = "home";
let browseQuery = "";
let browseTotal = 0;
let statsData: { days: number; stats: { date: string; reviews: number; due: number }[] } | null = null;
let browseItems: NoteDto[] = [];
let editNote: NoteDto | null = null;
let flash: string | null = null;

// ---- AI 问卡（长按卡片唤出） ----
type AiTurn = { q: string; a: string };
type AiCardCtx = {
  deckName: string;
  front: string;
  back: string;
  example?: string;
  /** 长按选中的关键词；缺省 = 整卡提问 */
  keyword?: string;
};
type AiSeedCard = { front: string; back: string; tags: string[]; deck?: string };
let aiOpen = false;
let aiCtx: AiCardCtx | null = null;
let aiTurns: AiTurn[] = [];
let aiBusy = false;
let aiError: string | null = null;
let pendingAICard: AiSeedCard | null = null;

/** 预设问题随关键词变化 */
function aiPresets(kw?: string): Array<[string, string]> {
  const k = kw ? `「${kw}」` : "这张卡片的内容";
  const it = kw ? `「${kw}」` : "它";
  return [
    ["详解", `请详细解释${k}：含义、要点和常见用法。`],
    ["例句", `请给出${it}的 5 个实用例句（附中文翻译）。`],
    ["记忆", `请给出${k}的记忆技巧/联想方法/词源。`],
    ["辨析", `请辨析${it}的近义词/易混淆点，并给出对比。`],
  ];
}

function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className?: string,
  text?: string,
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text != null) node.textContent = text;
  return node;
}

function render() {
  // Android 返回手势/返回键 => 应用内后退（而不是退出 App）
  if (renderedMode !== mode) {
    if (renderedMode !== null) history.pushState({ mode }, "", location.href);
    renderedMode = mode;
  }
  app.innerHTML = "";
  const shell = el("div", "shell");
  shell.appendChild(renderTop());

  const stage = el("div", "stage");
  if (error) {
    stage.appendChild(renderError(error));
  } else if (loading) {
    stage.appendChild(renderLoading());
  } else if (mode === "session" && session) {
    stage.appendChild(renderSession(session));
  } else if (mode === "add") {
    stage.appendChild(renderAddForm());
  } else if (mode === "browse") {
    stage.appendChild(renderBrowse());
  } else if (mode === "edit" && editNote) {
    stage.appendChild(renderEditForm(editNote));
  } else if (mode === "settings") {
    stage.appendChild(renderSettings());
  } else if (mode === "stats") {
    stage.appendChild(renderStats());
  } else {
    stage.appendChild(renderDecks(decks));
  }
  shell.appendChild(stage);
  if (flash) {
    const f = el("div", "flash", flash);
    shell.appendChild(f);
    setTimeout(() => {
      flash = null;
      render();
    }, 1800);
  }
  shell.appendChild(renderKeys());
  if (aiOpen && aiCtx) shell.appendChild(renderAiSheet());
  app.appendChild(shell);
}

function renderTop() {
  const top = el("div", "top");
  top.appendChild(el("div", "brand", "Anka"));
  const label =
    mode === "add"
      ? "新建卡片"
      : mode === "browse"
        ? "浏览笔记"
        : mode === "edit"
          ? "编辑笔记"
          : mode === "settings"
            ? "连接服务器"
            : session
              ? session.deckName
              : useRemote()
                ? `远程 · ${remoteConfig().base.replace(/^https?:\/\//, "")}`
                : "本地收藏 · FSRS";
  top.appendChild(el("div", "path", label));

  const chip = el("div", "chip");
  if (session && mode === "session") {
    chip.innerHTML = `<strong>${session.graded}</strong> / ${session.cards.length}`;
  } else {
    const due = decks.reduce(
      (n, d) => n + d.newCount + d.reviewCount + d.learningCount,
      0,
    );
    chip.innerHTML = due > 0 ? `待复习 <strong>${due}</strong>` : "全部完成";
  }
  top.appendChild(chip);

  // Nav buttons
  const nav = el("div", "top-nav");
  if (mode === "home") {
    const addBtn = el("button", "nav-btn", "+ 新建");
    addBtn.onclick = () => {
      mode = "add";
      render();
    };
    const browseBtn = el("button", "nav-btn", "全部卡组");
    browseBtn.onclick = () => {
      mode = "browse";
      void loadBrowse();
    };
    const statsBtn = el("button", "nav-btn", "统计");
    statsBtn.onclick = () => {
      mode = "stats";
      void loadStats();
    };
    const settingsBtn = el("button", "nav-btn", "⚙");
    settingsBtn.title = "连接服务器";
    settingsBtn.onclick = () => {
      mode = "settings";
      render();
    };
    nav.append(addBtn, browseBtn, statsBtn, settingsBtn);
  } else {
    const back = el("button", "nav-btn", "← 返回");
    back.onclick = () => {
      if (mode === "edit") {
        mode = "browse";
      } else if (mode === "session") {
        session = null;
        mode = "home";
        void refreshDecks();
      } else {
        mode = "home";
      }
      render();
    };
    nav.appendChild(back);
  }
  top.appendChild(nav);
  return top;
}

function renderSettings() {
  const panel = el("div", "form-panel");
  panel.appendChild(el("h1", undefined, "设置"));

  // ---------- ① AnkiWeb 账号（主路径，两种模式都可用） ----------
  panel.appendChild(el("h2", undefined, "① AnkiWeb 账号"));
  const aw = (() => {
    try {
      return JSON.parse(localStorage.getItem("anka.aw") || "{}") as {
        email?: string;
        hkey?: string;
      };
    } catch {
      return {};
    }
  })();
  const card = el("div", "subcard");
  if (aw.hkey) {
    card.appendChild(
      el(
        "p",
        "settings-hint",
        `已登录：${aw.email || "AnkiWeb"}（已保存会话凭证，密码不保留）`,
      ),
    );
    const a1 = el("div", "form-actions");
    const syncBtn = el("button", "reveal", "双向同步");
    syncBtn.onclick = async () => {
      loadingMsg = "正在与 AnkiWeb 同步…";
      loading = true;
      error = null;
      render();
      try {
        let r: {
          notesCreated?: number;
          notesUpdated?: number;
          cards?: number;
          schedUpdated?: number;
        };
        if (native()) {
          const { invoke } = await import("@tauri-apps/api/core");
          r = await invoke("ankiweb_sync", { hkey: aw.hkey });
        } else {
          const res = await apiFetch("/api/ankiweb/sync", {
            method: "POST",
            headers: { "content-type": "application/json" },
            body: JSON.stringify({ hkey: aw.hkey }),
          });
          const text = await res.text();
          try {
            r = JSON.parse(text);
          } catch {
            throw new Error(text || `HTTP ${res.status}`);
          }
        }
        flash = `同步完成：新增 ${r.notesCreated ?? 0} · 更新 ${r.notesUpdated ?? 0} · 卡片 ${r.cards ?? 0} · 调度 ${r.schedUpdated ?? 0}`;
        syncPct = null;
        loadingMsg = "正在读取收藏…";
        loading = false;
        await refreshDecks();
      } catch (e) {
        loading = false;
        error = e instanceof Error ? e.message : String(e);
        render();
      }
    };
    const out = el("button", "nav-btn", "退出登录");
    out.onclick = () => {
      localStorage.removeItem("anka.aw");
      render();
    };
    a1.append(syncBtn, out);
    card.appendChild(a1);
  } else {
    card.appendChild(
      el(
        "p",
        "settings-hint",
        native()
          ? "登录后全量导入 AnkiWeb 收藏到本机。密码仅登录用一次，之后凭会话同步，不保存。"
          : "登录后全量拉取 AnkiWeb 收藏到当前库。密码仅登录用一次，不保存。",
      ),
    );
    const em = fieldInput("AnkiWeb 邮箱", "");
    em.input.placeholder = "you@example.com";
    const pw = fieldInput("AnkiWeb 密码", "");
    (pw.input as HTMLInputElement).type = "password";
    card.append(em.wrap, pw.wrap);
    const a2 = el("div", "form-actions");
    const loginBtn = el("button", "reveal", "登录 AnkiWeb");
    loginBtn.onclick = async () => {
      const user = em.input.value.trim();
      const pass = pw.input.value;
      if (!user || !pass) {
        error = "请填写邮箱和密码";
        render();
        return;
      }
      loadingMsg = "正在登录 AnkiWeb…";
      loading = true;
      error = null;
      render();
      try {
        let hkey: string;
        if (native()) {
          const { invoke } = await import("@tauri-apps/api/core");
          const loginRes = await invoke<{ hkey: string }>("ankiweb_login", {
            user,
            password: pass,
          });
          hkey = loginRes.hkey;
          localStorage.setItem("anka.aw", JSON.stringify({ email: user, hkey }));
          loadingMsg = "正在从 AnkiWeb 下载收藏（首次约需 1-2 分钟）…";
          render();
          await invoke("ankiweb_import", { user, password: pass });
        } else {
          const res = await apiFetch("/api/ankiweb/login", {
            method: "POST",
            headers: { "content-type": "application/json" },
            body: JSON.stringify({ user, password: pass }),
          });
          const text = await res.text();
          let j: { hkey?: string };
          try {
            j = JSON.parse(text);
          } catch {
            throw new Error(text || `HTTP ${res.status}`);
          }
          if (!j.hkey) throw new Error(text);
          hkey = j.hkey;
          localStorage.setItem("anka.aw", JSON.stringify({ email: user, hkey }));
          loadingMsg = "正在与 AnkiWeb 双向同步（首次约需 1-2 分钟）…";
          render();
          const r2 = await apiFetch("/api/ankiweb/sync", {
            method: "POST",
            headers: { "content-type": "application/json" },
            body: JSON.stringify({ hkey }),
          });
          const t2 = await r2.text();
          try {
            JSON.parse(t2);
          } catch {
            throw new Error(t2 || `HTTP ${r2.status}`);
          }
        }
        flash = "AnkiWeb 已登录并完成同步";
        syncPct = null;
        syncPct = null;
        loadingMsg = "正在读取收藏…";
        loading = false;
        await refreshDecks();
      } catch (e) {
        loading = false;
        error = e instanceof Error ? e.message : String(e);
        render();
      }
    };
    a2.appendChild(loginBtn);
    card.appendChild(a2);
  }
  panel.appendChild(card);

  // ---------- ② 高级：连接自建服务器（Tauri 端显示；网页端自身就在服务器上） ----------
  if (isTauri()) {
    panel.appendChild(el("h2", undefined, "② 高级：连接自建服务器"));
    panel.appendChild(
      el(
        "p",
        "settings-hint",
        "填自托管 anka-server 地址 → 手机/电脑/Agent 共用同一个库；留空 → 使用本机收藏。",
      ),
    );
    const cfg = remoteConfig();
    const serverField = fieldInput("服务器地址", cfg.base);
    serverField.input.placeholder = "http://192.168.6.100:8788";
    const tokenField = fieldInput(
      "访问令牌（服务器的 Token，不是 AnkiWeb 密码）",
      cfg.token,
    );
    panel.append(serverField.wrap, tokenField.wrap);

    const actions = el("div", "form-actions");
    const save = el("button", "reveal", "保存并连接");
    save.onclick = () => {
      const url = serverField.input.value.trim().replace(/\/+$/, "");
      localStorage.setItem("anka.server", url);
      localStorage.setItem("anka.token", tokenField.input.value.trim());
      session = null;
      mode = "home";
      void refreshDecks();
    };
    const clear = el("button", "nav-btn", "断开（用本机收藏）");
    clear.onclick = () => {
      localStorage.removeItem("anka.server");
      localStorage.removeItem("anka.token");
      session = null;
      mode = "home";
      void refreshDecks();
    };
    actions.append(save, clear);
    panel.appendChild(actions);
  }

  // ---------- ③ AI 大模型（长按卡片提问） ----------
  panel.appendChild(el("h2", undefined, "③ AI 大模型（长按卡片提问）"));
  panel.appendChild(
    el(
      "p",
      "settings-hint",
      "填 OpenAI 兼容接口后，复习时长按卡片即可向 AI 提问，还能把回答提炼成新卡片。常用：DeepSeek https://api.deepseek.com · 通义 https://dashscope.aliyuncs.com/compatible-mode/v1 · Kimi https://api.moonshot.cn/v1 · OpenAI https://api.openai.com/v1 · 本地 Ollama http://localhost:11434/v1",
    ),
  );
  const savedAi = aiConfig();
  const aiBase = fieldInput("接口地址 Base URL", savedAi.baseUrl || "");
  aiBase.input.placeholder = "https://api.deepseek.com";
  const aiKey = fieldInput("API Key", savedAi.apiKey || "");
  (aiKey.input as HTMLInputElement).type = "password";
  const aiModel = fieldInput("模型名 Model", savedAi.model || "");
  aiModel.input.placeholder = "deepseek-chat";
  panel.append(aiBase.wrap, aiKey.wrap, aiModel.wrap);
  const aiActions = el("div", "form-actions");
  const aiClear = el("button", "nav-btn", "清除");
  aiClear.onclick = () => {
    localStorage.removeItem("anka.ai");
    flash = "AI 配置已清除";
    render();
  };
  const aiSave = el("button", "reveal", "保存 AI 配置");
  aiSave.onclick = () => {
    localStorage.setItem(
      "anka.ai",
      JSON.stringify({
        baseUrl: aiBase.input.value.trim().replace(/\/+$/, ""),
        apiKey: aiKey.input.value.trim(),
        model: aiModel.input.value.trim(),
      }),
    );
    flash = "AI 配置已保存";
    render();
  };
  aiActions.append(aiClear, aiSave);
  panel.appendChild(aiActions);

  // ---------- 检查更新 ----------
  const upd = el("div", "subcard");
  upd.appendChild(el("h2", undefined, "检查更新"));
  const updActions = el("div", "form-actions");
  const updBtn = el("button", "reveal", "检查并更新");
  updBtn.onclick = async () => {
    loading = true;
    error = null;
    render();
    try {
      flash = await checkForUpdate();
      loading = false;
      render();
    } catch (e) {
      loading = false;
      error = e instanceof Error ? e.message : String(e);
      render();
    }
  };
  updActions.appendChild(updBtn);
  upd.appendChild(updActions);
  panel.appendChild(upd);

  const back = el("button", "nav-btn", "← 返回");
  back.onclick = () => {
    mode = "home";
    render();
  };
  panel.appendChild(back);
    const verLine = el("p", "version-line", "Anka");
  if (isTauri()) {
    void (async () => {
      try {
        const { getVersion } = await import("@tauri-apps/api/app");
        verLine.textContent = `Anka v${await getVersion()}`;
      } catch {
        /* ignore */
      }
    })();
  } else {
    verLine.textContent = "Anka Web";
  }
  panel.appendChild(verLine);
return panel;
}


async function loadStats() {
  loadingMsg = "正在读取学习统计…";
  loading = true;
  renderedMode = mode;
  render();
  try {
    const res = await apiFetch("/api/stats/daily?days=120");
    if (!res.ok) throw new Error(await res.text());
    statsData = await res.json();
    error = null;
  } catch (e) {
    error = e instanceof Error ? e.message : String(e);
  }
  loading = false;
  renderedMode = null;
  render();
}

function svgEl(tag: string, attrs: Record<string, string | number>): SVGElement {
  const node = document.createElementNS("http://www.w3.org/2000/svg", tag);
  for (const [k, v] of Object.entries(attrs)) node.setAttribute(k, String(v));
  return node;
}

function dateStr(d: Date): string {
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}

function renderStats() {
  const panel = el("div", "form-panel");
  panel.appendChild(el("h1", undefined, "学习统计"));
  if (!statsData) {
    panel.appendChild(el("p", "form-hint", "暂无数据"));
    return panel;
  }

  const byDate = new Map(statsData.stats.map((x) => [x.date, x]));
  const today = new Date();

  // ---- 近 30 天复习 + 未来 30 天到期（柱状图） ----
  const past: { date: string; reviews: number }[] = [];
  for (let i = 29; i >= 0; i--) {
    const d = new Date(today);
    d.setDate(d.getDate() - i);
    past.push({ date: dateStr(d), reviews: byDate.get(dateStr(d))?.reviews ?? 0 });
  }
  const future: { date: string; due: number }[] = [];
  for (let i = 0; i < 30; i++) {
    const d = new Date(today);
    d.setDate(d.getDate() + i);
    future.push({ date: dateStr(d), due: byDate.get(dateStr(d))?.due ?? 0 });
  }

  const maxPast = Math.max(1, ...past.map((x) => x.reviews));
  const maxDue = Math.max(1, ...future.map((x) => x.due));
  const W = 640;
  const H = 120;
  const bw = W / 30;

  const barSvg = svgEl("svg", { viewBox: `0 0 ${W} ${H + 16}`, width: "100%" });
  past.forEach((x, i) => {
    const h = (x.reviews / maxPast) * (H - 6);
    if (h > 0) barSvg.appendChild(svgEl("rect", { x: i * bw + 1, y: H - h, width: bw - 2, height: h, fill: "#ff8c42", rx: 1 }));
  });
  panel.appendChild(el("p", "form-hint", "近 30 天复习次数"));
  panel.appendChild(barSvg);

  const futSvg = svgEl("svg", { viewBox: `0 0 ${W} ${H + 16}`, width: "100%" });
  future.forEach((x, i) => {
    const h = (x.due / maxDue) * (H - 6);
    if (h > 0) futSvg.appendChild(svgEl("rect", { x: i * bw + 1, y: H - h, width: bw - 2, height: h, fill: "#7eb6ff", rx: 1 }));
  });
  panel.appendChild(el("p", "form-hint", "未来 30 天到期预测"));
  panel.appendChild(futSvg);

  // ---- 学习日历热力图（GitHub 风格） ----
  panel.appendChild(el("p", "form-hint", "学习日历（近 18 周）"));
  const calSvg = svgEl("svg", { viewBox: "0 0 140 130", width: "100%" });
  const cell = 16;
  const start = new Date(today);
  start.setDate(start.getDate() - 125);
  const totalReviews = statsData.stats.reduce((s2, x) => s2 + x.reviews, 0);
  for (let i = 0; i <= 125; i++) {
    const d = new Date(start);
    d.setDate(d.getDate() + i);
    const ds = dateStr(d);
    const n = byDate.get(ds)?.reviews ?? 0;
    const col = Math.floor(i / 7);
    const row = d.getDay();
    let fill = "#1d222a";
    if (n >= 10) fill = "#ffc15e";
    else if (n >= 6) fill = "#e4572e";
    else if (n >= 3) fill = "#ff8c42";
    else if (n >= 1) fill = "#8a4a2b";
    const rect = svgEl("rect", { x: col * (cell + 3), y: row * (cell + 3), width: cell, height: cell, rx: 3, fill });
    const t = document.createElementNS("http://www.w3.org/2000/svg", "title");
    t.textContent = `${ds}：${n} 次复习`;
    rect.appendChild(t);
    calSvg.appendChild(rect);
  }
  void totalReviews;
  panel.appendChild(calSvg);
  panel.appendChild(el("p", "form-hint", "颜色越亮 = 当天复习越多 · 悬停查看日期"));

  return panel;
}

/** Auto-sync to AnkiWeb after a review session finishes (if logged in). */
function maybeAutoSync() {
  const aw = ankiwebSession();
  if (!aw.hkey) return;
  void (async () => {
    try {
      if (native()) {
        const { invoke } = await import("@tauri-apps/api/core");
        const r = await invoke<{ notesCreated?: number; notesUpdated?: number }>(
          "ankiweb_sync",
          { hkey: aw.hkey },
        );
        flash = `已同步 AnkiWeb：新增 ${r.notesCreated ?? 0} · 更新 ${r.notesUpdated ?? 0}`;
      } else {
        const res = await apiFetch("/api/ankiweb/sync", {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ hkey: aw.hkey }),
        });
        const text = await res.text();
        let r: { notesCreated?: number; notesUpdated?: number };
        try {
          r = JSON.parse(text);
        } catch {
          throw new Error(text || `HTTP ${res.status}`);
        }
        flash = `已同步 AnkiWeb：新增 ${r.notesCreated ?? 0} · 更新 ${r.notesUpdated ?? 0}`;
      }
    } catch {
      /* background sync: stay silent on failure */
    }
    render();
  })();
}



/** Numeric semver-ish compare: returns >0 if a is newer. */
function cmpVersion(a: string, b: string): number {
  const pa = a.replace(/^v/, "").split(".").map(Number);
  const pb = b.replace(/^v/, "").split(".").map(Number);
  for (let i = 0; i < 3; i++) {
    const d = (pa[i] || 0) - (pb[i] || 0);
    if (d) return d;
  }
  return 0;
}

/** 服务器中转的版本信息（GET /api/app/latest）。 */
type AppLatestRelay = {
  tag: string;
  version: string;
  apkSize: number;
  apkPath: string;
};

/** App version check + update. Desktop: Tauri updater (download+install).
 *  Mobile: compare against GitHub latest release, open the download page. */
async function checkForUpdate(): Promise<string> {
  if (isTauri()) {
    const isMobile = /android|ios/i.test(navigator.userAgent);
    const { getVersion } = await import("@tauri-apps/api/app");
    const current = await getVersion();
    if (isMobile) {
      const current = await getVersion();
      // ① 配置了远程服务器：先问服务器（局域网快、还带缓存），手机连不上 GitHub 也能更新
      let relay: AppLatestRelay | null = null;
      if (useRemote()) {
        try {
          const res = await apiFetch("/api/app/latest");
          if (res.ok) {
            const j = (await res.json()) as AppLatestRelay;
            if (j?.version) relay = j;
          }
        } catch {
          /* 服务器不支持或不可达 → GitHub 直连 */
        }
      }
      // ② GitHub API（版本核对 + 直链兜底）
      let rel: {
        tag_name?: string;
        html_url?: string;
        assets?: { name: string; size: number; browser_download_url: string }[];
      } | null = null;
      try {
        const res = await fetch(
          "https://api.github.com/repos/meichuanyi/anka/releases/latest",
          { headers: { accept: "application/vnd.github+json" } },
        );
        if (res.ok) rel = await res.json();
      } catch {
        /* 手机连不上 GitHub：只剩服务器中转 */
      }
      if (!relay && !rel) throw new Error("无法获取版本信息（服务器与 GitHub 都不可达）");
      const latest = relay?.version || (rel?.tag_name || "").replace(/^v/, "");
      if (!latest || cmpVersion(latest, current) <= 0)
        return `当前已是最新版本（v${current}）`;

      // 精简签名包优先（anka-mobile-*），其次最小的 .apk
      const apks = (rel?.assets || []).filter((a) => a.name.endsWith(".apk"));
      const apkAsset =
        apks.find((a) => a.name.startsWith("anka-mobile-")) ??
        apks.sort((a, b) => a.size - b.size)[0];
      const relayUrl = relay ? remoteConfig().base + relay.apkPath : undefined;
      const expected = relay?.apkSize ?? apkAsset?.size ?? 0;
      if (relayUrl || apkAsset) {
        try {
          const { invoke } = await import("@tauri-apps/api/core");
          await invoke("install_apk", {
            url: apkAsset?.browser_download_url ?? relayUrl!,
            relayUrl,
            token: useRemote() ? remoteConfig().token : undefined,
            expectedSize: expected,
          });
          return `新版本 v${latest}（${(expected / 1024 / 1024).toFixed(0)}MB）开始下载${relayUrl ? "（服务器中转）" : ""}，完成后自动弹出安装界面`;
        } catch {
          /* fall through to browser */
        }
      }
      const { openUrl } = await import("@tauri-apps/plugin-opener");
      await openUrl(
        rel?.html_url || relayUrl || "https://github.com/meichuanyi/anka/releases/latest",
      );
      return `发现新版本 v${latest}，已打开下载页（下载 APK 后安装覆盖即可）`;
    }
    const { check } = await import("@tauri-apps/plugin-updater");
    const update = await check();
    if (!update) return "当前已是最新版本";
    await update.downloadAndInstall();
    return `已更新到 ${update.version}，重启应用后生效`;
  }
  const res = await fetch(
    "https://api.github.com/repos/meichuanyi/anka/releases/latest",
    { headers: { accept: "application/vnd.github+json" } },
  );
  if (!res.ok) throw new Error(`GitHub API ${res.status}`);
  const rel = (await res.json()) as { tag_name?: string; html_url?: string };
  return `网页版始终使用服务器最新部署。桌面/手机最新版本：${rel.tag_name}（${rel.html_url}）`;
}

function renderKeys() {
  const keys = el("div", "footer-keys");
  if (mode === "session" && session && !session.revealed) {
    keys.innerHTML = `<span class="kbd">Space</span> 显示答案 · <span class="kbd">Esc</span> 返回牌组`;
  } else if (mode === "session" && session && session.revealed) {
    keys.innerHTML = `<span class="kbd">1</span> Again · <span class="kbd">2</span> Hard · <span class="kbd">3</span> Good · <span class="kbd">4</span> Easy`;
  } else if (mode === "add" || mode === "edit") {
    keys.innerHTML = `Ctrl/⌘+Enter 保存`;
  } else if (mode === "browse") {
    keys.innerHTML = `搜索后点击「编辑」`;
  } else if (mode === "stats") {
    keys.innerHTML = `学习日历与进度曲线`;
  } else {
    keys.innerHTML = `<span class="kbd">↑↓</span> 选择 · <span class="kbd">Enter</span> 开始复习`;
  }
  return keys;
}

function renderLoading() {
  const wrap = el("div", "empty");
  wrap.appendChild(el("h2", undefined, "加载中"));
  wrap.appendChild(el("p", undefined, loadingMsg));
  if (syncPct !== null) {
    const track = el("div", "progress-track");
    const bar = el("div", "progress-bar");
    bar.style.width = `${Math.min(100, Math.max(0, syncPct))}%`;
    track.appendChild(bar);
    wrap.appendChild(track);
    wrap.appendChild(el("p", undefined, `${syncPct}%`));
  }
  return wrap;
}

function renderError(message: string) {
  const wrap = el("div", "error");
  wrap.appendChild(el("h2", undefined, "出错了"));
  wrap.appendChild(el("p", undefined, message));
  const actions = el("div", "form-actions");
  const retry = el("button", "reveal", "重试");
  retry.addEventListener("click", () => {
    error = null;
    void boot();
  });
  actions.appendChild(retry);
  const settings = el("button", "reveal", "⚙ 连接设置");
  settings.addEventListener("click", () => {
    error = null;
    mode = "settings";
    render();
  });
  actions.appendChild(settings);
  wrap.appendChild(actions);
  return wrap;
}

function studyableDecks(list: DeckCounts[]) {
  return list
    .filter((d) => d.newCount + d.reviewCount + d.learningCount > 0)
    .sort((a, b) => a.name.localeCompare(b.name, "zh"));
}

function renderDecks(list: DeckCounts[]) {
  if (!list.length) {
    const wrap = el("div", "empty");
    wrap.appendChild(el("h2", undefined, "还没有牌组"));
    wrap.appendChild(
      el(
        "p",
        undefined,
        "用 CLI 导入 .apkg，或设置 ANKA_COLLECTION 指向已有收藏。",
      ),
    );
    return wrap;
  }

  const panel = el("div", "deck-panel");
  panel.appendChild(el("h1", undefined, "牌组"));
  const studyable = studyableDecks(list);

  if (!studyable.length) {
    const wrap = el("div", "empty");
    wrap.appendChild(el("h2", undefined, "今天没有到期卡片"));
    wrap.appendChild(el("p", undefined, "休息一下，或导入新的牌组。"));
    return wrap;
  }

  if (selected >= studyable.length) selected = 0;

  studyable.forEach((d, i) => {
    const row = el("button", i === selected ? "deck-row active" : "deck-row");
    row.type = "button";
    row.appendChild(el("div", "name", d.name));
    const counts = el("div", "counts");
    counts.innerHTML = `<span>新 <b>${d.newCount}</b></span><span>学 <b>${d.learningCount}</b></span><span>到 <b>${d.reviewCount}</b></span>`;
    row.append(counts);
    row.addEventListener("click", () => {
      selected = i;
      void startSession(d.name);
    });
    panel.appendChild(row);
  });
  return panel;
}

function mediaUrl(raw: string): string {
  // HTTP mode: sounds are bare filenames → /media/<name>
  // Tauri mode: absolute path → convertFileSrc
  if (!raw) return raw;
  if (raw.includes("/") || raw.includes("\\")) {
    if (isTauri()) {
      // lazy import would be async; use asset protocol via convertFileSrc if available
      const w = window as unknown as {
        __TAURI_INTERNALS__?: { convertFileSrc?: (p: string) => string };
      };
      const conv = w.__TAURI_INTERNALS__?.convertFileSrc;
      if (typeof conv === "function") return conv(raw);
      return raw;
    }
    const parts = raw.split(/[\\/]/);
    return `${remoteConfig().base}/media/${encodeURIComponent(parts[parts.length - 1] ?? raw)}`;
  }
  return `${remoteConfig().base}/media/${encodeURIComponent(raw)}`;
}

let audioEl: HTMLAudioElement | null = null;

function playSounds(sounds: string[] | undefined) {
  if (audioEl) {
    audioEl.pause();
    audioEl = null;
  }
  if (!sounds?.length) return;
  const url = mediaUrl(sounds[0]!);
  audioEl = new Audio(url);
  audioEl.volume = 1;
  audioEl.play().catch(() => {
    /* autoplay may be blocked until user gesture */
  });
}

function stopSounds() {
  if (audioEl) {
    audioEl.pause();
    audioEl = null;
  }
}

function renderSoundButton(card: DueCard) {
  if (!card.sounds?.length) return null;
  const btn = el("button", "sound-btn");
  btn.type = "button";
  btn.innerHTML = `🔊 发音`;
  btn.addEventListener("click", (e) => {
    e.stopPropagation();
    playSounds(card.sounds);
  });
  return btn;
}

function renderSession(s: Session) {
  const host = el("div");
  host.style.display = "contents";

  const bar = el("div", "progress-bar");
  const fill = document.createElement("i");
  const total = Math.max(s.cards.length, 1);
  fill.style.width = `${Math.round((s.graded / total) * 100)}%`;
  bar.appendChild(fill);
  host.appendChild(bar);

  const current = s.cards[s.index];
  if (!current) {
    const wrap = el("div", "empty");
    wrap.appendChild(el("h2", undefined, "这一轮完成了"));
    wrap.appendChild(el("p", undefined, `共复习 ${s.graded} 张。`));
    const btn = el("button", "reveal", "返回牌组");
    btn.addEventListener("click", () => {
      session = null;
      void refreshDecks();
    });
    wrap.appendChild(btn);
    host.appendChild(wrap);
    return host;
  }

  const card = el("div", "card");
  card.appendChild(el("div", "deck-tag", current.deckName));
  const headRow = el("div", "head-row");
  headRow.appendChild(el("div", "head", current.front));
  const soundBtn = renderSoundButton(current);
  if (soundBtn) headRow.appendChild(soundBtn);
  card.appendChild(headRow);
  if (current.phonetic) {
    card.appendChild(el("div", "phonetic", current.phonetic));
  }
  const tpl = current.template || "recite";
  const tip =
    tpl === "spelling"
      ? "释义 → 拼写单词"
      : tpl === "dictation"
        ? "听音 → 写出单词"
        : "单词 → 回忆释义";
  card.appendChild(el("div", "prompt", `${tip} · 长按词语问 AI`));
  onLongPress(card, (x, y) =>
    openAiSheet({
      deckName: current.deckName,
      front: current.front,
      back: current.back,
      example: current.example || undefined,
      keyword: keywordAtPointIn(card, x, y),
    }),
  );

  if (s.revealed) {
    const answer = el("div", "answer md");
    answer.innerHTML = renderMarkdown(current.back || "（无释义）");
    if (current.example) {
      const ex = el("div", "example md");
      ex.innerHTML = renderMarkdown(current.example);
      answer.appendChild(ex);
    }
    const meta = el("div", "meta-row");
    const next = s.lastGrade
      ? `→ ${new Date(s.lastGrade.nextDue).toLocaleString("zh-CN", {
          month: "numeric",
          day: "numeric",
          hour: "2-digit",
          minute: "2-digit",
        })}`
      : "";
    meta.innerHTML = `<span>${current.kind}</span><span>S ${current.stability.toFixed(2)}</span><span>lapses ${current.lapses}</span>${next ? `<span class="next">${next}</span>` : ""}`;
    answer.appendChild(meta);
    card.appendChild(answer);
  }
  host.appendChild(card);

  const actions = el("div", s.revealed ? "actions" : "actions single");
  if (!s.revealed) {
    const btn = el("button", "reveal", "显示答案");
    btn.addEventListener("click", () => {
      s.revealed = true;
      render();
    });
    actions.appendChild(btn);
  } else {
    const grades: Array<[string, string, number, string]> = [
      ["again", "Again", 1, "重来"],
      ["hard", "Hard", 2, "困难"],
      ["good", "Good", 3, "良好"],
      ["easy", "Easy", 4, "简单"],
    ];
    for (const [cls, label, rating, zh] of grades) {
      const btn = el("button", `grade ${cls}`);
      btn.innerHTML = `<span>${label}</span><small>${zh} · ${rating}</small>`;
      btn.addEventListener("click", () => void grade(rating));
      actions.appendChild(btn);
    }
  }
  host.appendChild(actions);
  return host;
}

/** Fire fn after a 500ms touch long-press; desktop right-click also works.
 *  The press coordinates are passed so callers can resolve the tapped word. */
function onLongPress(node: HTMLElement, fn: (x: number, y: number) => void) {
  let timer: number | null = null;
  let sx = 0;
  let sy = 0;
  const cancel = () => {
    if (timer != null) {
      window.clearTimeout(timer);
      timer = null;
    }
  };
  node.addEventListener(
    "touchstart",
    (e) => {
      if (e.touches.length !== 1) return cancel();
      const t = e.touches[0]!;
      sx = t.clientX;
      sy = t.clientY;
      timer = window.setTimeout(() => {
        timer = null;
        fn(sx, sy);
      }, 500);
    },
    { passive: true },
  );
  node.addEventListener(
    "touchmove",
    (e) => {
      const t = e.touches[0];
      if (t && Math.hypot(t.clientX - sx, t.clientY - sy) > 12) cancel();
    },
    { passive: true },
  );
  node.addEventListener("touchend", cancel);
  node.addEventListener("touchcancel", cancel);
  node.addEventListener("contextmenu", (e) => {
    e.preventDefault();
    fn(e.clientX, e.clientY);
  });
}

/** The word under the long-press point, used as the AI keyword.
 *  Scans every text node under `root` and tests each word's on-screen rect
 *  against (x, y) — unlike caretRangeFromPoint this ignores user-select,
 *  which otherwise makes hit-testing miss the whole card on WebViews.
 *  Latin runs come out whole; long CJK runs collapse to a short window
 *  around the finger. Undefined when the press missed any text. */
function keywordAtPointIn(
  root: HTMLElement,
  x: number,
  y: number,
): string | undefined {
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  const re = /[\p{L}\p{N}'’_-]+/gu;
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    const text = node.textContent || "";
    re.lastIndex = 0;
    for (let m = re.exec(text); m; m = re.exec(text)) {
      const range = document.createRange();
      range.setStart(node, m.index);
      range.setEnd(node, m.index + m[0].length);
      const r = range.getBoundingClientRect();
      range.detach?.();
      if (x < r.left || x > r.right || y < r.top || y > r.bottom) continue;
      let word = m[0];
      if (/^[\p{Script=Han}]+$/u.test(word) && word.length > 6) {
        // 中文无词边界：按横向位置取按压点附近的 5 字窗口
        const ratio = (x - r.left) / Math.max(r.width, 1);
        const c =
          m.index +
          Math.min(
            Math.max(Math.floor(ratio * m[0].length), 2),
            m[0].length - 3,
          );
        word = text.slice(c - 2, c + 3);
      }
      word = word.trim().replace(/^[-'’_]+|[-'’_]+$/g, "");
      return word || undefined;
    }
  }
  return undefined;
}

/** Minimal, dependency-free markdown → HTML for LLM answers.
 *  All HTML is escaped first, so the output is safe to inject. */
function renderMarkdown(src: string): string {
  const esc = (s: string) =>
    s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  const inline = (s: string) =>
    esc(s)
      .replace(/`([^`]+)`/g, "<code>$1</code>")
      .replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>")
      .replace(/(^|[^*])\*([^*\n]+)\*/g, "$1<em>$2</em>")
      .replace(
        /\[([^\]]+)\]\((https?:\/\/[^)\s]+)\)/g,
        '<a href="$2" target="_blank" rel="noopener">$1</a>',
      );

  // fenced code blocks become placeholders so their content is not transformed
  const codeBlocks: string[] = [];
  const work = src
    .replace(/\r\n/g, "\n")
    .replace(/```[a-zA-Z0-9+-]*\n?([\s\S]*?)```/g, (_m, code: string) => {
      codeBlocks.push(
        `<pre><code>${esc(code.replace(/\n$/, ""))}</code></pre>`,
      );
      return `\u0000${codeBlocks.length - 1}\u0000`;
    });

  const lines = work.split("\n");
  const out: string[] = [];
  let list: "ul" | "ol" | null = null;
  let para: string[] = [];
  const closeList = () => {
    if (list) {
      out.push(`</${list}>`);
      list = null;
    }
  };
  const flushPara = () => {
    if (para.length) {
      out.push(`<p>${para.map(inline).join("<br>")}</p>`);
      para = [];
    }
  };
  for (const raw of lines) {
    const line = raw.trim();
    if (!line) {
      flushPara();
      closeList();
      continue;
    }
    const ph = line.match(/^\u0000(\d+)\u0000$/);
    if (ph) {
      flushPara();
      closeList();
      out.push(codeBlocks[Number(ph[1])]!);
      continue;
    }
    const h = line.match(/^#{1,6}\s+(.*)$/);
    if (h) {
      flushPara();
      closeList();
      out.push(`<p><strong>${inline(h[1]!)}</strong></p>`);
      continue;
    }
    const ul = line.match(/^[-•]\s+(.*)$/);
    if (ul) {
      flushPara();
      if (list !== "ul") {
        closeList();
        out.push("<ul>");
        list = "ul";
      }
      out.push(`<li>${inline(ul[1]!)}</li>`);
      continue;
    }
    const ol = line.match(/^\d+[.、)]\s+(.*)$/);
    if (ol) {
      flushPara();
      if (list !== "ol") {
        closeList();
        out.push("<ol>");
        list = "ol";
      }
      out.push(`<li>${inline(ol[1]!)}</li>`);
      continue;
    }
    const bq = line.match(/^>\s?(.*)$/);
    if (bq) {
      flushPara();
      closeList();
      out.push(`<blockquote>${inline(bq[1]!)}</blockquote>`);
      continue;
    }
    closeList();
    para.push(line);
  }
  flushPara();
  closeList();
  return out.join("");
}

/** 每张卡（+关键词）一份对话历史，存 localStorage，重开面板自动恢复 */
function aiConvKey(ctx: AiCardCtx): string {
  return `${ctx.deckName}\u0000${ctx.front}\u0000${ctx.keyword ?? ""}`;
}

function aiHistoryLoad(key: string): { keyword?: string; turns: AiTurn[]; ts: number } | undefined {
  try {
    const all = JSON.parse(localStorage.getItem("anka.aiHistory") || "{}") as Record<
      string,
      { keyword?: string; turns: AiTurn[]; ts: number }
    >;
    return all[key];
  } catch {
    return undefined;
  }
}

function aiHistorySave(
  key: string,
  conv: { keyword?: string; turns: AiTurn[]; ts: number },
) {
  try {
    const all = JSON.parse(localStorage.getItem("anka.aiHistory") || "{}") as Record<
      string,
      { keyword?: string; turns: AiTurn[]; ts: number }
    >;
    all[key] = conv;
    // 只留最近 50 份对话，防止无限膨胀
    const kept = Object.entries(all)
      .sort((a, b) => (b[1].ts ?? 0) - (a[1].ts ?? 0))
      .slice(0, 50);
    localStorage.setItem("anka.aiHistory", JSON.stringify(Object.fromEntries(kept)));
  } catch {
    /* 存储不可用/已满：历史是附属品，静默放弃 */
  }
}

function openAiSheet(ctx: AiCardCtx) {
  aiCtx = ctx;
  aiTurns = aiHistoryLoad(aiConvKey(ctx))?.turns ?? [];
  aiError = null;
  aiOpen = true;
  render();
}

function closeAiSheet() {
  aiOpen = false;
  aiCtx = null;
  render();
}

function aiSystemPrompt(): string {
  const c = aiCtx!;
  return [
    "你是 Anka 记忆卡片的学习助手。用户正在复习下面这张卡片：",
    `正面：${c.front}`,
    `背面：${c.back || "（无）"}`,
    c.example ? `例句：${c.example}` : "",
    c.keyword ? `用户长按选中了关键词：「${c.keyword}」，提问通常围绕它展开。` : "",
    "请围绕卡片内容（及选中的关键词）解答提问：中文为主，简洁准确，用 Markdown 格式（加粗、列表等），不要输出 HTML。",
  ]
    .filter(Boolean)
    .join("\n");
}

async function aiAsk(question: string) {
  if (!aiCtx || aiBusy) return;
  const q = question.trim();
  if (!q) return;
  aiBusy = true;
  aiError = null;
  render();
  const messages = [
    { role: "system", content: aiSystemPrompt() },
    ...aiTurns.flatMap((t) => [
      { role: "user", content: t.q },
      { role: "assistant", content: t.a },
    ]),
    { role: "user", content: q },
  ];
  try {
    const a = await api.aiChat(messages);
    aiTurns.push({ q, a });
    if (aiCtx) {
      aiHistorySave(aiConvKey(aiCtx), {
        keyword: aiCtx.keyword,
        turns: aiTurns,
        ts: Date.now(),
      });
    }
  } catch (e) {
    aiError = e instanceof Error ? e.message : String(e);
  } finally {
    aiBusy = false;
    render();
  }
}

/** Distill the Q&A turns into a new card via the LLM, then prefill the
 *  add-note form so the user can review and confirm before saving. */
async function aiDistill() {
  if (!aiCtx || aiBusy || !aiTurns.length) return;
  aiBusy = true;
  aiError = null;
  render();
  const ctx = aiCtx;
  const qa = aiTurns.map((t) => `问：${t.q}\n答：${t.a}`).join("\n\n");
  try {
    const text = await api.aiChat([
      {
        role: "system",
        content:
          '把用户提供的卡片问答内容提炼成一张记忆卡片。只输出一个 JSON 对象，格式：{"front":"简短的问题或提示，能独立理解，不引用上下文","back":"答案要点，简洁分点","tags":["相关标签"]}，不要输出其他文字。',
      },
      {
        role: "user",
        content: `卡片原文：\n正面：${ctx.front}\n背面：${ctx.back || "（无）"}\n\n问答记录：\n${qa}`,
      },
    ]);
    const parsed = extractJsonCard(text);
    pendingAICard = parsed
      ? { ...parsed, deck: ctx.deckName || undefined }
      : {
          front: ctx.front,
          back: aiTurns.map((t) => `${t.q}\n${t.a}`).join("\n\n"),
          tags: [],
          deck: ctx.deckName || undefined,
        };
    aiOpen = false;
    aiCtx = null;
    aiBusy = false;
    mode = "add";
    render();
  } catch (e) {
    aiBusy = false;
    aiError = e instanceof Error ? e.message : String(e);
    render();
  }
}

/** Tolerant JSON extraction from an LLM reply: strips code fences,
 *  slices the outermost {...}, and validates the card shape. */
function extractJsonCard(
  text: string,
): { front: string; back: string; tags: string[] } | null {
  let t = text
    .trim()
    .replace(/^```(?:json)?\s*/i, "")
    .replace(/```\s*$/, "")
    .trim();
  const s = t.indexOf("{");
  const e = t.lastIndexOf("}");
  if (s >= 0 && e > s) t = t.slice(s, e + 1);
  try {
    const j = JSON.parse(t) as { front?: unknown; back?: unknown; tags?: unknown };
    if (typeof j.front === "string" && typeof j.back === "string") {
      return {
        front: j.front,
        back: j.back,
        tags: Array.isArray(j.tags) ? j.tags.map(String) : [],
      };
    }
  } catch {
    /* fall through to caller's fallback */
  }
  return null;
}

function renderAiSheet() {
  const backdrop = el("div", "ai-backdrop");
  backdrop.addEventListener("click", (e) => {
    if (e.target === backdrop) closeAiSheet();
  });

  const sheet = el("div", "ai-sheet");
  const head = el("div", "ai-head");
  head.appendChild(
    el(
      "div",
      "ai-title",
      `AI 问卡 · ${aiCtx!.keyword ? `「${aiCtx!.keyword}」` : aiCtx!.front.slice(0, 24)}`,
    ),
  );
  const closeBtn = el("button", "nav-btn", "✕");
  closeBtn.addEventListener("click", closeAiSheet);
  head.appendChild(closeBtn);
  sheet.appendChild(head);

  const chat = el("div", "ai-chat");
  const intro = el("div", "ai-intro");
  intro.textContent =
    `正面：${aiCtx!.front}\n背面：${aiCtx!.back || "（无）"}${aiCtx!.example ? `\n例句：${aiCtx!.example}` : ""}${aiCtx!.keyword ? `\n🔑 关键词：${aiCtx!.keyword}` : ""}`;
  chat.appendChild(intro);
  for (const t of aiTurns) {
    chat.appendChild(el("div", "ai-q", t.q));
    const a = el("div", "ai-a md");
    a.innerHTML = renderMarkdown(t.a);
    chat.appendChild(a);
  }
  if (aiBusy) chat.appendChild(el("div", "ai-typing", "思考中…"));
  if (aiError) chat.appendChild(el("div", "ai-error", aiError));
  sheet.appendChild(chat);
  setTimeout(() => {
    chat.scrollTop = chat.scrollHeight;
  }, 0);

  const chips = el("div", "ai-chips");
  const newBtn = el("button", "ai-chip", "新对话");
  newBtn.addEventListener("click", () => {
    aiTurns = [];
    aiError = null;
    if (aiCtx) {
      aiHistorySave(aiConvKey(aiCtx), {
        keyword: aiCtx.keyword,
        turns: [],
        ts: Date.now(),
      });
    }
    render();
  });
  chips.appendChild(newBtn);
  for (const [label, prompt] of aiPresets(aiCtx!.keyword)) {
    const chip = el("button", "ai-chip", label);
    chip.addEventListener("click", () => void aiAsk(prompt));
    chips.appendChild(chip);
  }
  sheet.appendChild(chips);

  const inputRow = el("div", "ai-inputrow");
  const input = document.createElement("input");
  input.type = "text";
  input.placeholder = "针对这张卡片提问…";
  if (aiCtx!.keyword) input.value = `详解「${aiCtx!.keyword}」`;
  const send = () => {
    const v = input.value;
    input.value = "";
    void aiAsk(v);
  };
  input.addEventListener("keydown", (e) => {
    if (e.key === "Enter") send();
  });
  const sendBtn = el("button", "reveal ai-send", aiBusy ? "…" : "发送");
  sendBtn.disabled = aiBusy;
  sendBtn.addEventListener("click", send);
  inputRow.append(input, sendBtn);
  sheet.appendChild(inputRow);

  const actions = el("div", "form-actions");
  const distill = el("button", "reveal", "✂ 提炼成卡片");
  distill.disabled = aiBusy || !aiTurns.length;
  distill.addEventListener("click", () => void aiDistill());
  actions.appendChild(distill);
  sheet.appendChild(actions);
  sheet.appendChild(
    el("p", "form-hint", "提炼后进入新建卡片，确认后保存"),
  );

  backdrop.appendChild(sheet);
  return backdrop;
}

/** One-click setup: anka-server logs a URL with #t=<token>; opening it
 *  configures the connection to the serving origin automatically. */
function applySetupToken() {
  if (location.hash.length < 2) return;
  const params = new URLSearchParams(location.hash.slice(1));
  const token = params.get("t");
  if (token) {
    localStorage.setItem("anka.server", location.origin);
    localStorage.setItem("anka.token", token);
  }
  history.replaceState(null, "", location.pathname + location.search);
}

async function boot() {
  applySetupToken();
  loading = true;
  render();
  try {
    decks = await api.decks();
    error = null;
  } catch (e) {
    error = String(e);
  } finally {
    loading = false;
    render();
  }
}

async function refreshDecks() {
  try {
    decks = await api.decks();
  } catch (e) {
    error = String(e);
  }
  render();
}

async function startSession(deckName: string) {
  loading = true;
  mode = "session";
  render();
  try {
    const cards = await api.due(deckName, 50);
    session = {
      deckName,
      cards,
      index: 0,
      revealed: false,
      graded: 0,
    };
    error = null;
    if (cards[0]) playSounds(cards[0].sounds);
  } catch (e) {
    error = String(e);
  } finally {
    loading = false;
    render();
  }
}

async function grade(rating: number) {
  if (!session) return;
  const current = session.cards[session.index];
  if (!current) return;
  try {
    const result = await api.grade(current.cardId, rating);
    session.lastGrade = result ?? undefined;
    session.graded += 1;
    session.index += 1;
    session.revealed = false;
    const next = session.cards[session.index];
    if (next) playSounds(next.sounds);
    else stopSounds();
    render();
    if (session.index >= session.cards.length) void maybeAutoSync();
  } catch (e) {
    error = String(e);
    render();
  }
}

window.addEventListener("popstate", () => {
  const st = (history.state || {}) as { mode?: Mode };
  const target = (st.mode as Mode) || "home";
  if (target === mode) return;
  if (mode === "session" && target !== "session") session = null;
  mode = target;
  renderedMode = mode;
  render();
});

window.addEventListener("keydown", (ev) => {
  if (error && ev.key === "Escape") {
    error = null;
    render();
    return;
  }
  if (aiOpen) {
    if (ev.key === "Escape") closeAiSheet();
    return;
  }
  if (!session) {
    const studyable = studyableDecks(decks);
    if (ev.key === "ArrowDown") {
      ev.preventDefault();
      selected = Math.min(selected + 1, Math.max(studyable.length - 1, 0));
      render();
      return;
    }
    if (ev.key === "ArrowUp") {
      ev.preventDefault();
      selected = Math.max(selected - 1, 0);
      render();
      return;
    }
    if (ev.key === "Enter") {
      const d = studyable[selected];
      if (d) void startSession(d.name);
    }
    return;
  }
  if (ev.key === "Escape") {
    stopSounds();
    session = null;
    void refreshDecks();
    return;
  }
  if (!session.revealed && (ev.key === " " || ev.key === "Enter")) {
    ev.preventDefault();
    session.revealed = true;
    render();
    return;
  }
  if (session.revealed && ["1", "2", "3", "4"].includes(ev.key)) {
    void grade(Number(ev.key));
  }
});

function fieldInput(label: string, value: string, multiline = false) {
  const wrap = el("label", "field");
  wrap.appendChild(el("span", undefined, label));
  const input = multiline
    ? document.createElement("textarea")
    : document.createElement("input");
  if (!multiline) (input as HTMLInputElement).type = "text";
  input.value = value;
  if (multiline) (input as HTMLTextAreaElement).rows = 4;
  wrap.appendChild(input);
  return { wrap, input };
}

function renderAddForm() {
  const aiSeed = pendingAICard;
  const panel = el("div", "form-panel");
  panel.appendChild(el("h1", undefined, aiSeed ? "AI 提炼的卡片" : "新建卡片"));
  if (aiSeed) {
    panel.appendChild(el("p", "form-hint", "由 AI 问答提炼生成，可修改后保存。"));
  }
  const deckField = fieldInput(
    "牌组",
    aiSeed?.deck || decks[0]?.name || "Default",
  );
  const frontField = fieldInput("正面 / 单词", aiSeed?.front || "");
  const backField = fieldInput("背面 / 释义", aiSeed?.back || "", true);
  const tagsField = fieldInput("标签（空格分隔）", aiSeed?.tags.join(" ") || "");
  panel.append(deckField.wrap, frontField.wrap, backField.wrap, tagsField.wrap);

  const actions = el("div", "form-actions");
  const save = el("button", "reveal", "保存卡片");
  const cancel = el("button", "nav-btn", "取消");
  cancel.onclick = () => {
    pendingAICard = null;
    mode = "home";
    render();
  };
  const doSave = async () => {
    const front = frontField.input.value.trim();
    const back = backField.input.value.trim();
    if (!front) {
      error = "正面不能为空";
      render();
      return;
    }
    loading = true;
    render();
    try {
      await api.createNote(
        deckField.input.value.trim() || "Default",
        front,
        back,
        tagsField.input.value.split(/\s+/).filter(Boolean),
      );
      flash = "已添加";
      pendingAICard = null;
      mode = "home";
      error = null;
      await refreshDecks();
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
      render();
    }
  };
  save.onclick = () => void doSave();
  actions.append(cancel, save);
  panel.appendChild(actions);

  const onKey = (ev: KeyboardEvent) => {
    if ((ev.ctrlKey || ev.metaKey) && ev.key === "Enter") {
      ev.preventDefault();
      void doSave();
      window.removeEventListener("keydown", onKey);
    }
  };
  window.addEventListener("keydown", onKey);
  setTimeout(() => frontField.input.focus(), 0);
  return panel;
}

async function loadBrowse() {
  loading = true;
  render();
  try {
    const res = await api.searchNotes(browseQuery, 200);
    browseTotal = res.total;
    browseItems = res.items;
    error = null;
  } catch (e) {
    error = String(e);
  } finally {
    loading = false;
    render();
  }
}

function renderBrowse() {
  const panel = el("div", "form-panel");
  panel.appendChild(el("h1", undefined, "全部卡组"));
  const searchRow = el("div", "search-row");
  const q = document.createElement("input");
  q.type = "search";
  q.placeholder = "搜索单词 / 释义…";
  q.value = browseQuery;
  const go = el("button", "nav-btn", "搜索");
  go.onclick = () => {
    browseQuery = q.value.trim();
    void loadBrowse();
  };
  q.addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      browseQuery = q.value.trim();
      void loadBrowse();
    }
  });
  searchRow.append(q, go);
  panel.appendChild(searchRow);

  if (!browseItems.length) {
    panel.appendChild(
      el("p", "form-hint", browseQuery ? "没有匹配的笔记，试试别的关键词。" : "还没有卡片：点 + 新建，或登录 AnkiWeb 导入。"),
    );
    return panel;
  }

  panel.appendChild(
    el("p", "form-hint", `共 ${browseTotal} 条笔记，按牌组分组 · 长按词语问 AI`),
  );

  const byDeck = new Map<string, NoteDto[]>();
  for (const n of browseItems) {
    const key = n.deckName || "默认牌组";
    if (!byDeck.has(key)) byDeck.set(key, []);
    byDeck.get(key)!.push(n);
  }
  for (const [deck, notes] of byDeck) {
    panel.appendChild(el("h2", undefined, `${deck}（${notes.length}）`));
    const list = el("div", "note-list");
    for (const n of notes) {
      const row = el("div", "note-row");
      const main = el("div", "note-main");
      main.appendChild(el("div", "note-front", n.front || n.fields[0] || ""));
      main.appendChild(el("div", "note-back", n.back || n.fields[1] || ""));
      const edit = el("button", "nav-btn", "编辑");
      edit.onclick = () => {
        editNote = n;
        mode = "edit";
        render();
      };
      row.append(main, edit);
      onLongPress(row, (x, y) =>
        openAiSheet({
          deckName: n.deckName,
          front: n.front || n.fields[0] || "",
          back: n.back || n.fields[1] || "",
          keyword: keywordAtPointIn(row, x, y),
        }),
      );
      list.appendChild(row);
    }
    panel.appendChild(list);
  }
  return panel;
}

function renderEditForm(note: NoteDto) {
  const panel = el("div", "form-panel");
  panel.appendChild(el("h1", undefined, "编辑笔记"));
  panel.appendChild(el("p", "form-hint", note.deckName || ""));

  const fields = [...note.fields];
  while (fields.length < 2) fields.push("");

  const inputs: HTMLInputElement[] = [];
  const labels = ["字段 1（通常为单词）", "字段 2（通常为释义）", "字段 3", "字段 4"];
  fields.slice(0, 4).forEach((v, i) => {
    const f = fieldInput(labels[i] || `字段 ${i + 1}`, v, i >= 1);
    inputs.push(f.input as HTMLInputElement);
    panel.appendChild(f.wrap);
  });
  const tagsField = fieldInput("标签（空格分隔）", note.tags.join(" "));
  panel.appendChild(tagsField.wrap);

  const actions = el("div", "form-actions");
  const cancel = el("button", "nav-btn", "取消");
  cancel.onclick = () => {
    mode = "browse";
    render();
  };
  const save = el("button", "reveal", "保存修改");
  const doSave = async () => {
    const nextFields = inputs.map((inp) =>
      "value" in inp ? String(inp.value) : "",
    );
    // textareas may not be HTMLInputElement
    const fieldEls = panel.querySelectorAll<HTMLInputElement | HTMLTextAreaElement>(
      ".field input, .field textarea",
    );
    const values = Array.from(fieldEls)
      .slice(0, fields.length)
      .map((el) => el.value);
    loading = true;
    render();
    try {
      await api.updateNote(
        note.id,
        values.length ? values : nextFields,
        tagsField.input.value.split(/\s+/).filter(Boolean),
      );
      flash = "已保存";
      mode = "browse";
      error = null;
      await loadBrowse();
    } catch (e) {
      error = String(e);
      loading = false;
      render();
    }
  };
  save.onclick = () => void doSave();
  actions.append(cancel, save);
  panel.appendChild(actions);
  return panel;
}

void boot();
