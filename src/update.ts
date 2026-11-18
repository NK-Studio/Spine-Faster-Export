// 앱 업데이트 창(Ramus 의 update.js 와 같은 구조). 켤 때와 그 뒤 6시간마다 새 판이 있는지 보고(설정 › 일반 › «새 버전이 나오면 알리기»),
// 메뉴의 «업데이트 확인…» 은 바로 묻는다(Rust: src-tauri/src/updater.rs 의 update_check · update_install).
// 창에는 새 판 번호와 그 판에서 바뀐 내용(ChangeLog.md 항목, 건너뛴 사이 판은 빼고 가장 새 판만)을 보이고, [업데이트하고 다시 시작]을 누르면
// 앱이 받아서 바꿔 넣고 다시 켠다(진행 막대 → «다시 시작합니다»). 앱 안에서 바꿀 수 없는 경우(MSI 판·쓸 수 없는 폴더)는 내려받기 페이지를 연다.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";

import { t } from "./i18n";

interface ReleaseInfo {
  version: string;
  date: string;
  /** ChangeLog.md 의 그 판 항목(마크다운). */
  notes: string;
}

interface UpdateInfo {
  disabled?: boolean;
  available?: boolean;
  current: string;
  version?: string;
  date?: string;
  releases?: ReleaseInfo[];
  page?: string;
  canInstall?: boolean;
}

type Toast = (kind: "info" | "success" | "warning" | "error", title: string, sub?: string) => void;

/** 업데이트 설정(localStorage). auto: 자동 확인, skip: 건너뛴 판, lastVersion: 마지막으로 켠 판. */
export interface UpdatePrefs {
  auto: boolean;
  skip: string;
  lastVersion: string;
}

const PREFS_KEY = "sfe.update";
const DEFAULT_PREFS: UpdatePrefs = { auto: true, skip: "", lastVersion: "" };
/** 자동 확인 간격. */
const INTERVAL_MS = 6 * 60 * 60 * 1000;

export function loadUpdatePrefs(): UpdatePrefs {
  try {
    return { ...DEFAULT_PREFS, ...JSON.parse(localStorage.getItem(PREFS_KEY) ?? "{}") };
  } catch {
    return { ...DEFAULT_PREFS };
  }
}

export function saveUpdatePrefs(patch: Partial<UpdatePrefs>) {
  try {
    localStorage.setItem(PREFS_KEY, JSON.stringify({ ...loadUpdatePrefs(), ...patch }));
  } catch {
    /* 저장 불가는 무시 */
  }
}

const esc = (s: string) => s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);

/** 바이트 → "12.3 MB". */
const megabytes = (bytes: number) => `${(bytes / (1024 * 1024)).toFixed(1)} MB`;

const message = (e: unknown) => (typeof e === "string" ? e : e instanceof Error ? e.message : String(e));

