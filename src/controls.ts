// 공용 컨트롤: 슬라이더 칸(숫자칸 + 막대), 셀렉트(자체 펼침 목록), 묶음 버튼 키보드 이동.
// 디자인 시트 5.3(입력칸)·6(상태 규칙)을 따른다.

import { t } from "./i18n";

const esc = (s: string) => s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);

// ───────────────────────────────────────── 셀렉트

/**
 * ▾ 아이콘을 단 셀렉트. 펼친 목록은 [`enhanceSelects`] 가 그리는 자체 목록(JetBrains 콤보 상자 모양)이다.
 * def: 기본값(목록에서 파란 글자 + «기본값» 표시).
 */
export function selectHtml(
  attrs: string,
  /** short: 버튼에만 보일 짧은 이름(좁은 칸용). 펼친 목록에는 label 이 보인다. */
  items: [value: string, label: string, disabled?: boolean, short?: string][],
  current: string,
  enabled = true,
  tip = "",
  def?: string,
): string {
  // WKWebView 는 <select> 를 누르면 OS 메뉴를 띄운다(mousedown 을 막아도). 그래서 보이는 것은 자체 버튼이고,
  // 값과 change 이벤트는 숨긴 <select> 가 맡는다(기존 처리 코드는 select 를 그대로 쓴다).
  const it = items.find(([v]) => v === current) ?? items[0];
  const label = it ? (it[3] ?? it[1]) : "";
  return `<span class="select${enabled ? "" : " disabled"}"${tip ? ` title="${esc(tip)}"` : ""}><select class="field" hidden tabindex="-1" aria-hidden="true" ${attrs} ${enabled ? "" : "disabled"}>${items
    .map(([v, l, d, sh]) => `<option value="${esc(v)}" ${v === current ? "selected" : ""} ${d ? "disabled" : ""} ${v === def ? 'data-default=""' : ""}${sh ? ` data-short="${esc(sh)}"` : ""}>${esc(l)}</option>`)
    .join("")}</select><button type="button" class="field select-btn" aria-haspopup="listbox" aria-expanded="false" ${enabled ? "" : "disabled"}><span class="v">${esc(label)}</span></button></span>`;
}

let openList: { sel: HTMLSelectElement; pop: HTMLElement; close: (focus?: boolean) => void } | null = null;

const buttonOf = (sel: HTMLSelectElement) => sel.parentElement!.querySelector<HTMLButtonElement>(".select-btn")!;

/** 열린 목록을 닫는다(대화상자를 닫을 때 등). */
export function closeDropdown() {
  openList?.close(false);
}

/** root 안의 셀렉트 버튼이 자체 목록을 열게 한다. 값이 바뀌면 숨긴 select 에 change 를 보낸다. */
export function enhanceSelects(root: HTMLElement) {
  root.querySelectorAll<HTMLSelectElement>(".select > select").forEach((sel) => {
    if (sel.dataset.enhanced) return;
    sel.dataset.enhanced = "1";
    const btn = buttonOf(sel);
    if (!btn) return;
    // 접근성(VoiceOver 등)의 누르기는 click 만 보낸다. 마우스는 mousedown 에서 바로 연다.
    let opened = false;
    btn.addEventListener("mousedown", (e) => {
      if (e.button !== 0 || btn.disabled) return;
      e.preventDefault();
      btn.focus();
      opened = true;
      if (openList?.sel === sel) openList.close();
      else showList(sel);
    });
    btn.addEventListener("click", () => {
      if (opened) {
        opened = false;
        return;
      }
      if (btn.disabled) return;
      if (openList?.sel === sel) openList.close();
      else showList(sel);
    });
    btn.addEventListener("keydown", (e) => {
      if (openList?.sel === sel) return; // 열린 동안은 목록이 키를 받는다
      if (e.key === "Enter" || e.key === " " || e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        showList(sel);
      }
    });
  });
}

