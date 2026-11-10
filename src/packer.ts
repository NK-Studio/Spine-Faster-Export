// Spine 의 «Texture Packer Settings» 대화상자와 같은 배치·이름. 값은 Spine pack.json 과 같은 키를 쓴다.
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";

import { MAX_SIZE_STOPS, SIZE_STOPS, closeDropdown, enhanceSelects, mountSlider, selectHtml, type Slider } from "./controls";

/** spine-unity 가 가져오는 확장자. */
export const UNITY_ATLAS_EXT = ".atlas.txt";
export const UNITY_SKEL_EXT = ".skel.bytes";

export type Filter = "Nearest" | "Linear" | "MipMap" | "MipMapNearestNearest" | "MipMapLinearNearest" | "MipMapNearestLinear" | "MipMapLinearLinear";
export type Wrap = "ClampToEdge" | "Repeat" | "MirroredRepeat";
export type PixelFormat = "Alpha" | "Intensity" | "LuminanceAlpha" | "RGB565" | "RGBA4444" | "RGB888" | "RGBA8888";

export interface PackSettings {
  stripWhitespaceX: boolean;
  stripWhitespaceY: boolean;
  rotation: boolean;
  alias: boolean;
  ignoreBlankImages: boolean;
  alphaThreshold: number;
  paddingX: number;
  paddingY: number;
  edgePadding: boolean;
  duplicatePadding: boolean;
  minWidth: number;
  minHeight: number;
  maxWidth: number;
  maxHeight: number;
  pot: boolean;
  multipleOfFour: boolean;
  square: boolean;
  filterMin: Filter;
  filterMag: Filter;
  wrapX: Wrap;
  wrapY: Wrap;
  format: PixelFormat;
  outputFormat: string;
  grid: boolean;
  /** Spine 4.x 의 Packing 이름(rectangles · grid · polygons). 화면은 `grid` 를 쓰고 저장할 때 맞춘다. */
  packing: string;
  premultiplyAlpha: boolean;
  bleed: boolean;
  bleedIterations: number;
  scale: number[];
  scaleSuffix: string[];
  atlasExtension: string;
  combineSubdirectories: boolean;
  flattenPaths: boolean;
  useIndexes: boolean;
  fast: boolean;
  debug: boolean;
  /** 두 번째 페이지부터 이름과 번호 사이에 넣는 글자(이름_2.png). */
  separator: string;
}

const FILTERS: Filter[] = ["Nearest", "Linear", "MipMap", "MipMapNearestNearest", "MipMapLinearNearest", "MipMapNearestLinear", "MipMapLinearLinear"];
const WRAPS: Wrap[] = ["ClampToEdge", "Repeat", "MirroredRepeat"];
const FORMATS: PixelFormat[] = ["Alpha", "Intensity", "LuminanceAlpha", "RGB565", "RGBA4444", "RGB888", "RGBA8888"];

const esc = (s: string) => s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);

/** 메인 창에 보일 한 줄 요약. */
export function packSummary(p: PackSettings, unity = false): string {
  const parts = [`${p.maxWidth}×${p.maxHeight}`, `여백 ${p.paddingX},${p.paddingY}`];
  if (p.premultiplyAlpha) parts.push("PMA");
  else if (p.bleed) parts.push("Bleed");
  if (p.rotation) parts.push("회전");
  if (p.alias) parts.push("Alias");
  if (p.pot) parts.push("POT");
  if (p.multipleOfFour) parts.push("4의 배수");
  if (p.square) parts.push("정사각형");
  if (p.grid) parts.push("Grid");
  const scales = p.scale.filter((s) => s > 0);
  if (scales.length > 1 || (scales.length === 1 && scales[0] !== 1)) parts.push(`배율 ${scales.join(", ")}`);
  parts.push(p.atlasExtension + (unity ? " (Unity)" : ""));
  return parts.join(" · ");
}

export interface PackerResult {
  pack: PackSettings;
  unity: boolean;
}

interface SliderSpec {
  key: "alphaThreshold" | "paddingX" | "paddingY" | "minWidth" | "minHeight" | "maxWidth" | "maxHeight";
  label: string;
  tip: string;
  min: number;
  max: number;
  stops?: number[];
  inputMax?: number;
}

