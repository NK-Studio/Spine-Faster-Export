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
import { installTooltips } from "./tooltip";
import { attachSuggestions, bindSegmented, enhanceSelects, selectHtml } from "./controls";
import { UNITY_ATLAS_EXT, UNITY_SKEL_EXT, openPackerDialog, packSummary, type PackSettings } from "./packer";
import { LANG, resolveLang, t, type LangPref } from "./i18n";
import { openSettingsDialog } from "./settings";

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
  /** 비필수 데이터(Spine «Nonessential data»). JSON·바이너리 모두에 쓴다. */
  nonessential: boolean;
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
  ["skel", t("스켈레톤 데이터")],
  ["atlas", t("텍스처 아틀라스")],
  ["out", t("출력")],
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
  legacyOutput: false,
};

const DEFAULTS: ExportOptions = {
  version: "",
  format: "json",
  nonessential: true,
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
  packTarget: "single",
  unity: false,
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
  /** 화면 언어(src/i18n.ts 가 시작할 때 직접 읽는다). */
  lang: "system" as LangPref,
  outMode: "beside" as "beside" | "custom",
  customOut: "",
  /** 설정 탭 */
  tab: "skel" as SettingsTab,
});
let options: ExportOptions = load("sfe.options", DEFAULTS);
options.pack = { ...DEFAULT_PACK, ...(options.pack ?? {}) };
// 예전 버전이 저장한 이미지 폴더 지정은 더 쓰지 않는다(프로젝트에 적힌 경로만 쓴다).
delete (options as unknown as { imagesDir?: unknown }).imagesDir;
// 예전 «자동»(null: JSON 은 포함, 바이너리는 제외)은 지금 형식에 맞춰 켬/끔 하나로 정한다
if (typeof options.nonessential !== "boolean") options.nonessential = options.format !== "binary";
if (typeof options.unity !== "boolean") options.unity = options.pack.atlasExtension === UNITY_ATLAS_EXT && options.skelExt === UNITY_SKEL_EXT;
// 확장자·JSON 형식은 고르지 않는다: JSON 은 표준 .json, 바이너리는 Unity 형식이면 .skel.bytes 아니면 .skel
options.jsonFormat = "json";
options.jsonExt = ".json";
options.skelExt = options.unity ? UNITY_SKEL_EXT : ".skel";
delete (options as unknown as { skelBeforeUnity?: unknown }).skelBeforeUnity;

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
// 맥은 시스템 UI 글꼴(SF Pro · Apple SD Gothic Neo)을 쓴다(styles.css `:root.mac`)
document.documentElement.classList.toggle("mac", isMac);
const MOD = isMac ? "⌘" : "Ctrl+";
document.documentElement.dataset.platform = isMac ? "mac" : "win";
installTooltips();
const media = matchMedia("(prefers-color-scheme: dark)");
function applyTheme() {
  const mode = prefs.theme === "system" ? (media.matches ? "dark" : "light") : prefs.theme;
  document.documentElement.dataset.theme = mode;
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
    <div class="g-title">${t("입력")}</div>
    <div class="input-row">
      <span class="lbl">${t("프로젝트")}:</span>
      <input type="text" class="field mono proj-path" id="proj-path" spellcheck="false" autocomplete="off" placeholder="${t(".spine 파일을 이 창에 끌어 놓거나 열기를 누르세요")}" />
      <button class="btn" id="btn-open" title="${t("열기 ({mod}O)", { mod: MOD })}">${icons.open}<span>${t("열기…")}</span></button>
      <button class="btn icon" id="btn-reload" title="${t("다시 읽기 ({mod}R)", { mod: MOD })}" disabled>${icons.reload}</button>
    </div>
  </section>

  <div class="body">
    <section class="group sk-group">
      <div class="g-title">
        <label class="check"><input type="checkbox" class="tri-check" id="chk-all" title="${t("모두 체크/해제")}" disabled />${t("스켈레톤")}</label>
        <span class="count" id="sel-count"></span>
        <span class="grow"></span>
        <button class="icon-btn" id="btn-invert" title="${t("체크 뒤집기")}" disabled>
          <svg viewBox="0 0 24 24" fill="currentColor"><path d="M7 17h10V7Zm-2 4q-.825 0-1.412-.587Q3 19.825 3 19V5q0-.825.588-1.413Q4.175 3 5 3h14q.825 0 1.413.587Q21 4.175 21 5v14q0 .825-.587 1.413Q19.825 21 19 21Zm0-2h14V5H5v14Z"/></svg>
        </button>
      </div>
      <label class="field-with-icon" id="filter-box">${icons.search}<input type="text" id="filter" placeholder="${t("스켈레톤 검색 ({mod}F)", { mod: MOD })}" spellcheck="false" disabled /></label>
      <div class="sk-list" id="sk-list" tabindex="0" role="listbox" aria-multiselectable="true" aria-label="${t("스켈레톤")}"></div>
      <div class="sk-foot" id="sk-foot"></div>
    </section>

    <div class="settings" id="settings"></div>
  </div>

  <div class="actions">
    <button class="btn" id="btn-settings-load" title="${t("Spine 의 내보내기 대화상자에서 저장(Save)한 설정 JSON 을 불러옵니다")}">${t("설정 불러오기…")}</button>
    <button class="btn" id="btn-settings-save" title="${t("지금 설정을 Spine 내보내기 설정 JSON 으로 저장합니다(Spine 의 Load · CLI --export 로 읽을 수 있습니다)")}">${t("설정 저장…")}</button>
    <span class="grow"></span>
    <button class="btn" id="btn-reveal" disabled>${icons.folder}<span>${t("출력 폴더 열기")}</span></button>
    <button class="btn primary" id="btn-export" disabled></button>
  </div>
</div>

<footer class="footer">
  <span class="grow" id="foot-msg"></span>
  <span class="ver">v0.2.1</span>
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
    <button class="icon-btn x" title="${t("닫기")}">${icons.close}</button>`;
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
    let timer = setTimeout(close, ms);
    el.onmouseenter = () => clearTimeout(timer);
    el.onmouseleave = () => (timer = setTimeout(close, 3000));
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
  const stHtml = st === "ok" ? `<span class="state ok" title="${t("내보냄")}">${icons.check}</span>` : st === "fail" ? `<span class="state fail" title="${t("실패")}">${icons.error}</span>` : st === "run" ? `<span class="state run"></span>` : "";
  return `<div class="sk-row" role="option" data-i="${i}" title="${esc(name)}">
    <input type="checkbox" class="tri-check" tabindex="-1" title="${t("내보내기 체크")}" />
    <span class="name">${label}</span>${stHtml}</div>`;
}

function renderList() {
  const list = $("#sk-list");
  if (!project) {
    list.innerHTML = `<div class="empty-note">${icons.open.replace("<svg", '<svg class="hero"')}<div>${t(".spine 파일을 끌어 놓으세요")}</div><div class="sub">${t("파인더·탐색기의 «다음으로 열기»도 됩니다")}</div></div>`;
  } else {
    const vis = visibleIndices();
    if (!vis.length) list.innerHTML = `<div class="empty-note">${project.skeletons.length ? t("맞는 스켈레톤이 없습니다.") : t("스켈레톤이 없는 프로젝트입니다.")}</div>`;
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
  eb.innerHTML = `${icons.export}<span>${exporting ? t("내보내는 중…") : t("내보내기")}</span>${project ? `<span class="count">${selected.size}</span>` : ""}`;
  eb.title = blocker ?? t("내보내기 ({mod}E)", { mod: MOD });
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
    foot.innerHTML = `<span class="n">${t("<b>{n}</b>줄 선택", { n: picked.size })}</span><span class="grow"></span>
      <button class="btn small" data-act="check" ${exporting || allOn ? "disabled" : ""} title="${t("선택한 줄을 모두 체크 (Space)")}">${t("체크")}</button>
      <button class="btn small" data-act="uncheck" ${exporting || ![...picked].some((i) => selected.has(i)) ? "disabled" : ""} title="${t("선택한 줄을 모두 해제 (Space)")}">${t("해제")}</button>
`;
    foot.querySelector<HTMLButtonElement>('[data-act="check"]')!.onclick = () => setChecks([...picked], true);
    foot.querySelector<HTMLButtonElement>('[data-act="uncheck"]')!.onclick = () => setChecks([...picked], false);
  } else {
    foot.innerHTML = `<span class="hint-line">${t("클릭 선택 · <kbd>{mod}</kbd>/<kbd>Shift</kbd> 여러 줄", { mod: isMac ? "⌘" : "Ctrl" })}</span>`;
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
  out: t("내보낸 파일을 둘 폴더"),
  version: t("게임의 Spine 런타임 버전과 맞추세요"),
  format: t("JSON: 읽기 쉬움 · 바이너리: 작고 빠름"),
  ness: t("에디터로 다시 가져올 때만 쓰는 데이터. 런타임에는 필요 없습니다"),
};

// ───────────────────────────────────────── 런타임 버전

/** 내보낼 수 있는 런타임: 3.8.x, 4.0.x ~ 4.3.x (crates/spine-export `check_version` 과 같다). */
const SUPPORTED = "3.8, 4.0 ~ 4.3";
/** 버전 목록에 보이는 버전(계열마다 마지막 것). 3.8 은 Spine 3.8 의 마지막 버전. */
const KNOWN_VERSIONS = ["4.3.26", "4.2.43", "4.1.24", "4.0.64", "3.8.99"];
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
const exportable = (v: string) => /^(4\.[0-3]|3\.8)\.\d{1,7}$/.test(v.trim());
/**
 * 3.8 로 내보내는가. Spine 3.8 의 내보내기에는 Pack target 이 없어 atlas 를 늘 스켈레톤마다 만든다
 * (프로젝트가 어느 버전이든: crates/spine-export `per_skeleton_only`).
 */
const is38 = (v: string) => /^3\.8\./.test(v.trim());
/** 실제로 쓸 Pack target(crates/spine-export `options_for` 와 같다). */
const packTargetOf = (o: ExportOptions): ExportOptions["packTarget"] => (is38(versionState().effective) ? "perskeleton" : o.packTarget);

function versionState(): VersionState {
  const typed = options.version.trim();
  const proj = project?.formatVersion ?? "";
  const effective = typed || proj;
  if (!effective) return { kind: "none", effective, message: t("프로젝트를 열면 그 프로젝트의 Spine 버전으로 내보냅니다.") };
  if (!parseVersion(effective)) return { kind: "invalid", effective, message: t("«{v}»: 버전 형식이 아닙니다. 4.2.43처럼 적으세요.", { v: effective }) };
  if (!exportable(effective)) {
    return typed
      ? { kind: "unsupported", effective, message: t("{v} 버전은 지원하지 않습니다. 지원하는 버전: {sup}.", { v: typed, sup: SUPPORTED }) }
      : { kind: "unsupported", effective, message: t("이 프로젝트의 버전({proj})은 지원하지 않습니다. 지원하는 버전: {sup}. 내보낼 버전을 적으세요(예: 4.2.43).", { proj, sup: SUPPORTED }) };
  }
  if (!typed) return { kind: "follow", effective, message: t("프로젝트 버전({proj})으로 내보냅니다.", { proj }) };
  const p = parseVersion(proj);
  const tv = parseVersion(typed)!;
  if (!p) return { kind: "follow", effective, message: t("{v} 버전으로 내보냅니다.", { v: typed }) };
  if (tv[0] === p[0] && tv[1] === p[1] && tv[2] === p[2]) return { kind: "follow", effective, message: t("프로젝트 버전({proj})으로 내보냅니다.", { proj }) };
  if (tv[0] === p[0] && tv[1] === p[1]) return { kind: "patch", effective, message: t("프로젝트({proj})와 같은 {mm} 계열이라 차이는 거의 없습니다.", { proj, mm: `${p[0]}.${p[1]}` }) };
  const newer = tv[0] * 1e6 + tv[1] * 1e3 > p[0] * 1e6 + p[1] * 1e3;
  return {
    kind: "changed",
    effective,
    message: newer
      ? t("프로젝트({proj})보다 높은 {v} 버전으로 내보냅니다. 버전마다 기능과 동작이 달라 Spine에서 본 모습과 실제 모습이 다를 수 있습니다.", { proj, v: typed })
      : t("프로젝트({proj})보다 낮은 {v} 버전으로 내보냅니다. 버전마다 기능과 동작이 달라 Spine에서 본 모습과 실제 모습이 다를 수 있습니다.", { proj, v: typed }),
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
  const raw = first !== undefined && project ? project.skeletons[first] : t("스켈레톤|file");
  const name = raw.replace(/[\\/:*?"<>|]/g, "_");
  const files: string[] = [];
  if (o.format !== "binary") files.push(`${name}${o.jsonExt}`);
  if (o.format !== "json") files.push(`${name}${o.skelExt}`);
  if (o.atlas) {
    const scales = o.pack.scale.map((v, i) => [v, o.pack.scaleSuffix[i] ?? ""] as const).filter(([v]) => v > 0);
    const base = packTargetOf(o) === "single" && project ? singleAtlasName() : name;
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
  const seg = (attr: string, items: [string, string][], cur: string, tip = "") =>
    `<div class="view-switch"${tip ? ` title="${esc(tip)}"` : ""}>${items.map(([v, l]) => `<button data-${attr}="${v}" class="${v === cur ? "on" : ""}">${l}</button>`).join("")}</div>`;
  const a = o.atlas;
  const tab = TABS.some(([k]) => k === prefs.tab) ? prefs.tab : "out";
  const vs = versionState();
  const vDot = vs.kind === "changed" ? "warn" : vs.kind === "unsupported" || vs.kind === "invalid" ? "error" : "";
  const pane = (k: SettingsTab, body: string) =>
    `<div class="tab-pane" role="tabpanel" id="pane-${k}" aria-labelledby="tab-${k}" ${k === tab ? "" : "hidden"}>${body}</div>`;
  $("#settings").innerHTML = `
    <div class="tabs" role="tablist" aria-label="${t("내보내기 설정")}">
      ${TABS.map(
        ([k, label], i) =>
          `<button role="tab" id="tab-${k}" data-tab="${k}" aria-controls="pane-${k}" aria-selected="${k === tab}" tabindex="${k === tab ? 0 : -1}" class="${k === tab ? "on" : ""}" title="${label} (${MOD}${i + 1})">${label}${k === "out" && vDot ? `<i class="dot ${vDot}-dot" title="${esc(vs.message)}"></i>` : ""}</button>`,
      ).join("")}
    </div>

    ${pane(
      "out",
      `<div class="form">
        ${lbl(t("폴더"), TIPS.out)}
        <div class="stack">
          ${seg("out", [["beside", t("프로젝트 옆 export")], ["custom", t("직접 지정")]], prefs.outMode, TIPS.out)}
          <div class="row">${
            prefs.outMode === "custom"
              ? `<input type="text" class="field mono out-path" id="opt-out" spellcheck="false" autocomplete="off" value="${esc(prefs.customOut)}" placeholder="${t("출력 폴더 경로")}" />`
              : pathBox(outDir(), t("프로젝트를 열면 정해집니다"))
          }<button class="btn icon" id="btn-out" title="${t("폴더 고르기")}">${icons.folder}</button></div>
        </div>
        ${lbl(t("런타임 버전"), TIPS.version)}
        <div class="stack">
          <div class="row">
            <span class="combo${vs.kind === "unsupported" || vs.kind === "invalid" ? " invalid" : ""}">
              <input type="text" class="field mono" id="opt-version" spellcheck="false" autocomplete="off" title="${esc(TIPS.version)}" value="${esc(o.version)}"
                placeholder="${t("프로젝트 버전 따름")}${project ? ` (${esc(project.formatVersion)})` : ""}" aria-describedby="version-note" />
              <button class="combo-btn" id="btn-version-list" tabindex="-1" title="${t("버전 목록")}" aria-label="${t("버전 목록")}"></button>
            </span>
            ${o.version ? `<button class="btn icon" id="btn-version-reset" title="${t("프로젝트 버전으로 되돌리기")}">${icons.close}</button>` : ""}
          </div>
          <div id="version-note" class="${{ follow: "hint", none: "hint", patch: "hint", changed: "warn", unsupported: "error-note", invalid: "error-note" }[vs.kind]}">${esc(vs.message)}${vs.kind === "follow" || vs.kind === "none" ? t(" 지원하는 버전: {sup}.", { sup: SUPPORTED }) : ""}</div>
        </div>
      </div>`,
    )}

    ${pane(
      "skel",
      `<div class="form">
        ${lbl(t("형식"), TIPS.format)}
        ${seg("fmt", [["json", "JSON"], ["binary", t("바이너리")], ["both", t("둘 다")]], o.format, TIPS.format)}
        ${lbl(t("출력"))}
        <div class="stack">
          <div class="row">
            ${check("opt-ness", t("비필수 데이터"), o.nonessential, true, TIPS.ness)}
            ${check("opt-clean", t("애니메이션 정리"), o.clean, true, t("값이 바뀌지 않는 불필요한 키를 지웁니다(프로젝트 파일은 바꾸지 않습니다)"))}
          </div>
          <div class="row">
            ${check("opt-warnings", t("경고"), o.warnings, true, t("내보낸 뒤 알림(건너뛴 파일, 찾지 못한 이미지 등)을 보여 줍니다"))}
            ${check("opt-export-all", t("모두 내보내기"), o.exportAll, true, t("Export 체크가 꺼진 첨부도 내보냅니다(Spine 의 Export all)"))}
          </div>
          ${o.format === "binary" ? "" : `<div class="row">${check("opt-pretty", "Pretty print", o.prettyPrint, true, t("사람이 읽기 좋게 줄을 나눠 씁니다(파일이 조금 커집니다)"))}</div>`}
        </div>
      </div>`,
    )}

    ${pane(
      "atlas",
      `<div class="form">
        ${lbl(t("텍스처 아틀라스"))}
        <div class="stack">
          <div class="row">
            ${check("opt-atlas", t("팩"), a, true, t("내보낼 때 텍스처 아틀라스(atlas + PNG)도 함께 만듭니다"))}
            ${
              a
                ? `${select(
                    "opt-pack-source",
                    [["attachments", t("어태치먼트")], ["imagefolders", t("이미지 폴더")]],
                    o.packSource,
                    true,
                    t("어태치먼트: 첨부가 쓰는 이미지만 · 이미지 폴더: 스켈레톤 이미지 폴더의 이미지 모두"),
                    DEFAULTS.packSource,
                  )}
                  ${
                    is38(vs.effective)
                      ? "" // Spine 3.8 의 내보내기에는 이 선택이 없다: 늘 스켈레톤별
                      : `<span class="tilde" aria-hidden="true">~</span>
                  ${select(
                    "opt-pack-target",
                    [["perskeleton", t("스켈레톤별 아틀라스")], ["single", t("싱글 아틀라스")]],
                    o.packTarget,
                    true,
                    t("스켈레톤별: 스켈레톤 이름으로 하나씩 · 싱글: 고른 스켈레톤의 이미지를 프로젝트 파일 이름의 atlas 하나에"),
                    DEFAULTS.packTarget,
                  )}`
                  }`
                : ""
            }
          </div>
          ${
            a
              ? `<div class="row"><button class="btn" id="btn-pack" title="Texture Packer Settings"><span>${t("팩 설정…")}</span></button></div>
                 <div class="pack-sum" title="${esc(packSummary(o.pack, o.unity))}">${esc(packSummary(o.pack, o.unity))}</div>
                 ${o.packSource === "imagefolders" ? `<div class="hint">${t("이미지 폴더의 PNG 를 모두 묶습니다{sub}. 폴더 안 pack.json 은 Spine 처럼 그 폴더의 설정으로 읽습니다.", { sub: o.pack.combineSubdirectories ? "" : t("(하위 폴더마다 페이지를 따로 씁니다)") })}</div>` : ""}`
              : ""
          }
        </div>
      </div>`,
    )}

    <div class="file-preview" title="${t("첫 번째로 고른 스켈레톤 기준")}">
      <span class="cap">${t("만들 파일")}</span><span class="files" id="file-preview">${esc(filePreview())}</span>
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
  const outInput = document.getElementById("opt-out") as HTMLInputElement | null;
  if (outInput) {
    // 직접 적은 경로(Enter 나 칸을 벗어나면 적용)
    outInput.onchange = () => {
      prefs.customOut = outInput.value.trim();
      save("sfe.prefs", prefs);
      renderSettings();
    };
    outInput.addEventListener("keydown", (e) => {
      if (e.key === "Enter" && !e.isComposing) outInput.blur();
    });
  }
  $<HTMLInputElement>("#opt-version").onchange = (e) => {
    // 틀린 값도 그대로 두고 아래 안내와 내보내기 잠금으로 알린다
    setOpt({ version: (e.target as HTMLInputElement).value.trim() });
  };
  const proj = project && exportable(project.formatVersion) ? project.formatVersion : null;
  const versionItems = [
    ...(proj ? [{ value: proj, hint: t("이 프로젝트") }] : []),
    ...KNOWN_VERSIONS.filter((v) => v !== proj).map((v) => ({
      value: v,
      hint: t("{mm} 계열", { mm: v.slice(0, 3) }),
    })),
  ];
  attachSuggestions($<HTMLInputElement>("#opt-version"), versionItems, document.getElementById("btn-version-list"));
  const vReset = document.getElementById("btn-version-reset");
  if (vReset) vReset.onclick = () => setOpt({ version: "" });
  $<HTMLInputElement>("#opt-atlas").onchange = (e) => setOpt({ atlas: (e.target as HTMLInputElement).checked });
  const onSel = (id: string, fn: (v: string) => void) => {
    const el = document.getElementById(id) as HTMLSelectElement | null;
    if (el) el.onchange = () => fn(el.value);
  };
  onSel("opt-pack-source", (v) => setOpt({ packSource: v as ExportOptions["packSource"] }));
  onSel("opt-pack-target", (v) => setOpt({ packTarget: v as ExportOptions["packTarget"] }));
  const packBtn = document.getElementById("btn-pack");
  if (packBtn) packBtn.onclick = editPack;
  const chk = (id: string, key: keyof ExportOptions) => {
    const el = document.querySelector<HTMLInputElement>(id);
    if (el) el.onchange = () => setOpt({ [key]: el.checked });
  };
  chk("#opt-ness", "nonessential");
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
  return KNOWN_VERSIONS.find((x) => x.startsWith(`${m[1]}.${m[2]}.`)) ?? `${m[1]}.${m[2]}.0`;
}