function showList(sel: HTMLSelectElement) {
  openList?.close(false);
  const opts = [...sel.options];
  const pop = document.createElement("div");
  pop.className = "dropdown";
  pop.setAttribute("role", "listbox");
  pop.innerHTML = opts
    .map(
      (o, i) =>
        `<div class="dd-item${o.disabled ? " disabled" : ""}${o.hasAttribute("data-default") ? " default" : ""}" role="option" data-i="${i}" aria-selected="${i === sel.selectedIndex}">` +
        `<span class="t">${esc(o.textContent ?? "")}</span>${o.hasAttribute("data-default") ? `<span class="hint">${t("기본값")}</span>` : ""}</div>`,
    )
    .join("");
  document.body.appendChild(pop);
  const wrap = sel.parentElement as HTMLElement;
  const btn = buttonOf(sel);
  wrap.classList.add("open");
  btn.setAttribute("aria-expanded", "true");

  // 자리: 칸 바로 아래(모자라면 위), 폭은 칸 폭 이상
  const r = btn.getBoundingClientRect();
  pop.style.minWidth = `${r.width}px`;
  pop.style.left = `${Math.max(8, Math.min(r.left, innerWidth - pop.offsetWidth - 8))}px`;
  const below = innerHeight - r.bottom - 8;
  if (pop.offsetHeight > below && r.top > below) {
    pop.style.maxHeight = `${Math.min(320, r.top - 12)}px`;
    pop.style.top = `${r.top - 4 - pop.offsetHeight}px`;
  } else {
    pop.style.maxHeight = `${Math.min(320, below - 4)}px`;
    pop.style.top = `${r.bottom + 4}px`;
  }

  const items = [...pop.querySelectorAll<HTMLElement>(".dd-item")];
  let active = sel.selectedIndex;
  const setActive = (i: number, scroll = true) => {
    active = i;
    items.forEach((it, k) => it.classList.toggle("active", k === i));
    if (scroll) items[i]?.scrollIntoView({ block: "nearest" });
  };
  const move = (from: number, dir: number) => {
    for (let i = from + dir; i >= 0 && i < items.length; i += dir) if (!opts[i].disabled) return setActive(i);
  };
  setActive(active);

  const choose = (i: number) => {
    if (i < 0 || opts[i].disabled) return;
    const changed = sel.selectedIndex !== i;
    close(true);
    if (changed) {
      sel.selectedIndex = i;
      const v = btn.querySelector(".v");
      if (v) v.textContent = opts[i].dataset.short ?? opts[i].textContent ?? "";
      sel.dispatchEvent(new Event("change", { bubbles: true }));
    }
  };
  let typed = "";
  let typedAt = 0;
  const onKey = (e: KeyboardEvent) => {
    const k = e.key;
    if (k === "ArrowDown") move(active, 1);
    else if (k === "ArrowUp") move(active, -1);
    else if (k === "Home") move(-1, 1);
    else if (k === "End") move(items.length, -1);
    else if (k === "Enter" || k === " ") choose(active);
    else if (k === "Escape") close(true);
    else if (k === "Tab") {
      close(false);
      return; // 포커스 이동은 그대로
    } else if (k.length === 1 && !e.metaKey && !e.ctrlKey) {
      // 앞 글자로 찾기
      typed = Date.now() - typedAt > 700 ? k.toLowerCase() : typed + k.toLowerCase();
      typedAt = Date.now();
      const hit = opts.findIndex((o) => !o.disabled && (o.textContent ?? "").toLowerCase().startsWith(typed));
      if (hit >= 0) setActive(hit);
    } else return;
    e.preventDefault();
    e.stopImmediatePropagation(); // 대화상자 Esc·Enter 로 넘기지 않는다
  };
  const onDown = (e: MouseEvent) => {
    if (!pop.contains(e.target as Node) && !btn.contains(e.target as Node)) close(false);
  };
  const onScroll = (e: Event) => {
    if (!pop.contains(e.target as Node)) close(false);
  };
  pop.addEventListener("mousemove", (e) => {
    const it = (e.target as HTMLElement).closest(".dd-item") as HTMLElement | null;
    if (it && !it.classList.contains("disabled")) setActive(Number(it.dataset.i), false);
  });
  pop.addEventListener("mousedown", (e) => e.preventDefault()); // 포커스는 셀렉트에 둔다
  pop.addEventListener("click", (e) => {
    const it = (e.target as HTMLElement).closest(".dd-item") as HTMLElement | null;
    if (it) choose(Number(it.dataset.i));
  });
  window.addEventListener("keydown", onKey, true);
  window.addEventListener("mousedown", onDown, true);
  window.addEventListener("scroll", onScroll, true);
  window.addEventListener("resize", close as () => void);
  window.addEventListener("blur", close as () => void);

  function close(focus = true) {
    if (openList?.pop !== pop) return;
    openList = null;
    pop.remove();
    wrap.classList.remove("open");
    btn.setAttribute("aria-expanded", "false");
    window.removeEventListener("keydown", onKey, true);
    window.removeEventListener("mousedown", onDown, true);
    window.removeEventListener("scroll", onScroll, true);
    window.removeEventListener("resize", close as () => void);
    window.removeEventListener("blur", close as () => void);
    if (focus === true && btn.isConnected) btn.focus();
  }
  openList = { sel, pop, close };
}