/** ChangeLog.md 항목만 그리면 된다: ### 제목, - 목록, `코드`, **굵게**, [글](https://…). 먼저 HTML 을 모두 이스케이프한다. */
function inline(text: string): string {
  return esc(text)
    .replace(/`([^`]+)`/g, "<code>$1</code>")
    .replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>")
    .replace(/\[([^\]]+)\]\((https:\/\/[^)\s"]+)\)/g, '<a href="$2">$1</a>');
}

export function markdownToHtml(text: string): string {
  let html = "";
  let list = false;
  for (const line of String(text || "").split("\n")) {
    const item = /^\s*[-*] (.*)$/.exec(line);
    if (item) {
      if (!list) html += "<ul>";
      list = true;
      html += `<li>${inline(item[1])}</li>`;
      continue;
    }
    if (list) html += "</ul>";
    list = false;
    const head = /^#{1,6} (.*)$/.exec(line);
    if (head) html += `<h3>${inline(head[1])}</h3>`;
    else if (line.trim()) html += `<p>${inline(line)}</p>`;
  }
  return list ? `${html}</ul>` : html;
}

export interface UpdaterOptions {
  toast: Toast;
  /** 지금 판(Cargo.toml 의 version). */
  version: string;
  /** 내보내는 중이면 설치하지 않는다(앱을 끄면 끊긴다). */
  busy: () => boolean;
}

/**
 * 업데이트 기능을 붙인다. 돌려주는 start() 는 첫 화면을 다 그린 뒤 한 번, check() 는 메뉴의 «업데이트 확인…».
 */
export function createUpdater({ toast, version, busy }: UpdaterOptions) {
  // 업데이트하고 다시 켰다 — 한 번 알린다.
  const last = loadUpdatePrefs().lastVersion;
  if (version && last !== version) {
    if (last) toast("success", t("Quick Spine {version}(으)로 업데이트했습니다.", { version }));
    saveUpdatePrefs({ lastVersion: version });
  }

  // 자동 확인으로 찾은 판은 다른 대화상자(덮어쓰기 확인·설정 가져오기 등)가 닫힌 뒤에 띄운다. 같은 판은 한 번만.
  let shown: string | null = null;
  let waiting: ReturnType<typeof setTimeout> | null = null;
  const offer = (info: UpdateInfo | null) => {
    if (!info?.available || !info.version || loadUpdatePrefs().skip === info.version || shown === info.version) return;
    if (document.querySelector(".modal-backdrop") || busy()) {
      if (waiting) clearTimeout(waiting);
      waiting = setTimeout(() => offer(info), 1500);
      return;
    }
    shown = info.version;
    showUpdate(info, busy);
  };
  const auto = async () => {
    if (!loadUpdatePrefs().auto) return;
    offer(await invoke<UpdateInfo>("update_check").catch(() => null));
  };

  let started = false;
  return {
    /** 첫 화면을 다 그렸다. 지금 한 번, 그 뒤 6시간마다 자동으로 본다. */
    start() {
      if (started) return;
      started = true;
      void auto();
      setInterval(() => void auto(), INTERVAL_MS);
    },
    async check() {
      try {
        const info = await invoke<UpdateInfo>("update_check");
        if (info.disabled) toast("info", t("이 빌드는 업데이트를 확인하지 않습니다."));
        else if (!info.available) toast("success", t("최신 버전입니다 (Quick Spine {version}).", { version: info.current }));
        else showUpdate(info, busy);
      } catch (e) {
        toast("error", t("업데이트를 확인하지 못했습니다"), message(e));
      }
    },
  };
}

/** 업데이트 창을 띄운다. 이미 떠 있으면 아무것도 하지 않는다. */
function showUpdate(info: UpdateInfo, busyApp: () => boolean) {
  if (document.querySelector(".modal.update")) return;
  let busy = false;

  const backdrop = document.createElement("div");
  backdrop.className = "modal-backdrop";
  const title = t("새 버전이 있습니다");
  const releases = (info.releases ?? [])
    .map(
      (r) => `<section class="update-release">
        <div class="update-release-title"><span>Quick Spine ${esc(r.version)}</span>${r.date ? `<span class="muted">${esc(r.date)}</span>` : ""}</div>
        <div class="markdown">${markdownToHtml(r.notes)}</div></section>`,
    )
    .join("");
  backdrop.innerHTML = `<div class="modal update" role="dialog" aria-modal="true" aria-label="${esc(title)}">
    <div class="update-head">
      <div class="update-title">${esc(title)}</div>
      <div class="update-sub">${esc(t("Quick Spine {version}을(를) 쓸 수 있습니다. 지금은 {current}입니다.", { version: info.version ?? "", current: info.current }))}</div>
    </div>
    <div class="update-label">${esc(t("새로 바뀐 내용"))}</div>
    <div class="update-notes">${releases || `<div class="muted">${esc(t("적힌 내용이 없습니다."))}</div>`}</div>
    <div class="update-status" hidden></div>
    <div class="update-bar" hidden><div class="update-bar-fill"></div></div>
    <div class="modal-error"></div>
    <div class="modal-actions">
      <button class="btn update-skip" data-act="skip">${esc(t("이 버전 건너뛰기"))}</button>
      <div class="spacer"></div>
      <button class="btn" data-act="later">${esc(t("나중에"))}</button>
      <button class="btn primary" data-act="ok">${esc(info.canInstall ? t("업데이트하고 다시 시작") : t("내려받기 페이지 열기"))}</button>
    </div>
  </div>`;
  const $ = <T extends HTMLElement>(sel: string) => backdrop.querySelector(sel) as T;
  const notes = $(".update-notes");
  const status = $(".update-status");
  const progress = $(".update-bar");
  const bar = $(".update-bar-fill");
  const problem = $(".modal-error");
  const skip = $<HTMLButtonElement>('[data-act="skip"]');
  const later = $<HTMLButtonElement>('[data-act="later"]');
  const ok = $<HTMLButtonElement>('[data-act="ok"]');

  // 글 속 링크는 브라우저로 연다(웹뷰 안에서는 열지 않는다).
  notes.addEventListener("click", (event) => {
    const a = (event.target as HTMLElement).closest?.("a[href]") as HTMLAnchorElement | null;
    if (!a) return;
    event.preventDefault();
    if (a.href.startsWith("https://")) void openUrl(a.href).catch(() => {});
  });

  const close = () => {
    if (busy || !backdrop.isConnected) return;
    document.removeEventListener("keydown", onKey, true);
    backdrop.classList.add("leaving");
    setTimeout(() => backdrop.remove(), 260);
  };
  const onKey = (event: KeyboardEvent) => {
    if (event.key === "Escape" || event.key === "Esc") {
      event.preventDefault();
      event.stopPropagation();
      close();
    }
  };
  const openPage = () => {
    if (info.page) void openUrl(info.page).catch((e) => (problem.textContent = message(e)));
    close();
  };
  const install = async () => {
    if (busyApp()) {
      problem.textContent = t("내보내기가 끝난 뒤에 업데이트해 주세요.");
      return;
    }
    busy = true;
    problem.textContent = "";
    for (const b of [skip, later, ok]) b.disabled = true;
    status.hidden = false;
    progress.hidden = false;
    status.textContent = t("내려받는 중…");
    const off = await listen<{ received: number; total: number }>("update-progress", ({ payload: { received, total } }) => {
      if (total > 0) {
        bar.style.width = `${Math.min(100, (received / total) * 100)}%`;
        status.textContent = t("내려받는 중… {done} / {total}", { done: megabytes(received), total: megabytes(total) });
      }
    });
    try {
      await invoke("update_install", { version: info.version });
      bar.style.width = "100%";
      status.textContent = t("설치를 시작했습니다. Quick Spine 을 다시 시작합니다…");
    } catch (e) {
      busy = false;
      progress.hidden = true;
      status.hidden = true;
      problem.textContent = message(e);
      for (const b of [skip, later, ok]) b.disabled = false;
    } finally {
      off();
    }
  };

  skip.onclick = () => {
    saveUpdatePrefs({ skip: info.version ?? "" });
    close();
  };
  later.onclick = close;
  ok.onclick = info.canInstall ? () => void install() : openPage;
  backdrop.addEventListener("mousedown", (event) => event.target === backdrop && close());
  document.addEventListener("keydown", onKey, true);
  document.body.appendChild(backdrop);
  ok.focus();
}
