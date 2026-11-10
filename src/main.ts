import "./fonts.css";
import "@fontsource/jetbrains-mono/400.css";
import "./styles.css";

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";

import { icons } from "./icons";
import { attachSuggestions, bindSegmented, enhanceSelects, selectHtml } from "./controls";
import { UNITY_ATLAS_EXT, UNITY_SKEL_EXT, openPackerDialog, packSummary, type PackSettings } from "./packer";

// ───────────────────────────────────────── 타입

interface ProjectInfo {
  path: string;
  fileName: string;
  formatVersion: string;
  skeletons: string[];
  loadMs: number;
  partialError: string | null;
}

type Format = "json" | "binary" | "both";

interface ExportOptions {
  version: string;
  format: Format;
  nonessential: boolean | null;
  clean: boolean;
  atlas: boolean;
  /** Texture Packer Settings(Spine pack.json 키) */
  pack: PackSettings;
  skelExt: string;
  /** JSON 파일 확장자. */
  jsonExt: string;
  jsonFormat: "json" | "javascript" | "minimal";
  prettyPrint: boolean;
  /** Export 체크가 꺼진 첨부도 내보낸다(Spine «Export all»). */
  exportAll: boolean;
  /** 내보낸 뒤 알림(경고)을 보여 준다. */
  warnings: boolean;
  packSource: "attachments" | "imagefolders";
  packTarget: "perskeleton" | "single";
  /** Unity(spine-unity) 형식: atlas .atlas.txt · 바이너리 .skel.bytes. 앱 설정에만 둔다(Rust 는 모르는 키를 무시). */
  unity: boolean;
  /** Unity 를 끄면 돌아갈 바이너리 확장자. */
  skelBeforeUnity: string;
}

interface ExportProgress {
  /** 여러 스켈레톤을 묶은 아틀라스(Single atlas)는 null. */
  index: number | null;
  skeleton: string;
  ok: boolean;
  message: string | null;
  files: string[];
  notes: string[];
  done: number;
  total: number;
}

interface ExportSummary {
  succeeded: number;
  failed: number;
  outDir: string;
  elapsedMs: number;
}

// ───────────────────────────────────────── 저장되는 설정

type SettingsTab = "out" | "skel" | "atlas";
const TABS: [SettingsTab, string][] = [
  ["out", "출력"],
  ["skel", "스켈레톤 데이터"],
  ["atlas", "텍스처 아틀라스"],
];

// crates/spine-atlas `AtlasSettings::default()` 와 같아야 한다(C# 과 같은 결과를 내는 값).
const DEFAULT_PACK: PackSettings = {
  stripWhitespaceX: true,
  stripWhitespaceY: true,
  rotation: true,
  alias: false,
  ignoreBlankImages: false,
  alphaThreshold: 3,
  paddingX: 2,
  paddingY: 2,
  edgePadding: true,
  duplicatePadding: false,
  minWidth: 0,
  minHeight: 0,
  maxWidth: 4096,
  maxHeight: 4096,
  pot: false,
  multipleOfFour: false,
  square: false,
  filterMin: "Linear",
  filterMag: "Linear",
  wrapX: "ClampToEdge",
  wrapY: "ClampToEdge",
  format: "RGBA8888",
  outputFormat: "png",
  grid: false,
  packing: "rectangles",
  premultiplyAlpha: true,
  bleed: false,
  bleedIterations: 2,
  scale: [1],
  scaleSuffix: [""],
  atlasExtension: ".atlas",
  combineSubdirectories: false,
  flattenPaths: false,
  useIndexes: false,
  fast: false,
  debug: false,
  separator: "_",
};

const DEFAULTS: ExportOptions = {
  version: "",
  format: "json",
  nonessential: null,
  clean: false,
  atlas: true,
  pack: DEFAULT_PACK,
  skelExt: ".skel",
  jsonExt: ".json",
  jsonFormat: "json",
  prettyPrint: false,
  exportAll: false,
  warnings: true,
  packSource: "attachments",
  packTarget: "perskeleton",
  unity: false,
  skelBeforeUnity: ".skel",
};

function load<T>(key: string, fallback: T): T {
  try {
    const v = localStorage.getItem(key);
    return v ? { ...fallback, ...JSON.parse(v) } : fallback;
  } catch {
    return fallback;
  }
}
function save(key: string, v: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(v));
  } catch {
    /* 저장 불가는 무시 */
  }
}

const prefs = load("sfe.prefs", {
  theme: "system" as "system" | "dark" | "light",
  outMode: "beside" as "beside" | "custom",
  customOut: "",
  /** 설정 탭 */
  tab: "out" as SettingsTab,
});
let options: ExportOptions = load("sfe.options", DEFAULTS);
options.pack = { ...DEFAULT_PACK, ...(options.pack ?? {}) };
// 예전 버전이 저장한 이미지 폴더 지정은 더 쓰지 않는다(프로젝트에 적힌 경로만 쓴다).
delete (options as unknown as { imagesDir?: unknown }).imagesDir;
if (typeof options.unity !== "boolean") options.unity = options.pack.atlasExtension === UNITY_ATLAS_EXT && options.skelExt === UNITY_SKEL_EXT;

// ───────────────────────────────────────── 상태

let project: ProjectInfo | null = null;
const selected = new Set<number>();
/** 줄 선택(파란 줄). 내보낼지 여부(체크)는 `selected`. */
const picked = new Set<number>();
let focusIndex = -1;
let anchorIndex = -1;
let filter = "";
const exportState = new Map<number, "run" | "ok" | "fail">();
let exporting = false;
let confirming = false;
let loading = false;

// ───────────────────────────────────────── 테마·플랫폼

const isMac = navigator.userAgent.includes("Mac");
const MOD = isMac ? "⌘" : "Ctrl+";
document.documentElement.dataset.platform = isMac ? "mac" : "win";
const media = matchMedia("(prefers-color-scheme: dark)");
function applyTheme() {
  const t = prefs.theme === "system" ? (media.matches ? "dark" : "light") : prefs.theme;
  document.documentElement.dataset.theme = t;
  // 창 제목 막대도 같은 테마로(시스템이면 OS 를 따른다)
  void getCurrentWindow()
    .setTheme(prefs.theme === "system" ? null : prefs.theme)
    .catch(() => {});
}
media.addEventListener("change", applyTheme);
applyTheme();

// ───────────────────────────────────────── 화면 뼈대

const $ = <T extends HTMLElement = HTMLElement>(sel: string) => document.querySelector(sel) as T;
const esc = (s: string) => s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);