// ───────────────────────────────────────── 묶음 버튼(.view-switch)

/** 묶음 버튼: 켜진 것만 Tab 으로 들어가고, 좌우 방향키로 옮기면 바로 고른다. */
export function bindSegmented(root: HTMLElement) {
  root.querySelectorAll<HTMLElement>(".view-switch").forEach((group) => {
    const buttons = [...group.querySelectorAll<HTMLButtonElement>("button")];
    group.setAttribute("role", "radiogroup");
    buttons.forEach((b) => {
      b.setAttribute("role", "radio");
      b.setAttribute("aria-checked", String(b.classList.contains("on")));
      b.tabIndex = b.classList.contains("on") ? 0 : -1;
    });
    if (!buttons.some((b) => b.tabIndex === 0) && buttons[0]) buttons[0].tabIndex = 0;
    group.onkeydown = (e) => {
      if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(e.key)) return;
      e.preventDefault();
      const at = buttons.findIndex((b) => b.classList.contains("on"));
      const next = e.key === "Home" ? 0 : e.key === "End" ? buttons.length - 1 : Math.max(0, Math.min(buttons.length - 1, at + (e.key === "ArrowRight" ? 1 : -1)));
      if (next === at || buttons[next].disabled) return;
      const id = buttons[next].dataset;
      buttons[next].click();
      // 다시 그려졌으면 새 버튼에 포커스를 준다
      const sel = Object.keys(id).map((k) => `[data-${k.replace(/[A-Z]/g, (c) => "-" + c.toLowerCase())}="${id[k]}"]`).join("");
      (document.querySelector<HTMLElement>(`.view-switch button${sel}`) ?? buttons[next]).focus();
    };
  });
}

// ───────────────────────────────────────── 슬라이더 칸

export interface SliderOptions {
  /** 라벨(좌우로 끌면 값이 바뀐다). */
  label: string;
  tip?: string;
  value: number;
  min: number;
  max: number;
  /** 막대가 붙는 값 목록(예: 2의 거듭제곱). 없으면 min~max 를 고르게. */
  stops?: number[];
  /** 숫자칸에 적을 수 있는 가장 큰 값(막대 끝보다 클 수 있다). */
  inputMax?: number;
  onChange: (v: number) => void;
}

export interface Slider {
  set(v: number): void;
  /** 오류 표시(null 이면 지운다). */
  invalid(message: string | null): void;
}

const pow2 = (from: number, to: number) => {
  const v: number[] = [];
  for (let x = from; x <= to; x *= 2) v.push(x);
  return v;
};
export const SIZE_STOPS = [0, ...pow2(16, 8192)];
export const MAX_SIZE_STOPS = pow2(16, 8192);

/**
 * 라벨 + 숫자칸 + 막대. label 은 그리드의 왼쪽 칸, field 는 오른쪽 칸에 넣는다.
 * - 막대: 누른 곳으로 이동·끌기, 끄는 동안 값 말풍선, 방향키 ±1(Shift ±10; 눈금 막대는 늘 한 눈금), Home/End, 포커스 중 휠.
 * - 숫자칸: 직접 입력(Enter·포커스 잃을 때 반영), 위아래 방향키 ±1(Shift ±10).
 * - 라벨: 좌우로 끌면 값이 바뀐다(Unity 인스펙터처럼). Shift 는 10배.
 */
