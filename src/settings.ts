// 앱 설정 창(메뉴: 맥 «Quick Spine > 설정…», 윈도우 «편집 > 환경설정…», 단축키 ⌘, / Ctrl+,).
// 왼쪽은 분류 목록, 오른쪽은 그 분류의 항목. 고르면 바로 적용된다.
import { closeDropdown, enhanceSelects, selectHtml } from "./controls";
import { LANGS, t, type LangPref } from "./i18n";

export type ThemePref = "system" | "dark" | "light";

export interface AppSettings {
  theme: ThemePref;
  lang: LangPref;
}

export interface SettingsDialogOptions {
  current: AppSettings;
  /** 내보내는 중이면 언어는 바꿀 수 없다(바꾸면 창을 다시 그리므로). */
  busy: boolean;
  /** 값을 고를 때마다 부른다(바로 적용). */
  onChange: (next: AppSettings) => void;
}

const esc = (s: string) => s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);

type Section = "general";

let openBack: HTMLElement | null = null;

export function openSettingsDialog(o: SettingsDialogOptions) {
  if (openBack) {
    openBack.querySelector<HTMLElement>(".modal")?.focus();
    return;
  }
  const sections: [Section, string][] = [["general", t("일반")]];
  const sec: Section = "general";
  const back = document.createElement("div");
  back.className = "modal-backdrop";
  back.innerHTML = `<div class="modal settings-modal" role="dialog" aria-modal="true" aria-label="${esc(t("설정"))}" tabindex="-1">
    <div class="m-head">${esc(t("설정"))}</div>
    <div class="set-wrap">
      <nav class="set-nav" role="tablist" aria-orientation="vertical">
        ${sections.map(([k, label]) => `<button type="button" role="tab" data-sec="${k}" aria-selected="${k === sec}" class="${k === sec ? "on" : ""}">${esc(label)}</button>`).join("")}
      </nav>
      <div class="set-pane" role="tabpanel">
        <div class="set-title">${esc(t("일반"))}</div>
        <div class="set-field">
          <span class="set-lbl">${esc(t("테마"))}</span>
          ${selectHtml(
            'id="set-theme"',
            [
              ["system", t("시스템")],
              ["dark", t("다크")],
              ["light", t("라이트")],
            ],
            o.current.theme,
            true,
            "",
            "system",
          )}
        </div>
        <div class="set-field">
          <span class="set-lbl">${esc(t("언어"))}</span>
          ${selectHtml('id="set-lang"', [["system", t("시스템")], ...LANGS], o.current.lang, !o.busy, t("앱 화면과 메뉴에 쓸 언어"), "system")}
          <div class="hint">${esc(o.busy ? t("내보내는 중에는 언어를 바꿀 수 없습니다.") : t("시스템: 운영체제 설정을 따릅니다"))}</div>
        </div>
      </div>
    </div>
    <div class="m-foot"><button class="btn primary" data-act="ok">${esc(t("확인"))}</button></div>
  </div>`;
  document.body.appendChild(back);
  openBack = back;
  const dlg = back.querySelector<HTMLElement>(".modal")!;
  enhanceSelects(dlg);

  const close = () => {
    closeDropdown();
    back.remove();
    openBack = null;
  };
  let cur = { ...o.current };
  const apply = () => {
    const theme = back.querySelector<HTMLSelectElement>("#set-theme")!.value as ThemePref;
    const lang = back.querySelector<HTMLSelectElement>("#set-lang")!.value as LangPref;
    if (theme === cur.theme && lang === cur.lang) return;
    cur = { theme, lang };
    o.onChange(cur);
  };
  back.querySelector<HTMLSelectElement>("#set-theme")!.addEventListener("change", apply);
  back.querySelector<HTMLSelectElement>("#set-lang")!.addEventListener("change", apply);
  back.querySelector<HTMLElement>('[data-act="ok"]')!.onclick = close;
  back.addEventListener("mousedown", (e) => e.target === back && close());
  back.addEventListener("keydown", (e) => {
    if (e.key === "Escape") {
      e.preventDefault();
      close();
    } else if (e.key === "Enter" && (e.target as HTMLElement).tagName !== "BUTTON") {
      e.preventDefault();
      close();
    }
  });
  // 대화상자 자체에 포커스(첫 칸에 파란 테두리가 바로 뜨지 않게). Tab 을 누르면 칸으로 간다.
  dlg.focus();
}
