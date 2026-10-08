import * as vscode from "vscode";

type DueCard = {
  cardId: string;
  deckName: string;
  front: string;
  back: string;
  example?: string;
};

type GradeItem = vscode.QuickPickItem & { rating?: number; action?: string };

const GRADES: Array<{ label: string; detail: string; rating: number }> = [
  { label: "$(error) 1 · Again", detail: "重来（今天再见）", rating: 1 },
  { label: "$(dash) 2 · Hard", detail: "困难", rating: 2 },
  { label: "$(pass) 3 · Good", detail: "良好", rating: 3 },
  { label: "$(zap) 4 · Easy", detail: "简单", rating: 4 },
];

let status: vscode.StatusBarItem;
let queue: DueCard[] = [];
let current: DueCard | null = null;
let revealed = false;
let revealTimer: ReturnType<typeof setTimeout> | undefined;
let autoTimer: ReturnType<typeof setTimeout> | undefined;
let total = 0;
let done = 0;

function config() {
  return vscode.workspace.getConfiguration("anka");
}
function base(): string {
  return (config().get<string>("server") || "").replace(/\/+$/, "");
}
function token(): string {
  return config().get<string>("token") || "";
}
function trunc(s: string, n = 36): string {
  const t = s.replace(/\s+/g, " ").trim();
  return t.length > n ? t.slice(0, n - 1) + "…" : t;
}

async function api<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(base() + path, {
    ...init,
    headers: {
      "content-type": "application/json",
      ...(token() ? { authorization: `Bearer ${token()}` } : {}),
      ...(init?.headers ?? {}),
    },
  });
  if (!res.ok) {
    throw new Error(`HTTP ${res.status}: ${(await res.text()).slice(0, 160)}`);
  }
  return (await res.json()) as T;
}

function render() {
  const count = config().get<boolean>("showCount", true);
  if (!current) {
    status.text =
      total > 0 && done >= total
        ? `$(check-all) Anka ${done}/${total}`
        : "$(check-all) Anka ✓";
    status.tooltip = new vscode.MarkdownString(
      "本轮完成。点击重新拉取到期卡片。",
    );
    return;
  }
  const parts = ["$(book)", trunc(revealed ? current.back : current.front)];
  if (count) parts.push(`${done + 1}/${total}`);
  status.text = parts.join(" ");
  // 悬停不剧透：未揭示时 tooltip 只显示正面和操作提示
  const md = new vscode.MarkdownString();
  md.appendMarkdown(
    [
      `**${trunc(current.front, 60)}**`,
      "",
      revealed
        ? `### ${trunc(current.back, 200)}`
        : "答案已隐藏。`Ctrl+Alt+Space` 或点击状态栏查看并评分",
      current.example && revealed ? `\n> ${trunc(current.example, 160)}` : "",
      "",
      `\`${current.deckName}\` · 剩余 ${queue.length} 张 · \`Ctrl+Alt+1..4\` 评分`,
    ].join("\n"),
  );
  status.tooltip = md;
}

function scheduleAuto() {
  if (autoTimer) clearTimeout(autoTimer);
  const mins = config().get<number>("autoRefreshMinutes", 15);
  if (mins > 0) {
    autoTimer = setTimeout(() => void refresh(), mins * 60_000);
  }
}

async function refresh() {
  if (!base()) {
    status.text = "$(book) Anka 未配置";
    status.tooltip = new vscode.MarkdownString(
      "设置 `anka.server` 与 `anka.token` 后即可在状态栏背卡。",
    );
    return;
  }
  const deck = config().get<string>("deck") || "";
  const size = config().get<number>("pullSize", 20);
  const qs = new URLSearchParams({ limit: String(size) });
  if (deck) qs.set("deck", deck);
  try {
    const cards = await api<DueCard[]>(`/api/due?${qs}`);
    queue = cards.slice();
    current = queue.shift() ?? null;
    total = queue.length + (current ? 1 : 0);
    done = 0;
    revealed = false;
    render();
    scheduleAuto();
  } catch (e) {
    status.text = "$(error) Anka";
    status.tooltip = new vscode.MarkdownString(`连接失败：${e}`);
    scheduleAuto();
  }
}

function reveal() {
  if (!current) return;
  revealed = true;
  render();
  if (revealTimer) clearTimeout(revealTimer);
  const secs = Math.max(1, config().get<number>("revealSeconds", 8));
  revealTimer = setTimeout(() => {
    revealed = false;
    render();
  }, secs * 1000);
}

function skip() {
  if (!current) return;
  queue.push(current);
  current = queue.shift() ?? null;
  revealed = false;
  render();
}

async function grade(rating: number) {
  if (!current) {
    void refresh();
    return;
  }
  const card = current;
  try {
    await api("/api/grade", {
      method: "POST",
      body: JSON.stringify({ cardId: card.cardId, rating }),
    });
  } catch (e) {
    vscode.window.showErrorMessage(`Anka 评分失败：${e}`);
    return;
  }
  done += 1;
  revealed = false;
  if (revealTimer) clearTimeout(revealTimer);
  current = queue.shift() ?? null;
  if (!current) {
    await refresh();
  } else {
    render();
  }
}

function showPicker() {
  if (!base()) {
    void vscode.commands.executeCommand("anka.openSettings");
    return;
  }
  if (!current) {
    void refresh();
    return;
  }
  const card = current;
  const pick = vscode.window.createQuickPick();
  pick.title = trunc(card.front, 60);
  pick.placeholder = "回忆一下，再选评分（Esc 关闭）";
  pick.items = [
    { label: `$(eye) 答案：${trunc(card.back, 80)}`, action: "reveal" },
    ...GRADES.map((g) => ({ label: g.label, detail: g.detail, rating: g.rating })),
    { label: "$(arrow-right) 跳过这张", action: "skip" },
    { label: "$(refresh) 换一批", action: "refresh" },
  ] as GradeItem[];
  pick.onDidChangeSelection((sel) => {
    const item = sel[0] as GradeItem | undefined;
    if (!item) return;
    pick.hide();
    if (item.rating) {
      void grade(item.rating);
    } else if (item.action === "reveal") {
      reveal();
    } else if (item.action === "skip") {
      skip();
    } else if (item.action === "refresh") {
      void refresh();
    }
  });
  pick.show();
}

export function activate(ctx: vscode.ExtensionContext) {
  status = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Right, 50);
  status.name = "Anka 背诵";
  status.command = "anka.show";
  status.show();
  ctx.subscriptions.push(status);

  const reg = (cmd: string, fn: () => void) =>
    ctx.subscriptions.push(vscode.commands.registerCommand(cmd, fn));
  reg("anka.show", showPicker);
  reg("anka.reveal", reveal);
  reg("anka.skip", skip);
  reg("anka.refresh", () => void refresh());
  reg("anka.again", () => void grade(1));
  reg("anka.hard", () => void grade(2));
  reg("anka.good", () => void grade(3));
  reg("anka.easy", () => void grade(4));
  reg("anka.openSettings", () =>
    vscode.commands.executeCommand("workbench.action.openSettings", "anka.server"),
  );
  reg("anka.openWeb", () => {
    const b = base();
    if (b) void vscode.env.openExternal(vscode.Uri.parse(b));
  });

  void refresh();
  ctx.subscriptions.push(
    vscode.workspace.onDidChangeConfiguration((e) => {
      if (e.affectsConfiguration("anka")) void refresh();
    }),
  );
}

export function deactivate() {
  if (autoTimer) clearTimeout(autoTimer);
  if (revealTimer) clearTimeout(revealTimer);
}