$("#app").innerHTML = `
<div class="dlg">
  <section class="group input-group">
    <div class="g-title">입력</div>
    <div class="input-row">
      <span class="lbl">프로젝트:</span>
      <div class="field path-box selectable empty" id="proj-path"><span class="ph">.spine 파일을 이 창에 끌어 놓거나 열기를 누르세요</span></div>
      <button class="btn" id="btn-open" title="열기 (${MOD}O)">${icons.open}<span>열기…</span></button>
      <button class="btn icon" id="btn-reload" title="다시 읽기 (${MOD}R)" disabled>${icons.reload}</button>
    </div>
  </section>

  <div class="body">
    <section class="group sk-group">
      <div class="g-title">
        <label class="check"><input type="checkbox" class="tri-check" id="chk-all" title="모두 체크/해제" disabled />스켈레톤</label>
        <span class="count" id="sel-count"></span>
        <span class="grow"></span>
        <button class="icon-btn" id="btn-invert" title="체크 뒤집기" disabled>
          <svg viewBox="0 0 24 24" fill="currentColor"><path d="M7 17h10V7Zm-2 4q-.825 0-1.412-.587Q3 19.825 3 19V5q0-.825.588-1.413Q4.175 3 5 3h14q.825 0 1.413.587Q21 4.175 21 5v14q0 .825-.587 1.413Q19.825 21 19 21Zm0-2h14V5H5v14Z"/></svg>
        </button>
      </div>
      <label class="field-with-icon" id="filter-box">${icons.search}<input type="text" id="filter" placeholder="이름 거르기 (${MOD}F)" spellcheck="false" disabled /></label>
      <div class="sk-list" id="sk-list" tabindex="0" role="listbox" aria-multiselectable="true" aria-label="스켈레톤"></div>
      <div class="sk-foot" id="sk-foot"></div>
    </section>

    <div class="settings" id="settings"></div>
  </div>

  <div class="actions">
    <button class="btn" id="btn-settings-load" title="Spine 의 내보내기 대화상자에서 저장(Save)한 설정 JSON 을 불러옵니다">설정 불러오기…</button>
    <button class="btn" id="btn-settings-save" title="지금 설정을 Spine 내보내기 설정 JSON 으로 저장합니다(Spine 의 Load · CLI --export 로 읽을 수 있습니다)">설정 저장…</button>
    <span class="grow"></span>
    <button class="btn" id="btn-reveal" disabled>${icons.folder}<span>출력 폴더 열기</span></button>
    <button class="btn primary" id="btn-export" disabled></button>
  </div>
</div>

<footer class="footer">
  <span class="grow" id="foot-msg"></span>
  <button id="btn-theme" title="테마 바꾸기">${icons.theme}<span id="theme-name"></span></button>
  <span class="ver">v0.1.0</span>
  <div class="foot-progress" id="progress"><i></i></div>
</footer>

<div class="drop-scrim" id="drop"><div class="drop-card" id="drop-card"></div></div>
<div class="toasts" id="toasts"></div>
`;

// ───────────────────────────────────────── 토스트

interface ToastOptions {
  /** 토스트 자체를 누르면 할 일(누르면 닫힌다). */
  onClick?: () => void;
  /** 마우스를 올리면 보일 설명. */
  tip?: string;
}

function toast(
  kind: "info" | "success" | "warning" | "error",
  title: string,
  sub?: string,
  actions?: { label: string; run: () => void }[],
  ms = 4200,
  opts: ToastOptions = {},
) {
  const el = document.createElement("div");
  el.className = `toast ${kind}${opts.onClick ? " clickable" : ""}`;
  const ic = kind === "success" ? icons.check : kind === "warning" ? icons.warning : kind === "error" ? icons.error : icons.info;
  el.innerHTML = `${ic.replace("<svg", '<svg class="ic"')}<div class="body"><div class="t">${esc(title)}</div>${sub ? `<div class="sub">${esc(sub)}</div>` : ""}<div class="acts"></div></div>${opts.onClick ? `<span class="open-ic" aria-hidden="true">${icons.folder}</span>` : ""}
    <button class="icon-btn x" title="닫기">${icons.close}</button>`;
  const acts = el.querySelector(".acts") as HTMLElement;
  if (actions?.length) {
    for (const a of actions) {
      const b = document.createElement("button");
      b.className = "btn small";
      b.textContent = a.label;
      b.onclick = (e) => {
        e.stopPropagation(); // 토스트 누르기와 겹치지 않게
        a.run();
        close();
      };
      acts.appendChild(b);
    }
  } else acts.remove();
  const close = () => {
    if (el.classList.contains("out")) return;
    el.classList.add("out");
    setTimeout(() => el.remove(), 220);
  };
  (el.querySelector(".x") as HTMLElement).onclick = (e) => {
    e.stopPropagation();
    close();
  };
  if (opts.tip) el.title = opts.tip;
  if (opts.onClick) {
    el.setAttribute("role", "button");
    el.tabIndex = 0;
    const run = () => {
      opts.onClick!();
      close();
    };
    el.addEventListener("click", (e) => {
      // 글자를 끌어 고른 경우는 누르기로 보지 않는다
      if (getSelection()?.toString()) return;
      if ((e.target as HTMLElement).closest("button")) return;
      run();
    });
    el.addEventListener("keydown", (e) => {
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        run();
      }
    });
  }
  $("#toasts").appendChild(el);
  if (ms > 0) {
    let t = setTimeout(close, ms);
    el.onmouseenter = () => clearTimeout(t);
    el.onmouseleave = () => (t = setTimeout(close, 3000));
  }
}

// ───────────────────────────────────────── 스켈레톤 목록

function visibleIndices(): number[] {
  if (!project) return [];
  const f = filter.trim().toLowerCase();
  return project.skeletons.map((_, i) => i).filter((i) => !f || project!.skeletons[i].toLowerCase().includes(f));
}

function rowHtml(i: number): string {
  const name = project!.skeletons[i];
  const cut = name.lastIndexOf("/");
  const label = cut >= 0 ? `<span class="folder">${esc(name.slice(0, cut + 1))}</span>${esc(name.slice(cut + 1))}` : esc(name);
  const st = exportState.get(i);
  const stHtml = st === "ok" ? `<span class="state ok" title="내보냄">${icons.check}</span>` : st === "fail" ? `<span class="state fail" title="실패">${icons.error}</span>` : st === "run" ? `<span class="state run"></span>` : "";
  return `<div class="sk-row" role="option" data-i="${i}" title="${esc(name)}">
    <input type="checkbox" class="tri-check" tabindex="-1" title="내보내기 체크" />
    <span class="name">${label}</span>${stHtml}</div>`;
}

function renderList() {
  const list = $("#sk-list");
  if (!project) {
    list.innerHTML = `<div class="empty-note">${icons.open.replace("<svg", '<svg class="hero"')}<div>.spine 파일을 끌어 놓으세요</div><div class="sub">파인더·탐색기의 «다음으로 열기»도 됩니다</div></div>`;
  } else {
    const vis = visibleIndices();
    if (!vis.length) list.innerHTML = `<div class="empty-note">${project.skeletons.length ? "맞는 스켈레톤이 없습니다." : "스켈레톤이 없는 프로젝트입니다."}</div>`;
    else list.innerHTML = vis.map(rowHtml).join("");
    $("#filter-box").classList.toggle("nomatch", !!filter && !vis.length);
  }
  paintRows();
}

/** 줄 모양만 다시 칠한다(선택·포커스·체크). 끌어서 고를 때도 쓴다. */
function paintRows() {
  document.querySelectorAll<HTMLElement>("#sk-list .sk-row").forEach((r) => {
    const i = Number(r.dataset.i);
    r.classList.toggle("picked", picked.has(i));
    r.classList.toggle("focus", i === focusIndex);
    r.classList.toggle("on", selected.has(i));
    r.setAttribute("aria-selected", String(picked.has(i)));
    (r.firstElementChild as HTMLInputElement).checked = selected.has(i);
  });
  renderSelectionUi();
}

function renderSelectionUi() {
  const n = project?.skeletons.length ?? 0;
  const all = $<HTMLInputElement>("#chk-all");
  all.disabled = !project || exporting || n === 0;
  all.checked = n > 0 && selected.size === n;
  all.indeterminate = selected.size > 0 && selected.size < n;
  $("#sel-count").textContent = project ? `${selected.size}/${n}` : "";
  $<HTMLButtonElement>("#btn-invert").disabled = !project || exporting;
  const eb = $<HTMLButtonElement>("#btn-export");
  const blocker = project ? exportBlocker() : null;
  eb.disabled = !project || selected.size === 0 || exporting;
  // 버전 때문에 막힌 경우: 꺼진 모양이지만 누르면 이유를 알려 준다
  eb.classList.toggle("blocked", !!blocker && !eb.disabled);
  eb.setAttribute("aria-disabled", String(eb.disabled || !!blocker));
  eb.innerHTML = `${icons.export}<span>${exporting ? "내보내는 중…" : "내보내기"}</span>${project ? `<span class="count">${selected.size}</span>` : ""}`;
  eb.title = blocker ?? `내보내기 (${MOD}E)`;
  const fp = document.getElementById("file-preview");
  if (fp) fp.textContent = filePreview();
  renderListFoot();
}