const SLIDERS: SliderSpec[] = [
  { key: "alphaThreshold", label: "Alpha threshold", tip: "이 알파 이하는 투명으로 보고 잘라냅니다", min: 0, max: 255 },
  { key: "paddingX", label: "Padding X", tip: "영역 사이 가로 여백(픽셀)", min: 0, max: 16, inputMax: 1024 },
  { key: "paddingY", label: "Padding Y", tip: "영역 사이 세로 여백(픽셀)", min: 0, max: 16, inputMax: 1024 },
  { key: "minWidth", label: "Min width", tip: "페이지 최소 폭", min: 0, max: 8192, stops: SIZE_STOPS, inputMax: 16384 },
  { key: "minHeight", label: "Min height", tip: "페이지 최소 높이", min: 0, max: 8192, stops: SIZE_STOPS, inputMax: 16384 },
  { key: "maxWidth", label: "Max width", tip: "페이지 최대 폭", min: 16, max: 8192, stops: MAX_SIZE_STOPS, inputMax: 16384 },
  { key: "maxHeight", label: "Max height", tip: "페이지 최대 높이", min: 16, max: 8192, stops: MAX_SIZE_STOPS, inputMax: 16384 },
];

/** 대화상자를 열어 고친 설정을 돌려준다(Cancel 이면 null). */
export function openPackerDialog(
  current: PackSettings,
  currentUnity: boolean,
  defaults: PackSettings,
  toast: (kind: "info" | "error", title: string, sub?: string) => void,
): Promise<PackerResult | null> {
  return new Promise((resolve) => {
    let s: PackSettings = structuredClone(current);
    let unity = currentUnity;
    // Unity 를 끄면 돌아갈 확장자
    let extBeforeUnity = current.atlasExtension === UNITY_ATLAS_EXT ? defaults.atlasExtension : current.atlasExtension;
    const sliders = new Map<string, Slider>();
    const back = document.createElement("div");
    back.className = "modal-backdrop";
    back.innerHTML = `<div class="modal packer" role="dialog" aria-modal="true" aria-label="Texture Packer Settings" tabindex="-1">
      <div class="pk-head">Texture Packer Settings</div>
      <div class="pk-body" id="pk-body"></div>
      <div class="pk-foot">
        <button class="btn" data-act="load" title="pack.json 불러오기">Load</button>
        <button class="btn" data-act="save" title="pack.json 으로 저장">Save</button>
        <button class="btn" data-act="defaults" title="기본값으로">Defaults</button>
        <span class="gap"></span>
        <button class="btn primary" data-act="ok" title="적용 (Enter)">OK</button>
        <button class="btn" data-act="cancel" title="닫기 (Esc)">Cancel</button>
      </div>
    </div>`;
    document.body.appendChild(back);
    const body = back.querySelector("#pk-body") as HTMLElement;
    const dlg = back.querySelector(".modal") as HTMLElement;

    const chk = (key: keyof PackSettings | "unity", label: string, tip: string, enabled = true, on?: boolean) =>
      `<label class="check${enabled ? "" : " disabled"}" title="${esc(tip)}"><input type="checkbox" class="tri-check" data-k="${key}" ${(on ?? s[key as keyof PackSettings]) ? "checked" : ""} ${enabled ? "" : "disabled"}/><span>${label}</span></label>`;
    const slot = (key: string) => `<span data-sl-label="${key}"></span><div data-sl-field="${key}"></div>`;
    const sel = (key: keyof PackSettings, label: string, items: string[], tip: string) =>
      `<span class="lbl" title="${esc(tip)}">${label}:</span>${selectHtml(`data-k="${key}"`, items.map((v) => [v, v]), String(s[key]), true, tip, String(defaults[key]))}`;

    function render() {
      const scales = [0, 1, 2, 3, 4].map((i) => s.scale[i]);
      body.innerHTML = `
        <section class="pk-sec">
          <div class="pk-title">Regions</div>
          <div class="pk-checks">
            ${chk("stripWhitespaceX", "Strip whitespace X", "가로 투명 테두리를 잘라냅니다")}
            ${chk("stripWhitespaceY", "Strip whitespace Y", "세로 투명 테두리를 잘라냅니다")}
            ${chk("rotation", "Rotation", "더 촘촘하면 90도 돌려 넣습니다")}
            ${chk("alias", "Alias", "같은 그림은 한 자리를 함께 씁니다")}
            ${chk("ignoreBlankImages", "Ignore blank images", "완전히 투명한 이미지는 넣지 않습니다")}
          </div>
          <div class="pk-kv">${slot("alphaThreshold")}</div>
        </section>

        <section class="pk-sec">
          <div class="pk-title">Region Padding</div>
          <div class="pk-kv">${slot("paddingX")}${slot("paddingY")}</div>
          <div class="pk-checks center">
            ${chk("edgePadding", "Edge padding", "페이지 가장자리에도 여백을 둡니다")}
            ${chk("duplicatePadding", "Duplicate padding", "여백을 영역 가장자리 픽셀로 채웁니다")}
          </div>
        </section>

        <section class="pk-sec">
          <div class="pk-title">Pages</div>
          <div class="pk-kv">${slot("minWidth")}${slot("minHeight")}${slot("maxWidth")}${slot("maxHeight")}</div>
          <div class="pk-checks center">
            ${chk("pot", "Power of two", "페이지 크기를 2의 거듭제곱으로 맞춥니다")}
            ${chk("multipleOfFour", "Divisible by 4", "페이지 크기를 4의 배수로 맞춥니다(일부 압축 텍스처 형식에 필요)")}
            ${chk("square", "Square", "페이지를 정사각형으로 맞춥니다")}
          </div>
        </section>

        <section class="pk-sec">
          <div class="pk-title">Runtime</div>
          <div class="pk-kv">
            ${sel("filterMin", "Filter min", FILTERS, "축소 필터(atlas filter)")}
            ${sel("filterMag", "Filter mag", FILTERS, "확대 필터(atlas filter)")}
            ${sel("wrapX", "Wrap X", WRAPS, "가로 랩(atlas repeat)")}
            ${sel("wrapY", "Wrap Y", WRAPS, "세로 랩(atlas repeat)")}
            ${sel("format", "Format", FORMATS, "런타임이 텍스처를 올릴 픽셀 형식(atlas format). PNG 는 RGBA 로 저장합니다")}
          </div>
        </section>

        <section class="pk-sec">
          <div class="pk-title">Output</div>
          <div class="pk-kv two">
            <span class="lbl">Format:</span>
            ${selectHtml("", [["png", "PNG"], ["jpg", "JPG (지원 안 함)", true]], "png", true, "이 앱은 PNG 만 만듭니다")}
            <span class="lbl">Packing:</span>
            ${selectHtml('data-k="grid"', [["false", "Atlas"], ["true", "Grid"]], String(s.grid), true, "Atlas: 촘촘히 · Grid: 같은 크기 칸에 차례로", String(defaults.grid))}
          </div>
          <div class="pk-checks center">
            ${chk("premultiplyAlpha", "Premultiply alpha", "미리 곱한 알파로 저장합니다(atlas pma)")}
            ${chk("bleed", "Bleed", s.premultiplyAlpha ? "Premultiply alpha 를 끄면 쓸 수 있습니다" : "투명 픽셀의 색을 주변 색으로 채웁니다", !s.premultiplyAlpha)}
          </div>
          <div class="pk-scale">
            <span class="lbl" title="배율마다 atlas 를 하나씩 만듭니다">Scale:</span>
            ${scales.map((v, i) => `<input type="text" inputmode="decimal" class="field mono${v ? "" : " empty"}" data-scale="${i}" placeholder="${i === 0 ? "1" : ""}" spellcheck="false" value="${v ?? ""}" />`).join("")}
            <span class="lbl" title="배율별 파일 이름 접미사(예: @2x)">Suffix:</span>
            ${[0, 1, 2, 3, 4].map((i) => `<input type="text" class="field mono${s.scaleSuffix[i] ? "" : " empty"}" data-suffix="${i}" placeholder="${i === 0 ? "" : ["", "@2x", "@0.5x", "@3x", "@4x"][i]}" spellcheck="false" value="${esc(s.scaleSuffix[i] ?? "")}" />`).join("")}
          </div>
        </section>

        <section class="pk-sec">
          <div class="pk-title">Options</div>
          <div class="pk-kv two">
            <span class="lbl">Atlas extension:</span>
            <div class="row">
              <input type="text" class="field mono ext" data-k="atlasExtension" spellcheck="false" value="${esc(s.atlasExtension)}" ${unity ? "disabled" : ""} />
              ${unity ? `<span class="tag" title="spine-unity 가 가져오는 형식입니다">Unity</span>` : ""}
            </div>
          </div>
          <div class="pk-checks">
            ${chk("unity", "Unity (spine-unity)", `spine-unity 가져오기 형식: atlas 는 ${UNITY_ATLAS_EXT}, 바이너리는 ${UNITY_SKEL_EXT}`, true, unity)}
            ${chk("combineSubdirectories", "Combine subdirectories", "«이미지 폴더»로 묶을 때: 켜면 하위 폴더의 이미지를 같은 페이지에 섞어 묶고, 끄면 폴더마다 페이지를 따로 씁니다. «첨부 이미지»로 묶을 때는 쓰지 않습니다")}
            ${chk("flattenPaths", "Flatten paths", "영역 이름에서 폴더를 뺍니다")}
            ${chk("useIndexes", "Use indexes", "이름 끝의 _숫자를 index 로 씁니다")}
            ${chk("fast", "Fast", "덜 촘촘하지만 빨리 묶습니다")}
            ${chk("debug", "Debug", "영역 테두리를 분홍 선으로 그립니다")}
          </div>
        </section>`;
      sliders.clear();
      for (const sp of SLIDERS) {
        const label = body.querySelector<HTMLElement>(`[data-sl-label="${sp.key}"]`)!;
        const field = body.querySelector<HTMLElement>(`[data-sl-field="${sp.key}"]`)!;
        sliders.set(
          sp.key,
          mountSlider(label, field, {
            ...sp,
            value: s[sp.key],
            onChange: (v) => {
              s[sp.key] = v;
              showErrors();
            },
          }),
        );
      }
      bind();
      enhanceSelects(body);
      showErrors();
    }

    function bind() {
      body.querySelectorAll<HTMLInputElement>('input[type="checkbox"][data-k]').forEach(
        (el) =>
          (el.onchange = () => {
            const k = el.dataset.k!;
            if (k === "unity") {
              unity = el.checked;
              if (unity) {
                if (s.atlasExtension !== UNITY_ATLAS_EXT) extBeforeUnity = s.atlasExtension;
                s.atlasExtension = UNITY_ATLAS_EXT;
              } else s.atlasExtension = extBeforeUnity;
              return render();
            }
            (s as any)[k] = el.checked;
            if (k === "premultiplyAlpha") render();
          }),
      );
      body.querySelectorAll<HTMLSelectElement>("select[data-k]").forEach(
        (el) => (el.onchange = () => ((s as any)[el.dataset.k!] = el.dataset.k === "grid" ? el.value === "true" : el.value)),
      );
      const ext = body.querySelector<HTMLInputElement>('input[data-k="atlasExtension"]')!;
      ext.onchange = () => {
        let v = ext.value.trim() || ".atlas";
        if (!v.startsWith(".")) v = "." + v;
        s.atlasExtension = ext.value = v;
      };
      body.querySelectorAll<HTMLInputElement>("[data-scale]").forEach(
        (el) =>
          (el.onchange = () => {
            const v = Number(el.value);
            const i = Number(el.dataset.scale);
            const scales = [0, 1, 2, 3, 4].map((k) => s.scale[k] ?? 0);
            scales[i] = el.value.trim() === "" || !Number.isFinite(v) ? 0 : Math.max(0, v);
            el.value = scales[i] ? String(scales[i]) : "";
            el.classList.toggle("empty", !scales[i]);
            compactScales(scales, [0, 1, 2, 3, 4].map((k) => s.scaleSuffix[k] ?? ""));
            showErrors();
          }),
      );
      body.querySelectorAll<HTMLInputElement>("[data-suffix]").forEach(
        (el) =>
          (el.onchange = () => {
            const suffixes = [0, 1, 2, 3, 4].map((k) => s.scaleSuffix[k] ?? "");
            suffixes[Number(el.dataset.suffix)] = el.value.trim();
            el.classList.toggle("empty", !el.value.trim());
            compactScales([0, 1, 2, 3, 4].map((k) => s.scale[k] ?? 0), suffixes);
            showErrors();
          }),
      );
    }

    // 칸 자리를 그대로 두되, 저장할 때는 배율이 있는 칸까지만.
    function compactScales(scales: number[], suffixes: string[]) {
      let n = 5;
      while (n > 0 && !(scales[n - 1] > 0) && !suffixes[n - 1]) n--;
      s.scale = scales.slice(0, n);
      s.scaleSuffix = suffixes.slice(0, n);
    }

    /** 칸별 오류. */
    function errors(): Record<string, string> {
      const e: Record<string, string> = {};
      if (s.maxWidth < 16) e.maxWidth = "16 이상이어야 합니다.";
      if (s.maxHeight < 16) e.maxHeight = "16 이상이어야 합니다.";
      if (s.minWidth > s.maxWidth) e.minWidth = "Max width 보다 클 수 없습니다.";
      if (s.minHeight > s.maxHeight) e.minHeight = "Max height 보다 클 수 없습니다.";
      const used = s.scale.map((v, i) => [v, s.scaleSuffix[i] ?? ""] as const).filter(([v]) => v > 0);
      if (!used.length) e.scale = "Scale 을 하나 이상 적으세요(예: 1).";
      const sfx = used.map(([, x]) => x);
      if (new Set(sfx).size !== sfx.length) e.suffix = "배율마다 다른 Suffix 를 적으세요(예: @2x).";
      return e;
    }
    function showErrors() {
      const e = errors();
      for (const [k, sl] of sliders) sl.invalid(e[k] ?? null);
      body.querySelectorAll<HTMLInputElement>("[data-scale]").forEach((el, i) => {
        const bad = !!e.scale && i === 0;
        el.classList.toggle("invalid", bad);
        el.title = bad ? e.scale : "";
      });
      body.querySelectorAll<HTMLInputElement>("[data-suffix]").forEach((el) => {
        el.classList.toggle("invalid", !!e.suffix && s.scale[Number(el.dataset.suffix)] > 0);
        el.title = e.suffix ?? "";
      });
      return e;
    }
    function validate(): string | null {
      const e = showErrors();
      const labels: Record<string, string> = { minWidth: "Min width", minHeight: "Min height", maxWidth: "Max width", maxHeight: "Max height", scale: "Scale", suffix: "Suffix" };
      const first = Object.keys(e)[0];
      return first ? `${labels[first] ?? first}: ${e[first]}` : null;
    }

    const close = (v: PackerResult | null) => {
      closeDropdown();
      back.remove();
      resolve(v);
    };
    const ok = () => {
      const err = validate();
      if (err) return toast("error", "팩 설정을 확인하세요", err);
      // 빈 배율 칸은 빼고 저장
      const pairs = s.scale.map((v, i) => [v, s.scaleSuffix[i] ?? ""] as const).filter(([v]) => v > 0);
      close({ pack: { ...s, scale: pairs.map(([v]) => v), scaleSuffix: pairs.map(([, x]) => x) }, unity });
    };

    back.querySelector<HTMLElement>('[data-act="ok"]')!.onclick = ok;
    back.querySelector<HTMLElement>('[data-act="cancel"]')!.onclick = () => close(null);
    back.querySelector<HTMLElement>('[data-act="defaults"]')!.onclick = () => {
      s = structuredClone(defaults);
      unity = false;
      extBeforeUnity = defaults.atlasExtension;
      render();
    };
    back.querySelector<HTMLElement>('[data-act="save"]')!.onclick = async () => {
      const err = validate();
      if (err) return toast("error", "팩 설정을 확인하세요", err);
      const path = await saveDialog({ title: "팩 설정 저장", defaultPath: "pack.json", filters: [{ name: "Spine 팩 설정", extensions: ["json"] }] });
      if (!path) return;
      try {
        await invoke("save_pack_settings", { path, settings: s });
        toast("info", "팩 설정을 저장했습니다", path);
      } catch (e) {
        toast("error", "저장할 수 없습니다", String(e));
      }
    };
    back.querySelector<HTMLElement>('[data-act="load"]')!.onclick = async () => {
      const path = await openDialog({ title: "팩 설정 불러오기", multiple: false, filters: [{ name: "Spine 팩 설정", extensions: ["json"] }] });
      if (typeof path !== "string") return;
      try {
        s = await invoke<PackSettings>("load_pack_settings", { path });
        // 불러온 확장자가 Unity 형식이면 Unity 를 켠다
        unity = s.atlasExtension === UNITY_ATLAS_EXT;
        if (!unity) extBeforeUnity = s.atlasExtension;
        render();
        toast("info", "팩 설정을 불러왔습니다", path);
      } catch (e) {
        toast("error", "불러올 수 없습니다", String(e));
      }
    };
    back.addEventListener("keydown", (e) => {
      const t = e.target as HTMLElement;
      if (e.key === "Escape") {
        e.stopPropagation();
        close(null);
      } else if (e.key === "Enter" && t.tagName !== "BUTTON" && t.tagName !== "SELECT" && !t.classList.contains("sl-track")) {
        e.preventDefault();
        t.blur?.(); // 입력 중인 값을 먼저 반영
        ok();
      } else if (e.key === "Tab") {
        // 포커스를 대화상자 안에 가둔다
        const items = [...dlg.querySelectorAll<HTMLElement>("button, input, select, [tabindex='0']")].filter((x) => !(x as HTMLInputElement).disabled && x.offsetParent);
        if (!items.length) return;
        const first = items[0];
        const last = items[items.length - 1];
        if (e.shiftKey && document.activeElement === first) {
          e.preventDefault();
          last.focus();
        } else if (!e.shiftKey && document.activeElement === last) {
          e.preventDefault();
          first.focus();
        }
      }
    });
    back.addEventListener("mousedown", (e) => {
      // 바깥을 눌러도 닫지 않는다(Spine 과 같음). 대신 대화상자를 살짝 흔들어 알린다.
      if (e.target === back) {
        dlg.classList.remove("nudge");
        void dlg.offsetWidth;
        dlg.classList.add("nudge");
      }
    });
    render();
    dlg.focus();
  });
}