export function mountSlider(labelHost: HTMLElement, fieldHost: HTMLElement, o: SliderOptions): Slider {
  const inputMax = o.inputMax ?? o.max;
  let value = o.value;
  labelHost.className = "lbl scrub";
  labelHost.textContent = `${o.label}:`;
  if (o.tip) labelHost.title = `${o.tip}\n${t("(좌우로 끌어 값 바꾸기)")}`;
  fieldHost.className = "slider";
  if (o.tip) fieldHost.title = o.tip;
  fieldHost.innerHTML = `
    <input type="text" inputmode="numeric" class="field mono num" spellcheck="false" aria-label="${esc(o.label)}" />
    <div class="sl-track" role="slider" tabindex="0" aria-label="${esc(o.label)}" aria-valuemin="${o.min}" aria-valuemax="${o.max}">
      <div class="sl-rail"><div class="sl-fill"></div></div>
      ${o.stops ? `<div class="sl-ticks">${o.stops.map((_, i) => `<i style="left:${(i / (o.stops!.length - 1)) * 100}%"></i>`).join("")}</div>` : ""}
      <div class="sl-thumb"><span class="sl-bubble"></span></div>
    </div>`;
  const input = fieldHost.querySelector("input") as HTMLInputElement;
  const track = fieldHost.querySelector(".sl-track") as HTMLElement;
  const fill = fieldHost.querySelector(".sl-fill") as HTMLElement;
  const thumb = fieldHost.querySelector(".sl-thumb") as HTMLElement;
  const bubble = fieldHost.querySelector(".sl-bubble") as HTMLElement;

  // 값 ↔ 막대 위치(0~1). 눈금 막대는 눈금 사이를 고르게(로그) 나눈다.
  const toPos = (v: number): number => {
    if (!o.stops) return o.max === o.min ? 0 : Math.max(0, Math.min(1, (v - o.min) / (o.max - o.min)));
    const s = o.stops;
    if (v <= s[0]) return 0;
    if (v >= s[s.length - 1]) return 1;
    let i = 0;
    while (v > s[i + 1]) i++;
    const a = s[i] || 1;
    const frac = s[i] === 0 ? (v - s[i]) / (s[i + 1] - s[i]) : Math.log(v / a) / Math.log(s[i + 1] / a);
    return (i + frac) / (s.length - 1);
  };
  const fromPos = (p: number): number => {
    p = Math.max(0, Math.min(1, p));
    if (!o.stops) return Math.round(o.min + p * (o.max - o.min));
    return o.stops[Math.round(p * (o.stops.length - 1))];
  };
  const clamp = (v: number) => Math.max(o.min, Math.min(inputMax, Math.round(v)));

  function paint() {
    const p = toPos(value) * 100;
    fill.style.width = `${p}%`;
    thumb.style.left = `${p}%`;
    bubble.textContent = String(value);
    track.setAttribute("aria-valuenow", String(value));
    if (document.activeElement !== input) input.value = String(value);
  }
  function commit(v: number) {
    v = clamp(v);
    if (v === value) return paint();
    value = v;
    paint();
    o.onChange(value);
  }
  // 한 칸 이동: 눈금 막대는 늘 다음 눈금(사이 값은 숫자칸에 적는다), 아니면 ±1(Shift ±10)
  function stepBy(dir: number, big: boolean) {
    if (o.stops) {
      const s = o.stops;
      const next = dir > 0 ? s.find((x) => x > value) : [...s].reverse().find((x) => x < value);
      commit(next ?? value);
    } else commit(value + dir * (big ? 10 : 1));
  }

  // 막대: 누르기·끌기
  track.addEventListener("pointerdown", (e) => {
    if (e.button !== 0) return;
    e.preventDefault();
    track.focus();
    track.setPointerCapture(e.pointerId);
    track.classList.add("dragging");
    const r = track.getBoundingClientRect();
    const move = (ev: PointerEvent) => commit(fromPos((ev.clientX - r.left) / r.width));
    move(e);
    const up = () => {
      track.classList.remove("dragging");
      track.removeEventListener("pointermove", move);
      track.removeEventListener("pointerup", up);
      track.removeEventListener("pointercancel", up);
    };
    track.addEventListener("pointermove", move);
    track.addEventListener("pointerup", up);
    track.addEventListener("pointercancel", up);
  });
  track.addEventListener("keydown", (e) => {
    const dir = { ArrowRight: 1, ArrowUp: 1, ArrowLeft: -1, ArrowDown: -1 }[e.key];
    if (dir) {
      e.preventDefault();
      stepBy(dir, e.shiftKey);
    } else if (e.key === "Home") {
      e.preventDefault();
      commit(o.min);
    } else if (e.key === "End") {
      e.preventDefault();
      commit(o.max);
    } else if (e.key === "PageUp" || e.key === "PageDown") {
      e.preventDefault();
      stepBy(e.key === "PageUp" ? 1 : -1, true);
    }
  });
  track.addEventListener(
    "wheel",
    (e) => {
      if (document.activeElement !== track) return; // 포커스가 없을 때는 대화상자 스크롤
      e.preventDefault();
      stepBy(e.deltaY < 0 || e.deltaX > 0 ? 1 : -1, e.shiftKey);
    },
    { passive: false },
  );

  // 숫자칸
  const fromInput = () => {
    const n = Number(input.value.trim());
    if (input.value.trim() !== "" && Number.isFinite(n)) commit(n);
    else paint();
    input.value = String(value);
  };
  input.addEventListener("change", fromInput);
  input.addEventListener("blur", fromInput);
  input.addEventListener("keydown", (e) => {
    if (e.key === "ArrowUp" || e.key === "ArrowDown") {
      e.preventDefault();
      commit(value + (e.key === "ArrowUp" ? 1 : -1) * (e.shiftKey ? 10 : 1));
      input.value = String(value);
      input.select();
    } else if (e.key === "Enter") fromInput();
  });
  input.addEventListener("focus", () => input.select());

  // 라벨 끌기(4px 마다 1, 눈금 막대는 24px 마다 한 칸)
  labelHost.addEventListener("pointerdown", (e) => {
    if (e.button !== 0) return;
    e.preventDefault();
    labelHost.setPointerCapture(e.pointerId);
    const x0 = e.clientX;
    const v0 = value;
    let moved = false;
    document.body.classList.add("scrubbing");
    labelHost.classList.add("active");
    const move = (ev: PointerEvent) => {
      const dx = ev.clientX - x0;
      if (Math.abs(dx) > 2) moved = true;
      if (!moved) return;
      if (o.stops) {
        const steps = Math.trunc(dx / 24);
        const s = o.stops;
        let i = s.findIndex((x) => x >= v0);
        if (i < 0) i = s.length - 1;
        commit(s[Math.max(0, Math.min(s.length - 1, i + steps))]);
      } else commit(v0 + Math.trunc(dx / 4) * (ev.shiftKey ? 10 : 1));
    };
    const up = () => {
      document.body.classList.remove("scrubbing");
      labelHost.classList.remove("active");
      labelHost.removeEventListener("pointermove", move);
      labelHost.removeEventListener("pointerup", up);
      labelHost.removeEventListener("pointercancel", up);
      if (!moved) input.focus();
    };
    labelHost.addEventListener("pointermove", move);
    labelHost.addEventListener("pointerup", up);
    labelHost.addEventListener("pointercancel", up);
  });

  paint();
  return {
    set(v: number) {
      value = clamp(v);
      input.value = String(value);
      paint();
    },
    invalid(message: string | null) {
      fieldHost.classList.toggle("invalid", !!message);
      input.title = message ?? "";
      track.title = message ?? "";
    },
  };
}