$("#btn-settings-load").onclick = async () => {
  const f = await openDialog({
    multiple: false,
    title: t("Spine 내보내기 설정 불러오기"),
    defaultPath: project ? dirname(project.path) : undefined,
    filters: [{ name: t("Spine 내보내기 설정"), extensions: ["json"] }],
  });
  if (typeof f !== "string") return;
  try {
    const got = await invoke<ImportedSettings>("load_export_settings", { path: f, options });
    setOpt(importedPatch(got));
    const kind = importedKind(got);
    const acts = got.output ? [{ label: t("출력 폴더도 적용"), run: () => useOutDir(got.output!) }] : undefined;
    const sub = [got.output ? t("설정에 적힌 출력 폴더: {p}", { p: got.output }) : "", ...got.notes].filter(Boolean).join("\n");
    toast(got.notes.length ? "warning" : "success", t("Spine {kind} 내보내기 설정을 불러왔습니다", { kind }), sub || undefined, acts, got.output || got.notes.length ? 12000 : 4200);
  } catch (e) {
    toast("error", t("설정을 불러올 수 없습니다"), String(e), undefined, 8000);
  }
};

/** 불러온 Spine 설정 → 지금 설정에 덮을 값. */
function importedPatch(got: ImportedSettings): Partial<ExportOptions> {
  const o = got.options;
  // 확장자·JSON 형식·Unity 형식은 이 앱의 값을 그대로 둔다(고르는 칸이 없다)
  const patch: Partial<ExportOptions> = { ...o, jsonFormat: options.jsonFormat, jsonExt: options.jsonExt, skelExt: options.skelExt };
  delete patch.unity;
  if (options.unity && patch.pack) patch.pack = { ...patch.pack, atlasExtension: UNITY_ATLAS_EXT };
  // 설정에 없던 값(null)은 지금 값을 둔다
  for (const k of Object.keys(patch) as (keyof ExportOptions)[]) if (patch[k] === null || patch[k] === undefined) delete patch[k];
  patch.version = versionFromSpine(got.version) ?? options.version;
  return patch;
}