/** 목록 아래: 여러 줄을 고르면 개수와 [체크][해제], 아니면 조작 안내. */
function renderListFoot() {
  const foot = $("#sk-foot");
  if (!project || !project.skeletons.length) {
    foot.innerHTML = "";
    foot.hidden = true;
    return;
  }
  foot.hidden = false;
  if (picked.size > 1) {
    const allOn = [...picked].every((i) => selected.has(i));
    foot.innerHTML = `<span class="n"><b>${picked.size}</b>줄 선택</span><span class="grow"></span>
      <button class="btn small" data-act="check" ${exporting || allOn ? "disabled" : ""} title="선택한 줄을 모두 체크 (Space)">체크</button>
      <button class="btn small" data-act="uncheck" ${exporting || ![...picked].some((i) => selected.has(i)) ? "disabled" : ""} title="선택한 줄을 모두 해제 (Space)">해제</button>
`;
    foot.querySelector<HTMLButtonElement>('[data-act="check"]')!.onclick = () => setChecks([...picked], true);
    foot.querySelector<HTMLButtonElement>('[data-act="uncheck"]')!.onclick = () => setChecks([...picked], false);
  } else {
    foot.innerHTML = `<span class="hint-line">클릭 선택 · <kbd>${isMac ? "⌘" : "Ctrl"}</kbd>/<kbd>Shift</kbd> 여러 줄</span>`;
  }
}

function toggle(i: number, on?: boolean) {
  const want = on ?? !selected.has(i);
  if (want) selected.add(i);
  else selected.delete(i);
}

function setChecks(rows: number[], on: boolean) {
  if (exporting || !rows.length) return;
  for (const i of rows) toggle(i, on);
  paintRows();
}

/** 고른 줄의 체크를 뒤집는다: 하나라도 꺼져 있으면 모두 켜고, 모두 켜져 있으면 모두 끈다. */
function toggleChecks(rows: number[]) {
  setChecks(rows, rows.some((i) => !selected.has(i)));
}

/** 앵커부터 i 까지(보이는 순서). */
function rangeTo(i: number): number[] {
  const vis = visibleIndices();
  const a = vis.indexOf(anchorIndex >= 0 ? anchorIndex : i);
  const b = vis.indexOf(i);
  if (a < 0 || b < 0) return [i];
  return vis.slice(Math.min(a, b), Math.max(a, b) + 1);
}

function setFocus(i: number) {
  focusIndex = i;
  document.querySelector(`.sk-row[data-i="${i}"]`)?.scrollIntoView({ block: "nearest" });
}

function pickOnly(i: number) {
  picked.clear();
  picked.add(i);
  anchorIndex = i;
  setFocus(i);
}

// 마우스: 클릭 선택 · ⌘/Ctrl 더하기 · Shift 범위 · 끌어서 범위. 체크 상자는 바로 체크를 바꾼다(고른 줄이면 고른 줄 모두).
let dragging = false;
$("#sk-list").addEventListener("mousedown", (e) => {
  if (e.button !== 0) return;
  const row = (e.target as HTMLElement).closest(".sk-row") as HTMLElement | null;
  $("#sk-list").focus();
  e.preventDefault();
  if (!row) return;
  const i = Number(row.dataset.i);
  const mod = isMac ? e.metaKey : e.ctrlKey;
  if ((e.target as HTMLElement).classList.contains("tri-check")) {
    const rows = picked.has(i) && picked.size > 1 ? [...picked] : [i];
    setChecks(rows, !selected.has(i));
    if (!picked.has(i)) pickOnly(i);
    paintRows();
    return;
  }
  if (e.shiftKey) {
    const range = rangeTo(i);
    if (!mod) picked.clear();
    range.forEach((k) => picked.add(k));
    setFocus(i);
  } else if (mod) {
    if (picked.has(i)) picked.delete(i);
    else picked.add(i);
    anchorIndex = i;
    setFocus(i);
  } else {
    pickOnly(i);
    dragging = true;
  }
  paintRows();
});
$("#sk-list").addEventListener("mousemove", (e) => {
  if (!dragging || !(e.buttons & 1)) return;
  const row = (e.target as HTMLElement).closest(".sk-row") as HTMLElement | null;
  if (!row) return;
  const i = Number(row.dataset.i);
  if (i === focusIndex) return;
  picked.clear();
  rangeTo(i).forEach((k) => picked.add(k));
  setFocus(i);
  paintRows();
});
addEventListener("mouseup", () => (dragging = false));
// 체크는 mousedown 에서 이미 바꿨다. 브라우저가 click 에서 체크 상자를 한 번 더 뒤집지 못하게 막는다.
$("#sk-list").addEventListener("click", (e) => {
  if ((e.target as HTMLElement).classList.contains("tri-check")) e.preventDefault();
});
$("#sk-list").addEventListener("dblclick", (e) => {
  const row = (e.target as HTMLElement).closest(".sk-row") as HTMLElement | null;
  if (!row || (e.target as HTMLElement).classList.contains("tri-check")) return;
  toggleChecks([...picked]);
});

// 키보드: ↑↓ 이동 · Shift+↑↓ 범위 · Home/End · Space 체크 토글 · Esc 선택 줄이기
$("#sk-list").addEventListener("keydown", (e) => {
  if (!project) return;
  const vis = visibleIndices();
  if (!vis.length) return;
  const at = vis.indexOf(focusIndex);
  let next: number | undefined;
  if (e.key === "ArrowDown") next = vis[at < 0 ? 0 : Math.min(vis.length - 1, at + 1)];
  else if (e.key === "ArrowUp") next = vis[at < 0 ? 0 : Math.max(0, at - 1)];
  else if (e.key === "Home") next = vis[0];
  else if (e.key === "End") next = vis[vis.length - 1];
  else if (e.key === "PageDown" || e.key === "PageUp") {
    const page = Math.max(1, Math.floor($("#sk-list").clientHeight / 26) - 1);
    next = vis[Math.max(0, Math.min(vis.length - 1, (at < 0 ? 0 : at) + (e.key === "PageDown" ? page : -page)))];
  }
  if (next !== undefined) {
    e.preventDefault();
    if (e.shiftKey) {
      if (anchorIndex < 0) anchorIndex = focusIndex >= 0 ? focusIndex : next;
      picked.clear();
      rangeTo(next).forEach((k) => picked.add(k));
      setFocus(next);
    } else pickOnly(next);
    paintRows();
  } else if (e.key === " ") {
    e.preventDefault();
    if (!picked.size) pickOnly(focusIndex >= 0 ? focusIndex : vis[0]);
    toggleChecks([...picked]);
  } else if (e.key === "Escape" && picked.size > 1) {
    e.preventDefault();
    pickOnly(focusIndex >= 0 ? focusIndex : [...picked][0]);
    paintRows();
  }
});
$("#chk-all").addEventListener("change", (e) => {
  if (!project) return;
  setChecks(visibleIndices(), (e.target as HTMLInputElement).checked);
});
$("#btn-invert").onclick = () => {
  if (exporting) return;
  for (const i of visibleIndices()) toggle(i);
  paintRows();
};
$("#filter").addEventListener("input", (e) => {
  filter = (e.target as HTMLInputElement).value;
  const vis = new Set(visibleIndices());
  for (const i of [...picked]) if (!vis.has(i)) picked.delete(i);
  renderList();
});

