// 브라우저 기본 title 툴팁은 1초 넘게 기다려야 떠서, 짧은 지연으로 직접 띄운다.
// 화면 코드는 그대로 title 을 쓰고, 마우스가 올라오는 순간 title 을 data-tip 으로 옮겨
// 기본 툴팁이 겹쳐 뜨지 않게 한다. 나중에 title 을 다시 쓰면 다음 호버 때 새 값으로 바뀐다.

/** 처음 뜨기까지 기다리는 시간. */
const SHOW_DELAY = 300;
/** 툴팁이 닫힌 뒤 이 시간 안에 다른 항목으로 옮기면 바로 띄운다. */
const WARM_MS = 500;
const GAP = 8;
const MARGIN = 8;

let tipEl: HTMLDivElement | null = null;
let owner: HTMLElement | null = null;
let showTimer = 0;
let hiddenAt = 0;

/** title 을 data-tip 으로 옮기고, 툴팁이 있는 가장 가까운 요소를 돌려준다. */
function findOwner(start: EventTarget | null): HTMLElement | null {
  let el = start instanceof Element ? (start as HTMLElement) : null;
  while (el && el !== document.body) {
    const title = el.getAttribute("title");
    if (title !== null) {
      el.removeAttribute("title");
      if (title.trim()) {
        el.dataset.tip = title;
        // 글자 없는 아이콘 버튼은 title 이 접근성 이름이었으니 남겨 둔다
        if (!el.hasAttribute("aria-label") && !el.textContent?.trim()) el.setAttribute("aria-label", title);
      } else delete el.dataset.tip;
    }
    if (el.dataset.tip) return el;
    el = el.parentElement;
  }
  return null;
}

function ensureEl(): HTMLDivElement {
  if (!tipEl) {
    tipEl = document.createElement("div");
    tipEl.className = "tooltip";
    tipEl.setAttribute("role", "tooltip");
    document.body.appendChild(tipEl);
  }
  return tipEl;
}

function place(el: HTMLElement, target: HTMLElement) {
  const r = target.getBoundingClientRect();
  const w = el.offsetWidth;
  const h = el.offsetHeight;
  const vw = document.documentElement.clientWidth;
  const vh = document.documentElement.clientHeight;
  let top = r.bottom + GAP;
  let below = true;
  if (top + h > vh - MARGIN && r.top - GAP - h >= MARGIN) {
    top = r.top - GAP - h;
    below = false;
  }
  const left = Math.min(Math.max(r.left + r.width / 2 - w / 2, MARGIN), Math.max(MARGIN, vw - w - MARGIN));
  el.style.left = `${Math.round(left)}px`;
  el.style.top = `${Math.round(top)}px`;
  el.classList.toggle("above", !below);
}

function show(target: HTMLElement) {
  if (!target.isConnected || !target.dataset.tip) return;
  const el = ensureEl();
  el.textContent = target.dataset.tip;
  el.classList.remove("on");
  el.style.left = "0px";
  el.style.top = "0px";
  el.style.display = "block";
  // place 가 크기를 읽으며 꺼진 상태의 스타일을 확정하므로, 바로 켜도 나타나는 전환이 걸린다
  place(el, target);
  el.classList.add("on");
}

function hide() {
  clearTimeout(showTimer);
  showTimer = 0;
  if (tipEl && tipEl.style.display === "block") {
    tipEl.classList.remove("on");
    tipEl.style.display = "none";
    hiddenAt = performance.now();
  }
  owner = null;
}

function onOver(e: PointerEvent) {
  if (e.pointerType === "touch") return;
  const next = findOwner(e.target);
  if (next === owner) return;
  const warm = tipEl?.style.display === "block" || performance.now() - hiddenAt < WARM_MS;
  hide();
  if (!next) return;
  owner = next;
  if (warm) show(next);
  else showTimer = window.setTimeout(() => owner === next && show(next), SHOW_DELAY);
}

function onOut(e: PointerEvent) {
  if (!owner) return;
  const to = e.relatedTarget as Node | null;
  if (to && owner.contains(to)) return;
  // 바깥으로 나간 경우만 닫는다(다른 툴팁 항목으로 옮기면 pointerover 가 이어서 처리한다)
  if (!to) hide();
}

export function installTooltips() {
  document.addEventListener("pointerover", onOver, true);
  document.addEventListener("pointerout", onOut, true);
  // 누르거나 키를 치거나 화면이 움직이면 닫고, 같은 항목에서는 다시 뜨지 않게 한다
  const dismiss = () => {
    const keep = owner;
    hide();
    hiddenAt = 0;
    owner = keep;
  };
  document.addEventListener("pointerdown", dismiss, true);
  document.addEventListener("keydown", dismiss, true);
  document.addEventListener("wheel", dismiss, { capture: true, passive: true });
  document.addEventListener("scroll", dismiss, true);
  window.addEventListener("blur", hide);
}