const importedKind = (got: ImportedSettings) => (got.options.format === "json" ? "JSON" : t("바이너리"));

function useOutDir(dir: string) {
  prefs.customOut = dir;
  prefs.outMode = "custom";
  save("sfe.prefs", prefs);
  renderSettings();
}

/** 키 순서와 상관없이 같은 값인지. */
function sameValue(a: unknown, b: unknown): boolean {
  if (a === b) return true;
  if (typeof a !== "object" || typeof b !== "object" || a === null || b === null) return false;
  if (Array.isArray(a) !== Array.isArray(b)) return false;
  const ka = Object.keys(a);
  const kb = Object.keys(b);
  return ka.length === kb.length && ka.every((k) => sameValue((a as Record<string, unknown>)[k], (b as Record<string, unknown>)[k]));
}

/**
 * 연 프로젝트에 Spine 에서 마지막으로 쓴 내보내기 설정이 저장돼 있으면 가져올지 묻는다.
 * 가져와도 바뀌는 것이 없으면 묻지 않는다. 예를 누르면 설정에 적힌 출력 폴더도 함께 쓴다.
 */
let offering = false;
async function offerProjectSettings() {
  // 같은 파일이 두 번 열려도(앱을 켤 때 넘겨받은 파일 등) 한 번만 묻는다
  if (!project || offering) return;
  offering = true;
  try {
    await askProjectSettings();
  } finally {
    offering = false;
  }
}

