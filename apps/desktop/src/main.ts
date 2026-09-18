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

type Mode = "home" | "session" | "add" | "browse" | "edit" | "settings";

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
};

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
let error: string | null = null;
let selected = 0;
let mode: Mode = "home";
let browseQuery = "";
let browseItems: NoteDto[] = [];
let editNote: NoteDto | null = null;
let flash: string | null = null;

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
    const browseBtn = el("button", "nav-btn", "浏览");
    browseBtn.onclick = () => {
      mode = "browse";
      void loadBrowse();
    };
    const settingsBtn = el("button", "nav-btn", "⚙");
    settingsBtn.title = "连接服务器";
    settingsBtn.onclick = () => {
      mode = "settings";
      render();
    };
    nav.append(addBtn, browseBtn, settingsBtn);
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
  panel.appendChild(el("h1", undefined, "连接服务器"));
  panel.appendChild(
    el(
      "p",
      "settings-hint",
      "填写自托管 anka-server 地址即为客户端模式（手机/平板推荐，与 NAS 上的收藏实时同步）；留空则使用本机收藏。",
    ),
  );
  const cfg = remoteConfig();
  const serverField = fieldInput("服务器地址", cfg.base);
  serverField.input.placeholder = "http://192.168.10.3:8787";
  const tokenField = fieldInput("Token（ANKA_SERVER_TOKEN）", cfg.token);
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
  const cancel = el("button", "nav-btn", "← 返回");
  cancel.onclick = () => {
    mode = "home";
    render();
  };
  actions.append(save, clear, cancel);
  panel.appendChild(actions);
  return panel;
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
  } else {
    keys.innerHTML = `<span class="kbd">↑↓</span> 选择 · <span class="kbd">Enter</span> 开始复习`;
  }
  return keys;
}

function renderLoading() {
  const wrap = el("div", "empty");
  wrap.appendChild(el("h2", undefined, "加载中"));
  wrap.appendChild(el("p", undefined, "正在读取收藏…"));
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
  if (isTauri() || useRemote()) {
    const settings = el("button", "reveal", "⚙ 连接设置");
    settings.addEventListener("click", () => {
      error = null;
      mode = "settings";
      render();
    });
    actions.appendChild(settings);
  }
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
  card.appendChild(el("div", "prompt", tip));

  if (s.revealed) {
    const answer = el("div", "answer");
    answer.appendChild(document.createTextNode(current.back || "（无释义）"));
    if (current.example) {
      answer.appendChild(el("div", "example", current.example));
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

async function boot() {
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
  } catch (e) {
    error = String(e);
    render();
  }
}

window.addEventListener("keydown", (ev) => {
  if (error && ev.key === "Escape") {
    error = null;
    render();
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
  const panel = el("div", "form-panel");
  panel.appendChild(el("h1", undefined, "新建卡片"));
  const deckField = fieldInput(
    "牌组",
    decks[0]?.name || "Default",
  );
  const frontField = fieldInput("正面 / 单词", "");
  const backField = fieldInput("背面 / 释义", "", true);
  const tagsField = fieldInput("标签（空格分隔）", "");
  panel.append(deckField.wrap, frontField.wrap, backField.wrap, tagsField.wrap);

  const actions = el("div", "form-actions");
  const save = el("button", "reveal", "保存卡片");
  const cancel = el("button", "nav-btn", "取消");
  cancel.onclick = () => {
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
    const res = await api.searchNotes(browseQuery, 30);
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
  panel.appendChild(el("h1", undefined, "浏览笔记"));
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
    panel.appendChild(el("p", "form-hint", "没有匹配的笔记，试试别的关键词。"));
    return panel;
  }

  const list = el("div", "note-list");
  for (const n of browseItems) {
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
    list.appendChild(row);
  }
  panel.appendChild(list);
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