// ───────────────────────────────────────── 설정

function dirname(p: string) {
  const i = Math.max(p.lastIndexOf("/"), p.lastIndexOf("\\"));
  return i >= 0 ? p.slice(0, i) : p;
}
function joinPath(a: string, b: string) {
  const sep = a.includes("\\") && !a.includes("/") ? "\\" : "/";
  return a.endsWith(sep) ? a + b : a + sep + b;
}
function outDir(): string | null {
  if (prefs.outMode === "custom" && prefs.customOut) return prefs.customOut;
  return project ? joinPath(dirname(project.path), "export") : null;
}

const pathBox = (value: string | null, placeholder: string) =>
  `<div class="field path-box selectable${value ? "" : " empty"}" title="${esc(value ?? "")}">${value ? `&lrm;${esc(value)}` : `<span class="ph">${placeholder}</span>`}</div>`;
const check = (id: string, label: string, on: boolean, enabled = true, hint = "") =>
  `<label class="check${enabled ? "" : " disabled"}"${hint ? ` title="${esc(hint)}"` : ""}><input type="checkbox" class="tri-check" id="${id}" ${on ? "checked" : ""} ${enabled ? "" : "disabled"}/>${label}</label>`;
const select = (id: string, values: [string, string][], cur: string, enabled = true, tip = "", def?: string) => selectHtml(`id="${id}"`, values, cur, enabled, tip, def);
const lbl = (text: string, tip = "") => `<span class="lbl"${tip ? ` title="${esc(tip)}"` : ""}>${text}:</span>`;

// 호버 툴팁. 한 줄로 짧게, "이게 뭔지"만 적는다.
const TIPS = {
  out: "내보낸 파일을 둘 폴더",
  version: "게임의 Spine 런타임 버전과 맞추세요",
  format: "JSON: 읽기 쉬움 · 바이너리: 작고 빠름",
  ness: "에디터로 다시 가져올 때만 쓰는 데이터. 런타임에는 필요 없습니다",
  skelExt: ".skel.bytes 는 Unity 용",
  jsonFormat: "JSON 을 쓰는 문법(따옴표 생략 정도)",
  jsonExt: "스켈레톤 JSON 파일의 확장자",
};

// ───────────────────────────────────────── 런타임 버전

/** 내보낼 수 있는 런타임: 4.0.x ~ 4.3.x (crates/spine-export `check_version` 과 같다). */
const SUPPORTED = "4.0 ~ 4.3";
type VersionKind = "follow" | "patch" | "changed" | "unsupported" | "invalid" | "none";
interface VersionState {
  kind: VersionKind;
  /** 실제로 쓸 버전(비었으면 프로젝트 버전). */
  effective: string;
  message: string;
}

function parseVersion(v: string): [number, number, number] | null {
  const m = /^(\d+)\.(\d+)\.(\d+)/.exec(v.trim());
  return m ? [Number(m[1]), Number(m[2]), Number(m[3])] : null;
}
const exportable = (v: string) => /^4\.[0-3]\.\d{1,7}$/.test(v.trim());

function versionState(): VersionState {
  const typed = options.version.trim();
  const proj = project?.formatVersion ?? "";
  const effective = typed || proj;
  if (!effective) return { kind: "none", effective, message: "프로젝트를 열면 그 프로젝트의 Spine 버전으로 내보냅니다." };
  if (!parseVersion(effective)) return { kind: "invalid", effective, message: `«${effective}»: 버전 형식이 아닙니다. 4.2.43처럼 적으세요.` };
  if (!exportable(effective)) {
    return typed
      ? { kind: "unsupported", effective, message: `${typed} 버전은 지원하지 않습니다. 지원하는 버전: ${SUPPORTED}.` }
      : { kind: "unsupported", effective, message: `이 프로젝트의 버전(${proj})은 지원하지 않습니다. 지원하는 버전: ${SUPPORTED}. 내보낼 버전을 적으세요(예: 4.2.43).` };
  }
  if (!typed) return { kind: "follow", effective, message: `프로젝트 버전(${proj})으로 내보냅니다.` };
  const p = parseVersion(proj);
  const t = parseVersion(typed)!;
  if (!p) return { kind: "follow", effective, message: `${typed} 버전으로 내보냅니다.` };
  if (t[0] === p[0] && t[1] === p[1] && t[2] === p[2]) return { kind: "follow", effective, message: `프로젝트 버전(${proj})으로 내보냅니다.` };
  if (t[0] === p[0] && t[1] === p[1]) return { kind: "patch", effective, message: `프로젝트(${proj})와 같은 ${p[0]}.${p[1]} 계열이라 차이는 거의 없습니다.` };
  const dir = t[0] * 1e6 + t[1] * 1e3 > p[0] * 1e6 + p[1] * 1e3 ? "높은" : "낮은";
  return {
    kind: "changed",
    effective,
    message: `프로젝트(${proj})보다 ${dir} ${typed} 버전으로 내보냅니다. 버전마다 기능과 동작이 달라 Spine에서 본 모습과 실제 모습이 다를 수 있습니다.`,
  };
}

/** 내보내기를 막는 이유(없으면 null). */
function exportBlocker(): string | null {
  const v = versionState();
  return v.kind === "unsupported" || v.kind === "invalid" ? v.message : null;
}