async function askProjectSettings() {
  let got: ImportedSettings | null;
  try {
    got = await invoke<ImportedSettings | null>("project_export_settings", { options });
  } catch (e) {
    toast("warning", t("프로젝트의 내보내기 설정을 읽지 못했습니다"), String(e), undefined, 7000);
    return;
  }
  if (!got) return;
  const patch = importedPatch(got);
  const out = got.output && got.output !== outDir() ? got.output : null;
  const changed = (Object.keys(patch) as (keyof ExportOptions)[]).some((k) => !sameValue(options[k], patch[k]));
  if (!changed && !out) return;
  if (!(await confirmDialog(t("내보내기 설정을 가져올까요?"), "", [], t("예"), { noLabel: t("아니오") }))) return;
  if (out) {
    prefs.customOut = out;
    prefs.outMode = "custom";
    save("sfe.prefs", prefs);
  }
  setOpt(patch);
  toast(got.notes.length ? "warning" : "success", t("내보내기 설정을 가져왔습니다"), got.notes.join("\n") || undefined, undefined, got.notes.length ? 10000 : 4200);
}

$("#btn-settings-save").onclick = async () => {
  const kind = options.format === "json" ? "json" : "binary";
  const f = await saveDialog({
    title: t("Spine 내보내기 설정 저장"),
    defaultPath: project ? joinPath(dirname(project.path), `${kind}.export.json`) : `${kind}.export.json`,
    filters: [{ name: t("Spine 내보내기 설정"), extensions: ["json"] }],
  });
  if (typeof f !== "string") return;
  try {
    await invoke("save_export_settings", { path: f, options, input: project?.path ?? null, output: outDir() });
    const both = options.format === "both" ? t(" 형식이 «둘 다»라 바이너리 설정으로 저장했습니다(Spine 설정 파일은 한 형식만 담습니다).") : "";
    toast("success", t("Spine 내보내기 설정을 저장했습니다"), `${f}${both}`, undefined, both ? 9000 : 4200);
  } catch (e) {
    toast("error", t("설정을 저장할 수 없습니다"), String(e), undefined, 8000);
  }
};