// ───────────────────────────────────────── 추천 목록이 붙은 입력칸(콤보)

export interface Suggestion {
  value: string;
  hint?: string;
}

/**
 * 입력칸 아래에 자체 추천 목록을 띄운다(OS datalist 는 WKWebView 에서 투명하게 그려진다).
 * - 열기: 칸 클릭, ↓, 적기, 또는 toggle 버튼(Tab 으로 들어올 때는 열지 않는다). 적는 동안 앞부분이 맞는 것만 보인다.
 * - ↑↓ 이동 · Enter 고르기(고른 줄이 없으면 적은 값 그대로) · Esc 닫기 · 클릭 고르기.
 * 고르면 input.value 를 바꾸고 change 를 보낸다.
 */
export function attachSuggestions(input: HTMLInputElement, items: Suggestion[], toggle?: HTMLElement | null) {
  let pop: HTMLElement | null = null;
  let active = -1;
  let shown: Suggestion[] = [];
  const wrap = input.closest(".combo") as HTMLElement | null;

  const close = () => {
    pop?.remove();
    pop = null;
    active = -1;
    wrap?.classList.remove("open");
    input.setAttribute("aria-expanded", "false");
  };
  const pick = (v: string) => {
    close();
    if (input.value !== v) {
      input.value = v;
      input.dispatchEvent(new Event("change", { bubbles: true }));
    }
  };
  const paint = () => pop?.querySelectorAll<HTMLElement>(".dd-item").forEach((el, k) => el.classList.toggle("active", k === active));

  function open(all: boolean) {
    const q = input.value.trim().toLowerCase();
    shown = all || !q ? items : items.filter((it) => it.value.toLowerCase().startsWith(q));
    if (!shown.length) return close();
    if (!pop) {
      pop = document.createElement("div");
      pop.className = "dropdown suggest";
      pop.setAttribute("role", "listbox");
      pop.addEventListener("mousedown", (e) => e.preventDefault()); // 포커스는 칸에 둔다
      pop.addEventListener("mousemove", (e) => {
        const it = (e.target as HTMLElement).closest(".dd-item") as HTMLElement | null;
        if (it) {
          active = Number(it.dataset.i);
          paint();
        }
      });
      pop.addEventListener("click", (e) => {
        const it = (e.target as HTMLElement).closest(".dd-item") as HTMLElement | null;
        if (it) pick(shown[Number(it.dataset.i)].value);
      });
      document.body.appendChild(pop);
      wrap?.classList.add("open");
      input.setAttribute("aria-expanded", "true");
    }
    const cur = input.value.trim();
    pop.innerHTML = shown
      .map(
        (it, i) =>
          `<div class="dd-item${it.value === cur ? " current" : ""}" role="option" data-i="${i}"><span class="t mono">${esc(it.value)}</span>${it.hint ? `<span class="hint">${esc(it.hint)}</span>` : ""}</div>`,
      )
      .join("");
    active = shown.findIndex((it) => it.value === cur);
    paint();
    const r = (wrap ?? input).getBoundingClientRect();
    pop.style.minWidth = `${r.width}px`;
    pop.style.left = `${r.left}px`;
    const below = innerHeight - r.bottom - 8;
    pop.style.maxHeight = `${Math.min(280, Math.max(below - 4, 120))}px`;
    pop.style.top = pop.offsetHeight > below && r.top > below ? `${r.top - 4 - pop.offsetHeight}px` : `${r.bottom + 4}px`;
  }

  input.setAttribute("role", "combobox");
  input.setAttribute("aria-autocomplete", "list");
  input.setAttribute("aria-expanded", "false");
  input.addEventListener("mousedown", () => {
    if (pop) close();
    else setTimeout(() => document.activeElement === input && open(true));
  });
  input.addEventListener("input", () => open(false));
  input.addEventListener("blur", close);
  input.addEventListener("keydown", (e) => {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (!pop) return open(true);
      active = Math.max(0, Math.min(shown.length - 1, active + (e.key === "ArrowDown" ? 1 : -1)));
      paint();
      pop.querySelector<HTMLElement>(`.dd-item[data-i="${active}"]`)?.scrollIntoView({ block: "nearest" });
    } else if (e.key === "Enter" && pop && active >= 0) {
      e.preventDefault();
      e.stopPropagation();
      pick(shown[active].value);
    } else if (e.key === "Escape" && pop) {
      e.preventDefault();
      e.stopPropagation();
      close();
    } else if (e.key === "Tab") close();
  });
  addEventListener("resize", close);
  toggle?.addEventListener("mousedown", (e) => {
    e.preventDefault();
    if (pop) close();
    else {
      input.focus();
      open(true);
    }
  });
}