/** «하나로» 묶을 때의 atlas 이름: 프로젝트 파일 이름에서 확장자를 뺀 것(crates/spine-export `single_atlas_name`). */
function singleAtlasName(): string {
  return (project?.fileName ?? "atlas").replace(/\.[^.]*$/, "").replace(/[\\/:*?"<>|]/g, "_");
}

/** 내보낼 파일 이름 미리보기(첫 번째로 고른 스켈레톤 기준). */
function filePreview(): string {
  const o = options;
  const first = project ? [...selected].sort((x, y) => x - y)[0] : undefined;
  const raw = first !== undefined && project ? project.skeletons[first] : "스켈레톤";
  const name = raw.replace(/[\\/:*?"<>|]/g, "_");
  const files: string[] = [];
  if (o.format !== "binary") files.push(`${name}${o.jsonExt}`);
  if (o.format !== "json") files.push(`${name}${o.skelExt}`);
  if (o.atlas) {
    const scales = o.pack.scale.map((v, i) => [v, o.pack.scaleSuffix[i] ?? ""] as const).filter(([v]) => v > 0);
    const base = o.packTarget === "single" && project ? singleAtlasName() : name;
    for (const [, sfx] of scales.length ? scales : [[1, ""] as const]) files.push(`${base}${sfx}${o.pack.atlasExtension}`, `${base}${sfx}.png`);
  }
  return files.join(" · ");
}

/** 프로젝트가 없으면 목록·설정·버튼을 꺼진 상태로(보이기만, 조작은 막음). */
function applyProjectState() {
  const none = !project;
  document.documentElement.classList.toggle("no-project", none);
  for (const id of ["#settings", ".sk-group"]) {
    const el = document.querySelector<HTMLElement>(id);
    if (el) el.inert = none;
  }
  for (const id of ["#btn-settings-load", "#btn-settings-save"]) $<HTMLButtonElement>(id).disabled = none;
}

function renderSettings() {
  const o = options;
  const ness = o.nonessential === null ? "auto" : o.nonessential ? "on" : "off";
  const seg = (attr: string, items: [string, string][], cur: string, tip = "") =>
    `<div class="view-switch"${tip ? ` title="${esc(tip)}"` : ""}>${items.map(([v, l]) => `<button data-${attr}="${v}" class="${v === cur ? "on" : ""}">${l}</button>`).join("")}</div>`;
  const a = o.atlas;
  const tab = TABS.some(([k]) => k === prefs.tab) ? prefs.tab : "out";
  const vs = versionState();
  const vDot = vs.kind === "changed" ? "warn" : vs.kind === "unsupported" || vs.kind === "invalid" ? "error" : "";
  const pane = (k: SettingsTab, body: string) =>
    `<div class="tab-pane" role="tabpanel" id="pane-${k}" aria-labelledby="tab-${k}" ${k === tab ? "" : "hidden"}>${body}</div>`;
  $("#settings").innerHTML = `
    <div class="tabs" role="tablist" aria-label="내보내기 설정">
      ${TABS.map(
        ([k, label], i) =>
          `<button role="tab" id="tab-${k}" data-tab="${k}" aria-controls="pane-${k}" aria-selected="${k === tab}" tabindex="${k === tab ? 0 : -1}" class="${k === tab ? "on" : ""}" title="${label} (${MOD}${i + 1})">${label}${k === "out" && vDot ? `<i class="dot ${vDot}-dot" title="${esc(vs.message)}"></i>` : ""}${k === "atlas" && !a ? '<span class="off-tag">끔</span>' : ""}</button>`,
      ).join("")}
    </div>

    ${pane(
      "out",
      `<div class="form">
        ${lbl("폴더", TIPS.out)}
        <div class="stack">
          ${seg("out", [["beside", "프로젝트 옆 export"], ["custom", "직접 지정"]], prefs.outMode, TIPS.out)}
          <div class="row">${pathBox(outDir(), "프로젝트를 열면 정해집니다")}<button class="btn icon" id="btn-out" title="폴더 고르기">${icons.folder}</button></div>
        </div>
        ${lbl("런타임 버전", TIPS.version)}
        <div class="stack">
          <div class="row">
            <span class="combo${vs.kind === "unsupported" || vs.kind === "invalid" ? " invalid" : ""}">
              <input type="text" class="field mono" id="opt-version" spellcheck="false" autocomplete="off" title="${esc(TIPS.version)}" value="${esc(o.version)}"
                placeholder="프로젝트 버전 따름${project ? ` (${esc(project.formatVersion)})` : ""}" aria-describedby="version-note" />
              <button class="combo-btn" id="btn-version-list" tabindex="-1" title="버전 목록" aria-label="버전 목록"></button>
            </span>
            ${o.version ? `<button class="btn icon" id="btn-version-reset" title="프로젝트 버전으로 되돌리기">${icons.close}</button>` : ""}
          </div>
          <div id="version-note" class="${{ follow: "hint", none: "hint", patch: "hint", changed: "warn", unsupported: "error-note", invalid: "error-note" }[vs.kind]}">${esc(vs.message)}${vs.kind === "follow" || vs.kind === "none" ? ` 지원하는 버전: ${SUPPORTED}.` : ""}</div>
        </div>
      </div>`,
    )}

    ${pane(
      "skel",
      `<div class="form">
        ${lbl("형식", TIPS.format)}
        ${seg("fmt", [["json", "JSON"], ["binary", "바이너리"], ["both", "둘 다"]], o.format, TIPS.format)}
        ${lbl("비필수 데이터", TIPS.ness)}
        ${select("opt-ness", [["auto", "자동 (JSON 포함 · 바이너리 제외)"], ["on", "포함"], ["off", "제외"]], ness, true, TIPS.ness, "auto")}
        ${
          o.format === "json"
            ? ""
            : `${lbl("바이너리 확장자", TIPS.skelExt)}
        <div class="stack">
          ${select(
            "opt-skel-ext",
            [[".skel", ".skel"], [".skel.bytes", ".skel.bytes"]],
            o.skelExt,
            !o.unity,
            o.unity ? "Unity(spine-unity) 형식이 켜져 있어 .skel.bytes 로 고정됩니다. 팩 설정의 Options 에서 끌 수 있습니다" : TIPS.skelExt,
          )}
          ${o.unity ? `<div class="hint">Unity(spine-unity) 형식이라 .skel.bytes 로 고정됩니다.</div>` : ""}
        </div>`
        }
        ${
          o.format === "binary"
            ? ""
            : `${lbl("JSON 형식", TIPS.jsonFormat)}
        <div class="row">
          ${select(
            "opt-json-format",
            [["json", "JSON"], ["javascript", "JavaScript"], ["minimal", "Minimal"]],
            o.jsonFormat,
            true,
            "JSON: 표준 · JavaScript: 이름의 따옴표를 꼭 필요할 때만 · Minimal: 이름·값 대부분에 따옴표 없음(libgdx 같은 너그러운 파서 필요)",
            "json",
          )}
          ${check("opt-pretty", "Pretty print", o.prettyPrint, true, "사람이 읽기 좋게 줄을 나눠 씁니다(파일이 조금 커집니다)")}
        </div>
        ${lbl("JSON 확장자", TIPS.jsonExt)}
        <input type="text" class="field mono" id="opt-json-ext" spellcheck="false" autocomplete="off" title="${esc(TIPS.jsonExt)}" value="${esc(o.jsonExt)}" placeholder=".json" style="max-width:140px" />`
        }
        <span class="lbl">출력:</span>
        <div class="stack">
          <div class="row">
            ${check("opt-clean", "애니메이션 정리", o.clean, true, "값이 바뀌지 않는 불필요한 키를 지웁니다(프로젝트 파일은 바꾸지 않습니다)")}
            ${check("opt-export-all", "모두 내보내기", o.exportAll, true, "Export 체크가 꺼진 첨부도 내보냅니다(Spine 의 Export all)")}
          </div>
          <div class="row">
            ${check("opt-warnings", "경고", o.warnings, true, "내보낸 뒤 알림(건너뛴 파일, 찾지 못한 이미지 등)을 보여 줍니다")}
          </div>
        </div>
      </div>`,
    )}

    ${pane(
      "atlas",
      `<div class="form">
        <span class="lbl">텍스처 아틀라스:</span>
        <div class="stack">
          <div class="row">
            ${check("opt-atlas", "팩", a, true, "내보낼 때 텍스처 아틀라스(atlas + PNG)도 함께 만듭니다")}
            ${
              a
                ? `${select(
                    "opt-pack-source",
                    [["attachments", "어태치먼트"], ["imagefolders", "이미지 폴더"]],
                    o.packSource,
                    true,
                    "어태치먼트: 첨부가 쓰는 이미지만 · 이미지 폴더: 스켈레톤 이미지 폴더의 이미지 모두",
                    "attachments",
                  )}
                  <span class="tilde" aria-hidden="true">~</span>
                  ${select(
                    "opt-pack-target",
                    [["perskeleton", "스켈레톤별 아틀라스"], ["single", "싱글 아틀라스"]],
                    o.packTarget,
                    true,
                    "스켈레톤별: 스켈레톤 이름으로 하나씩 · 싱글: 고른 스켈레톤의 이미지를 프로젝트 파일 이름의 atlas 하나에",
                    "perskeleton",
                  )}`
                : ""
            }
          </div>
          ${
            a
              ? `<div class="row"><button class="btn" id="btn-pack" title="Texture Packer Settings"><span>팩 설정…</span></button></div>
                 <div class="pack-sum" title="${esc(packSummary(o.pack, o.unity))}">${esc(packSummary(o.pack, o.unity))}</div>
                 ${o.packSource === "imagefolders" ? `<div class="hint">이미지 폴더의 PNG 를 모두 묶습니다${o.pack.combineSubdirectories ? "" : "(하위 폴더마다 페이지를 따로 씁니다)"}. 폴더 안 pack.json 은 읽지 않습니다.</div>` : ""}`
              : ""
          }
        </div>
      </div>`,
    )}

    <div class="file-preview" title="첫 번째로 고른 스켈레톤 기준">
      <span class="cap">만들 파일</span><span class="files" id="file-preview">${esc(filePreview())}</span>
    </div>`;
  bindSettings();
  applyProjectState();
  bindTabs();
  bindSegmented($("#settings"));
  enhanceSelects($("#settings"));
}

function setTab(t: SettingsTab, focus = false) {
  if (prefs.tab === t) return;
  prefs.tab = t;
  save("sfe.prefs", prefs);
  renderSettings();
  if (focus) document.getElementById(`tab-${t}`)?.focus();
}

function bindTabs() {
  const tabs = [...document.querySelectorAll<HTMLButtonElement>("#settings [role=tab]")];
  tabs.forEach((b) => (b.onclick = () => setTab(b.dataset.tab as SettingsTab)));
  ($("#settings .tabs") as HTMLElement).onkeydown = (e) => {
    const at = tabs.findIndex((b) => b.dataset.tab === prefs.tab);
    const next = { ArrowRight: at + 1, ArrowLeft: at - 1, Home: 0, End: tabs.length - 1 }[e.key];
    if (next === undefined) return;
    e.preventDefault();
    setTab(tabs[(next + tabs.length) % tabs.length].dataset.tab as SettingsTab, true);
  };
}

function setOpt(patch: Partial<ExportOptions>) {
  options = { ...options, ...patch };
  save("sfe.options", options);
  renderSettings();
  renderSelectionUi();
}

function bindSettings() {
  const s = $("#settings");
  s.querySelectorAll<HTMLButtonElement>("[data-fmt]").forEach((b) => (b.onclick = () => setOpt({ format: b.dataset.fmt as Format })));
  s.querySelectorAll<HTMLButtonElement>("[data-out]").forEach(
    (b) =>
      (b.onclick = async () => {
        prefs.outMode = b.dataset.out as "beside" | "custom";
        if (prefs.outMode === "custom" && !prefs.customOut) await pickOut();
        save("sfe.prefs", prefs);
        renderSettings();
      }),
  );
  $<HTMLButtonElement>("#btn-out").onclick = pickOut;
  $<HTMLInputElement>("#opt-version").onchange = (e) => {
    // 틀린 값도 그대로 두고 아래 안내와 내보내기 잠금으로 알린다
    setOpt({ version: (e.target as HTMLInputElement).value.trim() });
  };
  const proj = project && exportable(project.formatVersion) ? project.formatVersion : null;
  const versionItems = [
    ...(proj ? [{ value: proj, hint: "이 프로젝트" }] : []),
    ...["4.3.26", "4.2.43", "4.1.24", "4.0.64"].filter((v) => v !== proj).map((v) => ({ value: v, hint: `${v.slice(0, 3)} 계열` })),
  ];
  attachSuggestions($<HTMLInputElement>("#opt-version"), versionItems, document.getElementById("btn-version-list"));
  const vReset = document.getElementById("btn-version-reset");
  if (vReset) vReset.onclick = () => setOpt({ version: "" });
  $<HTMLSelectElement>("#opt-ness").onchange = (e) => {
    const v = (e.target as HTMLSelectElement).value;
    setOpt({ nonessential: v === "auto" ? null : v === "on" });
  };
  $<HTMLInputElement>("#opt-atlas").onchange = (e) => setOpt({ atlas: (e.target as HTMLInputElement).checked });
  const onSel = (id: string, fn: (v: string) => void) => {
    const el = document.getElementById(id) as HTMLSelectElement | null;
    if (el) el.onchange = () => fn(el.value);
  };
  onSel("opt-pack-source", (v) => setOpt({ packSource: v as ExportOptions["packSource"] }));
  onSel("opt-pack-target", (v) => setOpt({ packTarget: v as ExportOptions["packTarget"] }));
  onSel("opt-skel-ext", (v) => setOpt({ skelExt: v }));
  onSel("opt-json-format", (v) => setOpt({ jsonFormat: v as ExportOptions["jsonFormat"] }));
  const jsonExt = document.getElementById("opt-json-ext") as HTMLInputElement | null;
  if (jsonExt)
    jsonExt.onchange = () => {
      const v = jsonExt.value.trim().replace(/[\\/:*?"<>|]/g, "");
      setOpt({ jsonExt: v ? (v.startsWith(".") ? v : `.${v}`) : ".json" });
    };
  const packBtn = document.getElementById("btn-pack");
  if (packBtn) packBtn.onclick = editPack;
  const chk = (id: string, key: keyof ExportOptions) => {
    const el = document.querySelector<HTMLInputElement>(id);
    if (el) el.onchange = () => setOpt({ [key]: el.checked });
  };
  chk("#opt-clean", "clean");
  chk("#opt-pretty", "prettyPrint");
  chk("#opt-export-all", "exportAll");
  chk("#opt-warnings", "warnings");
}

// ───────────────────────────────────────── Spine 내보내기 설정 파일

interface ImportedSettings {
  options: ExportOptions;
  output: string | null;
  /** JSON 내보내기의 Version(예: "4.1"). */
  version: string | null;
  notes: string[];
}

/** Spine 설정의 "4.2" 같은 버전 → 이 앱의 버전 칸 값. 프로젝트와 같은 계열이면 비운다(프로젝트 버전 따름). */
function versionFromSpine(v: string | null): string | null {
  const m = v ? /^(\d+)\.(\d+)(?:\.(\d+))?/.exec(v.trim()) : null;
  if (!m) return null;
  if (m[3] !== undefined) return `${m[1]}.${m[2]}.${m[3]}`;
  const p = project ? parseVersion(project.formatVersion) : null;
  if (p && p[0] === Number(m[1]) && p[1] === Number(m[2])) return "";
  return ["4.3.26", "4.2.43", "4.1.24", "4.0.64"].find((x) => x.startsWith(`${m[1]}.${m[2]}.`)) ?? `${m[1]}.${m[2]}.0`;
}

$("#btn-settings-load").onclick = async () => {
  const f = await openDialog({
    multiple: false,
    title: "Spine 내보내기 설정 불러오기",
    defaultPath: project ? dirname(project.path) : undefined,
    filters: [{ name: "Spine 내보내기 설정", extensions: ["json"] }],
  });
  if (typeof f !== "string") return;
  try {
    const got = await invoke<ImportedSettings>("load_export_settings", { path: f, options });
    // Rust 가 모르는 앱 설정(Unity 형식)은 확장자로 다시 정한다
    const o = got.options;
    const unity = o.atlas && o.pack.atlasExtension === UNITY_ATLAS_EXT && (o.format === "json" || o.skelExt === UNITY_SKEL_EXT);
    const patch: Partial<ExportOptions> = { ...o, unity, skelBeforeUnity: options.skelBeforeUnity };
    const ver = versionFromSpine(got.version);
    patch.version = ver ?? options.version;
    setOpt(patch);
    const kind = o.format === "json" ? "JSON" : "바이너리";
    const acts = got.output
      ? [
          {
            label: "출력 폴더도 적용",
            run: () => {
              prefs.customOut = got.output!;
              prefs.outMode = "custom";
              save("sfe.prefs", prefs);
              renderSettings();
            },
          },
        ]
      : undefined;
    const sub = [got.output ? `설정에 적힌 출력 폴더: ${got.output}` : "", ...got.notes].filter(Boolean).join("\n");
    toast(got.notes.length ? "warning" : "success", `Spine ${kind} 내보내기 설정을 불러왔습니다`, sub || undefined, acts, got.output || got.notes.length ? 12000 : 4200);
  } catch (e) {
    toast("error", "설정을 불러올 수 없습니다", String(e), undefined, 8000);
  }
};

$("#btn-settings-save").onclick = async () => {
  const kind = options.format === "json" ? "json" : "binary";
  const f = await saveDialog({
    title: "Spine 내보내기 설정 저장",
    defaultPath: project ? joinPath(dirname(project.path), `${kind}.export.json`) : `${kind}.export.json`,
    filters: [{ name: "Spine 내보내기 설정", extensions: ["json"] }],
  });
  if (typeof f !== "string") return;
  try {
    await invoke("save_export_settings", { path: f, options, input: project?.path ?? null, output: outDir() });
    const both = options.format === "both" ? " 형식이 «둘 다»라 바이너리 설정으로 저장했습니다(Spine 설정 파일은 한 형식만 담습니다)." : "";
    toast("success", "Spine 내보내기 설정을 저장했습니다", `${f}${both}`, undefined, both ? 9000 : 4200);
  } catch (e) {
    toast("error", "설정을 저장할 수 없습니다", String(e), undefined, 8000);
  }
};

let packOpen = false;
async function editPack() {
  if (packOpen) return;
  packOpen = true;
  try {
    const next = await openPackerDialog(options.pack, options.unity, DEFAULT_PACK, (kind, title, sub) => toast(kind, title, sub));
    if (!next) return;
    const patch: Partial<ExportOptions> = { pack: next.pack, unity: next.unity };
    if (next.unity && !options.unity) {
      // 켜기: 지금 확장자를 기억하고 spine-unity 형식으로
      if (options.skelExt !== UNITY_SKEL_EXT) patch.skelBeforeUnity = options.skelExt;
      patch.skelExt = UNITY_SKEL_EXT;
    } else if (!next.unity && options.unity) {
      patch.skelExt = options.skelBeforeUnity || ".skel";
    }
    setOpt(patch);
  } finally {
    packOpen = false;
  }
}

async function pickOut() {
  const dir = await openDialog({ directory: true, title: "출력 폴더", defaultPath: outDir() ?? undefined });
  if (typeof dir === "string") {
    prefs.customOut = dir;
    prefs.outMode = "custom";
    save("sfe.prefs", prefs);
    renderSettings();
  }
}

// ───────────────────────────────────────── 상태 줄

function setStatus(text: string, progress: number | null | undefined = undefined) {
  $("#foot-msg").textContent = text;
  const bar = $("#progress");
  bar.classList.toggle("indeterminate", progress === null);
  (bar.firstElementChild as HTMLElement).style.width = typeof progress === "number" ? `${Math.round(progress * 100)}%` : "0";
}
function idleStatus() {
  setStatus(project ? `포맷 ${project.formatVersion} · 스켈레톤 ${project.skeletons.length}개 · ${project.loadMs}ms` : "열린 프로젝트 없음");
}

// ───────────────────────────────────────── 열기

async function openProject(path: string) {
  if (loading || exporting) return;
  loading = true;
  const name = path.split(/[\\/]/).pop() ?? path;
  setStatus(`${name} 읽는 중…`, null);
  try {
    const info = await invoke<ProjectInfo>("open_project", { path });
    project = info;
    selected.clear();
    exportState.clear();
    filter = "";
    $<HTMLInputElement>("#filter").value = "";
    info.skeletons.forEach((_, i) => selected.add(i));
    picked.clear();
    focusIndex = -1;
    anchorIndex = -1;
    if (info.skeletons.length) pickOnly(0);
    const box = $("#proj-path");
    box.innerHTML = `&lrm;${esc(info.path)}`;
    box.classList.remove("empty");
    box.title = info.path;
    $<HTMLInputElement>("#filter").disabled = false;
    $<HTMLButtonElement>("#btn-reload").disabled = false;
    $<HTMLButtonElement>("#btn-reveal").disabled = false;
    void getCurrentWindow().setTitle(`${info.fileName} — Spine Faster Export`);
    renderList();
    renderSettings();
    if (info.partialError) toast("warning", "스켈레톤 뒤 편집기 상태 일부를 읽지 못했습니다", `내보내기에는 영향이 없습니다: ${info.partialError}`, undefined, 7000);
  } catch (e) {
    toast("error", `${name} 을(를) 열 수 없습니다`, String(e), undefined, 8000);
  } finally {
    loading = false;
    idleStatus();
  }
}

async function pickFile() {
  const f = await openDialog({ multiple: false, title: "Spine 프로젝트 열기", filters: [{ name: "Spine 프로젝트", extensions: ["spine"] }] });
  if (typeof f === "string") await openProject(f);
}

$("#btn-open").onclick = pickFile;
$("#btn-reload").onclick = () => project && openProject(project.path);
$("#btn-reveal").onclick = async () => {
  const d = outDir();
  if (!d) return;
  try {
    await revealItemInDir(d);
  } catch {
    toast("info", "출력 폴더가 아직 없습니다", "내보내면 만들어집니다.");
  }
};

// 끌어 놓기
void getCurrentWebview().onDragDropEvent((ev) => {
  const p = ev.payload;
  const drop = $("#drop");
  if (p.type === "enter") {
    const f = p.paths.find((x) => x.toLowerCase().endsWith(".spine"));
    $("#drop-card").classList.toggle("bad", !f);
    $("#drop-card").innerHTML = f
      ? `<div class="t">놓으면 엽니다</div><div class="s">${esc(f.split(/[\\/]/).pop()!)}</div>`
      : `<div class="t">.spine 파일이 아닙니다</div><div class="s">Spine 프로젝트 파일만 열 수 있습니다</div>`;
    drop.classList.add("on");
  } else if (p.type === "leave") {
    drop.classList.remove("on");
  } else if (p.type === "drop") {
    drop.classList.remove("on");
    const f = p.paths.find((x) => x.toLowerCase().endsWith(".spine"));
    if (f) void openProject(f);
    else toast("warning", ".spine 파일이 아닙니다", "Spine 프로젝트 파일(.spine)을 끌어 놓으세요.");
  }
});

// 파인더·탐색기에서 넘어온 파일
void listen<string[]>("open-files", (e) => {
  void invoke("take_pending_open");
  const f = e.payload[e.payload.length - 1];
  if (f) void openProject(f);
});

// ───────────────────────────────────────── 내보내기

/** 예/아니오 대화상자(결과 창과 같은 모양). Enter=확인, Esc·바깥=취소. */
function confirmDialog(title: string, sub: string, items: string[], okLabel: string): Promise<boolean> {
  return new Promise((resolve) => {
    const back = document.createElement("div");
    back.className = "modal-backdrop";
    const more = items.length > 12 ? `<div class="res-more">외 ${items.length - 12}개</div>` : "";
    back.innerHTML = `<div class="modal" role="alertdialog" aria-modal="true" aria-label="${esc(title)}">
      <div class="m-head">${esc(title)}<div class="sub">${esc(sub)}</div></div>
      ${items.length ? `<div class="m-body"><div class="file-list">${items.slice(0, 12).map((f) => `<div>${esc(f)}</div>`).join("")}${more}</div></div>` : ""}
      <div class="m-foot"><button class="btn" data-act="no">취소</button><button class="btn primary" data-act="yes">${esc(okLabel)}</button></div></div>`;
    document.body.appendChild(back);
    const done = (v: boolean) => {
      back.remove();
      resolve(v);
    };
    back.querySelector<HTMLElement>('[data-act="yes"]')!.onclick = () => done(true);
    back.querySelector<HTMLElement>('[data-act="no"]')!.onclick = () => done(false);
    back.addEventListener("mousedown", (e) => e.target === back && done(false));
    back.addEventListener("keydown", (e) => {
      if (e.key === "Escape") {
        e.preventDefault();
        done(false);
      }
    });
    back.querySelector<HTMLElement>('[data-act="yes"]')!.focus();
  });
}

function showResults(rows: ExportProgress[], sum: ExportSummary) {
  const back = document.createElement("div");
  back.className = "modal-backdrop";
  back.innerHTML = `<div class="modal" role="dialog">
    <div class="m-head">내보내기 결과<div class="sub">${sum.succeeded}개 성공 · ${sum.failed}개 실패 · ${(sum.elapsedMs / 1000).toFixed(2)}초</div></div>
    <div class="m-body">${rows
      .sort((a, b) => Number(a.ok) - Number(b.ok) || (a.index ?? Infinity) - (b.index ?? Infinity))
      .map(
        (r) => `<div class="res-row">${(r.ok ? icons.check : icons.error).replace("<svg", `<svg class="ic ${r.ok ? "ok" : "fail"}"`)}
        <div style="min-width:0;flex:1"><div class="nm">${esc(r.skeleton)}</div>
        ${r.ok ? `<div class="files">${r.files.map(esc).join(" · ")}</div>` : `<div class="msg">${esc(r.message ?? "")}</div>`}
        ${r.notes.map((n) => `<div class="note">${esc(n)}</div>`).join("")}</div></div>`,
      )
      .join("")}</div>
    <div class="m-foot"><button class="btn" id="m-reveal">${icons.folder}<span>폴더 열기</span></button><button class="btn primary" id="m-close">닫기</button></div></div>`;
  document.body.appendChild(back);
  const close = () => back.remove();
  (back.querySelector("#m-close") as HTMLElement).onclick = close;
  (back.querySelector("#m-reveal") as HTMLElement).onclick = () => void revealItemInDir(sum.outDir).catch(() => {});
  back.addEventListener("mousedown", (e) => e.target === back && close());
  back.addEventListener("keydown", (e) => e.key === "Escape" && close());
  (back.querySelector("#m-close") as HTMLElement).focus();
}

async function runExport() {
  if (!project || exporting || !selected.size) return;
  const blocker = exportBlocker();
  if (blocker) {
    setTab("out");
    toast("error", "지원하지 않는 버전입니다", blocker);
    return;
  }
  const dir = outDir();
  if (!dir) return;
  const indices = [...selected].sort((a, b) => a - b);
  // 이미 있는 파일을 덮어쓰면 먼저 묻는다(프로젝트 옆 export 에 예전 결과가 있을 때가 많다)
  if (confirming) return;
  confirming = true;
  try {
    const existing = (await invoke<string[] | null>("existing_outputs", { indices, outDir: dir, options }).catch(() => null)) ?? [];
    if (existing.length) {
      const ok = await confirmDialog(`파일 ${existing.length}개를 덮어씁니다`, "출력 폴더에 같은 이름의 파일이 이미 있습니다.", existing, "덮어쓰고 내보내기");
      if (!ok) return;
    }
  } finally {
    confirming = false;
  }
  exporting = true;
  for (const i of indices) exportState.set(i, "run");
  renderList();
  setStatus(`내보내는 중… 0/${indices.length}`, 0);
  const rows: ExportProgress[] = [];
  const un = await listen<ExportProgress>("export-progress", (e) => {
    const p = e.payload;
    rows.push(p);
    if (p.index !== null) exportState.set(p.index, p.ok ? "ok" : "fail");
    setStatus(`내보내는 중… ${p.done}/${p.total}`, p.done / p.total);
    const row = document.querySelector(`.sk-row[data-i="${p.index}"] .state`);
    if (row) row.outerHTML = p.ok ? `<span class="state ok" title="내보냄">${icons.check}</span>` : `<span class="state fail" title="실패">${icons.error}</span>`;
  });
  let last = "";
  try {
    const sum = await invoke<ExportSummary>("export_skeletons", { indices, outDir: dir, options });
    const secs = (sum.elapsedMs / 1000).toFixed(2);
    const notes = rows.flatMap((r) => r.notes);
    // 끝나면 토스트만: 누르면 Finder 에서 출력 폴더를(내보낸 파일을 골라 둔 채) 연다
    const firstFile = rows.find((r) => r.ok && r.files.length)?.files[0];
    const reveal = () => void revealItemInDir(firstFile ? joinPath(dir, firstFile) : dir).catch(() => toast("info", "출력 폴더를 열 수 없습니다", dir));
    const fine = sum.failed === 0 && notes.length === 0;
    const title =
      sum.failed === 0
        ? "내보냈습니다"
        : sum.succeeded === 0
          ? "내보내지 못했습니다"
          : `${sum.failed}개를 내보내지 못했습니다`;
    const sub = sum.failed === 0 && notes.length ? `알림 ${notes.length}개` : undefined;
    toast(
      fine ? "success" : sum.succeeded === 0 ? "error" : "warning",
      title,
      sub,
      fine ? undefined : [{ label: "자세히", run: () => showResults(rows, sum) }],
      fine ? 5000 : 10000,
      sum.succeeded > 0 ? { onClick: reveal, tip: "눌러서 Finder 에서 열기" } : {},
    );
    last = `마지막 내보내기: ${sum.succeeded}개 성공 · ${sum.failed}개 실패 · ${secs}초`;
  } catch (e) {
    for (const i of indices) if (exportState.get(i) === "run") exportState.delete(i);
    toast("error", "내보내기를 시작할 수 없습니다", String(e), undefined, 8000);
  } finally {
    un();
    exporting = false;
    renderList();
    if (last) setStatus(last);
    else idleStatus();
  }
}
$("#btn-export").onclick = runExport;

// ───────────────────────────────────────── 단축키·테마

addEventListener("keydown", (e) => {
  if (document.querySelector(".modal-backdrop")) return; // 대화상자가 떠 있을 때는 대화상자 키만
  const mod = isMac ? e.metaKey : e.ctrlKey;
  const typing = (e.target as HTMLElement).tagName === "INPUT" && (e.target as HTMLInputElement).type === "text";
  const k = e.key.toLowerCase();
  if (mod && k === "o") {
    e.preventDefault();
    void pickFile();
  } else if (mod && k === "e") {
    e.preventDefault();
    void runExport();
  } else if (mod && k === "r") {
    e.preventDefault();
    if (project) void openProject(project.path);
  } else if (mod && k === "a" && !typing && project) {
    // 보이는 줄을 모두 고른다(체크는 Space)
    e.preventDefault();
    const vis = visibleIndices();
    picked.clear();
    vis.forEach((i) => picked.add(i));
    if (focusIndex < 0 && vis.length) focusIndex = vis[0];
    anchorIndex = vis[0] ?? -1;
    paintRows();
    $("#sk-list").focus();
  } else if (mod && k === "f" && project) {
    e.preventDefault();
    $<HTMLInputElement>("#filter").focus();
  } else if (mod && ["1", "2", "3"].includes(e.key)) {
    e.preventDefault();
    setTab(TABS[Number(e.key) - 1][0], true);
  } else if (e.key === "Escape" && typing) {
    $("#sk-list").focus();
  }
});

const themeNames = { system: "시스템", dark: "다크", light: "라이트" } as const;
function renderTheme() {
  $("#theme-name").textContent = themeNames[prefs.theme];
}
$("#btn-theme").onclick = () => {
  prefs.theme = prefs.theme === "system" ? "dark" : prefs.theme === "dark" ? "light" : "system";
  save("sfe.prefs", prefs);
  applyTheme();
  renderTheme();
};
renderTheme();

// ───────────────────────────────────────── 시작

renderList();
renderSettings();
idleStatus();
// 앱을 켤 때 함께 넘어온 파일(파인더 «다음으로 열기», 탐색기 연결)
void invoke<string[]>("take_pending_open").then((paths) => {
  const f = paths[paths.length - 1];
  if (f) void openProject(f);
});