let packOpen = false;
async function editPack() {
  if (packOpen) return;
  packOpen = true;
  try {
    const next = await openPackerDialog(options.pack, options.unity, DEFAULT_PACK, (kind, title, sub) => toast(kind, title, sub));
    if (!next) return;
    // 바이너리 확장자는 Unity 형식을 따른다
    setOpt({ pack: next.pack, unity: next.unity, skelExt: next.unity ? UNITY_SKEL_EXT : ".skel" });
  } finally {
    packOpen = false;
  }
}

async function pickOut() {
  const dir = await openDialog({ directory: true, title: t("출력 폴더"), defaultPath: outDir() ?? undefined });
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
  setStatus(project ? t("포맷 {v} · 스켈레톤 {n}개 · {ms}ms", { v: project.formatVersion, n: project.skeletons.length, ms: project.loadMs }) : t("열린 프로젝트 없음"));
}

// ───────────────────────────────────────── 열기

/** ask: 연 뒤 프로젝트에 저장된 내보내기 설정을 가져올지 묻는다(다시 읽기는 묻지 않는다). */
async function openProject(path: string, ask = true): Promise<boolean> {
  if (loading || exporting) return false;
  loading = true;
  let opened = false;
  const name = path.split(/[\\/]/).pop() ?? path;
  setStatus(t("{name} 읽는 중…", { name }), null);
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
    const box = $<HTMLInputElement>("#proj-path");
    box.value = info.path;
    box.title = info.path;
    $<HTMLInputElement>("#filter").disabled = false;
    $<HTMLButtonElement>("#btn-reload").disabled = false;
    $<HTMLButtonElement>("#btn-reveal").disabled = false;
    void getCurrentWindow().setTitle(`${info.fileName} — Quick Spine`);
    renderList();
    renderSettings();
    opened = true;
    if (info.partialError) toast("warning", t("스켈레톤 뒤 편집기 상태 일부를 읽지 못했습니다"), t("내보내기에는 영향이 없습니다: {e}", { e: info.partialError }), undefined, 7000);
  } catch (e) {
    toast("error", t("{name} 을(를) 열 수 없습니다", { name }), String(e), undefined, 8000);
  } finally {
    loading = false;
    idleStatus();
  }
  if (opened && ask) await offerProjectSettings();
  return opened;
}

/** 지금 프로젝트를 디스크에서 다시 읽고, 되면 토스트로 알린다. */
async function reloadProject() {
  if (!project) return;
  if (await openProject(project.path, false) && project)
    toast("success", t("다시 읽었습니다"), t("{name} · 스켈레톤 {n}개", { name: project.fileName, n: project.skeletons.length }), undefined, 2400);
}

async function pickFile() {
  const f = await openDialog({ multiple: false, title: t("Spine 프로젝트 열기"), filters: [{ name: t("Spine 프로젝트"), extensions: ["spine"] }] });
  if (typeof f === "string") await openProject(f);
}

$("#btn-open").onclick = pickFile;

// 프로젝트 경로 칸: 경로를 적고 Enter 로 연다. Esc·칸을 벗어나면 열린 프로젝트 경로로 되돌린다.
{
  const box = $<HTMLInputElement>("#proj-path");
  const restore = () => {
    box.value = project?.path ?? "";
  };
  box.addEventListener("keydown", (e) => {
    if (e.isComposing) return;
    if (e.key === "Enter") {
      e.preventDefault();
      const path = box.value.trim().replace(/^(["'])(.*)\1$/, "$2");
      if (!path) return restore();
      if (!path.toLowerCase().endsWith(".spine")) {
        toast("warning", t(".spine 파일이 아닙니다"), t("Spine 프로젝트 파일(.spine)의 경로를 적으세요."));
        return;
      }
      box.blur();
      void openProject(path).then(restore);
    } else if (e.key === "Escape") {
      restore();
      box.blur();
    }
  });
  box.addEventListener("blur", () => {
    if (!loading) restore();
  });
}
$("#btn-reload").onclick = () => void reloadProject();
$("#btn-reveal").onclick = async () => {
  const d = outDir();
  if (!d) return;
  try {
    await revealItemInDir(d);
  } catch {
    toast("info", t("출력 폴더가 아직 없습니다"), t("내보내면 만들어집니다."));
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
      ? `<div class="t">${t("놓으면 엽니다")}</div><div class="s">${esc(f.split(/[\\/]/).pop()!)}</div>`
      : `<div class="t">${t(".spine 파일이 아닙니다")}</div><div class="s">${t("Spine 프로젝트 파일만 열 수 있습니다")}</div>`;
    drop.classList.add("on");
  } else if (p.type === "leave") {
    drop.classList.remove("on");
  } else if (p.type === "drop") {
    drop.classList.remove("on");
    const f = p.paths.find((x) => x.toLowerCase().endsWith(".spine"));
    if (f) void openProject(f);
    else toast("warning", t(".spine 파일이 아닙니다"), t("Spine 프로젝트 파일(.spine)을 끌어 놓으세요."));
  }
});

// 파인더·탐색기에서 넘어온 파일
void listen<string[]>("open-files", (e) => {
  void invoke("take_pending_open");
  const f = e.payload[e.payload.length - 1];
  if (f) void openProject(f);
});

// ───────────────────────────────────────── 내보내기

/** 예/아니오 대화상자(결과 창과 같은 모양). Enter=확인, Esc·바깥=취소. noLabel: 거절 단추 글(기본: 취소). */
function confirmDialog(title: string, sub: string, items: string[], okLabel: string, { noLabel = t("취소") } = {}): Promise<boolean> {
  return new Promise((resolve) => {
    const back = document.createElement("div");
    back.className = "modal-backdrop";
    const rest = items.length > 12 ? `<div class="res-more">${t("외 {n}개", { n: items.length - 12 })}</div>` : "";
    back.innerHTML = `<div class="modal" role="alertdialog" aria-modal="true" aria-label="${esc(title)}">
      <div class="m-head">${esc(title)}${sub ? `<div class="sub">${esc(sub)}</div>` : ""}</div>
      ${items.length ? `<div class="m-body"><div class="file-list">${items.slice(0, 12).map((f) => `<div>${esc(f)}</div>`).join("")}${rest}</div></div>` : ""}
      <div class="m-foot"><button class="btn" data-act="no">${esc(noLabel)}</button><button class="btn primary" data-act="yes">${esc(okLabel)}</button></div></div>`;
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
    <div class="m-head">${t("내보내기 결과")}<div class="sub">${t("{ok}개 성공 · {fail}개 실패 · {s}초", { ok: sum.succeeded, fail: sum.failed, s: (sum.elapsedMs / 1000).toFixed(2) })}</div></div>
    <div class="m-body">${rows
      .sort((a, b) => Number(a.ok) - Number(b.ok) || (a.index ?? Infinity) - (b.index ?? Infinity))
      .map(
        (r) => `<div class="res-row">${(r.ok ? icons.check : icons.error).replace("<svg", `<svg class="ic ${r.ok ? "ok" : "fail"}"`)}
        <div style="min-width:0;flex:1"><div class="nm">${esc(r.skeleton)}</div>
        ${r.ok ? `<div class="files">${r.files.map(esc).join(" · ")}</div>` : `<div class="msg">${esc(r.message ?? "")}</div>`}
        ${r.notes.map((n) => `<div class="note">${esc(n)}</div>`).join("")}</div></div>`,
      )
      .join("")}</div>
    <div class="m-foot"><button class="btn" id="m-reveal">${icons.folder}<span>${t("폴더 열기")}</span></button><button class="btn primary" id="m-close">${t("닫기")}</button></div></div>`;
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
    toast("error", t("지원하지 않는 버전입니다"), blocker);
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
      const ok = await confirmDialog(t("파일 {n}개를 덮어씁니다", { n: existing.length }), t("출력 폴더에 같은 이름의 파일이 이미 있습니다."), existing, t("덮어쓰고 내보내기"));
      if (!ok) return;
    }
  } finally {
    confirming = false;
  }
  exporting = true;
  for (const i of indices) exportState.set(i, "run");
  renderList();
  setStatus(t("내보내는 중… {done}/{total}", { done: 0, total: indices.length }), 0);
  const rows: ExportProgress[] = [];
  const un = await listen<ExportProgress>("export-progress", (e) => {
    const p = e.payload;
    rows.push(p);
    if (p.index !== null) exportState.set(p.index, p.ok ? "ok" : "fail");
    setStatus(t("내보내는 중… {done}/{total}", { done: p.done, total: p.total }), p.done / p.total);
    const row = document.querySelector(`.sk-row[data-i="${p.index}"] .state`);
    if (row) row.outerHTML = p.ok ? `<span class="state ok" title="${t("내보냄")}">${icons.check}</span>` : `<span class="state fail" title="${t("실패")}">${icons.error}</span>`;
  });
  let last = "";
  try {
    const sum = await invoke<ExportSummary>("export_skeletons", { indices, outDir: dir, options });
    const secs = (sum.elapsedMs / 1000).toFixed(2);
    const notes = rows.flatMap((r) => r.notes);
    // 끝나면 토스트만: 누르면 Finder 에서 출력 폴더를(내보낸 파일을 골라 둔 채) 연다
    const firstFile = rows.find((r) => r.ok && r.files.length)?.files[0];
    const reveal = () => void revealItemInDir(firstFile ? joinPath(dir, firstFile) : dir).catch(() => toast("info", t("출력 폴더를 열 수 없습니다"), dir));
    const fine = sum.failed === 0 && notes.length === 0;
    const title =
      sum.failed === 0
        ? t("내보냈습니다")
        : sum.succeeded === 0
          ? t("내보내지 못했습니다")
          : t("{n}개를 내보내지 못했습니다", { n: sum.failed });
    const sub = sum.failed === 0 && notes.length ? t("알림 {n}개", { n: notes.length }) : undefined;
    toast(
      fine ? "success" : sum.succeeded === 0 ? "error" : "warning",
      title,
      sub,
      fine ? undefined : [{ label: t("자세히"), run: () => showResults(rows, sum) }],
      fine ? 5000 : 10000,
      sum.succeeded > 0 ? { onClick: reveal, tip: isMac ? t("눌러서 Finder 에서 열기") : t("눌러서 탐색기에서 열기") } : {},
    );
    last = t("마지막 내보내기: {ok}개 성공 · {fail}개 실패 · {s}초", { ok: sum.succeeded, fail: sum.failed, s: secs });
  } catch (e) {
    for (const i of indices) if (exportState.get(i) === "run") exportState.delete(i);
    toast("error", t("내보내기를 시작할 수 없습니다"), String(e), undefined, 8000);
  } finally {
    un();
    exporting = false;
    renderList();
    if (last) setStatus(last);
    else idleStatus();
  }
}
$("#btn-export").onclick = runExport;

// ───────────────────────────────────────── 단축키

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
    void reloadProject();
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
  } else if (mod && e.key === ",") {
    // 메뉴 단축키가 먼저 받는 것이 보통이지만, 못 받은 경우를 위해
    e.preventDefault();
    showSettings();
  } else if (e.key === "Escape" && typing) {
    $("#sk-list").focus();
  }
});

// ───────────────────────────────────────── 앱 설정 창(메뉴 › 설정…)

/** 언어를 바꿔 창을 다시 그릴 때 이어 갈 상태. */
const RESTORE_KEY = "sfe.restore";
interface RestoreState {
  path: string;
  checked: number[];
  filter: string;
  /** 설정 창에서 언어를 바꿨으면 다시 그린 뒤 설정 창을 다시 띄운다. */
  settings?: boolean;
}

function showSettings() {
  // 다른 대화상자(팩 설정·덮어쓰기 확인 등)가 떠 있으면 그것부터 닫게 한다
  if ([...document.querySelectorAll(".modal-backdrop")].some((b) => !b.querySelector(".settings-modal"))) return;
  openSettingsDialog({
    current: { theme: prefs.theme, lang: prefs.lang },
    busy: exporting || loading,
    onChange: (next) => {
      prefs.theme = next.theme;
      prefs.lang = next.lang;
      save("sfe.prefs", prefs);
      applyTheme();
      if (resolveLang(next.lang) === LANG) return;
      // 화면 문구는 처음 그릴 때 정해지므로 창을 다시 그린다. 열린 프로젝트·체크·검색어는 이어 간다.
      // 설정 창도 다시 띄운다.
      const keep: RestoreState = { path: project?.path ?? "", checked: [...selected], filter, settings: true };
      try {
        sessionStorage.setItem(RESTORE_KEY, JSON.stringify(keep));
      } catch {
        /* 이어 가지 못해도 다시 그리기는 한다 */
      }
      location.reload();
    },
  });
}
void listen("open-settings", showSettings);
// 메뉴 막대도 이 창의 언어로
void invoke("set_menu_language", { lang: LANG }).catch(() => {});

/** 언어를 바꾸기 전 상태로 되돌린다(있으면 true). */
async function restoreAfterReload(): Promise<boolean> {
  let keep: RestoreState | null = null;
  try {
    keep = JSON.parse(sessionStorage.getItem(RESTORE_KEY) ?? "null");
    sessionStorage.removeItem(RESTORE_KEY);
  } catch {
    keep = null;
  }
  if (keep?.settings) showSettings();
  if (!keep?.path) return false;
  await openProject(keep.path, false);
  if (project?.path !== keep.path) return true;
  selected.clear();
  for (const i of keep.checked) if (i >= 0 && i < project.skeletons.length) selected.add(i);
  if (keep.filter) {
    filter = keep.filter;
    $<HTMLInputElement>("#filter").value = keep.filter;
  }
  renderList();
  return true;
}


// ───────────────────────────────────────── 시작

renderList();
renderSettings();
idleStatus();
// 언어를 바꿔 다시 그린 경우엔 열려 있던 프로젝트를, 아니면 앱을 켤 때 함께 넘어온 파일(파인더 «다음으로 열기», 탐색기 연결)을 연다
void restoreAfterReload().then(async () => {
  const paths = await invoke<string[]>("take_pending_open");
  const f = paths[paths.length - 1];
  if (f) void openProject(f);
});
