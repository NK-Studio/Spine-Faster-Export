// Spine 의 «Texture Packer Settings» 대화상자와 같은 배치·이름. 값은 Spine pack.json 과 같은 키를 쓴다.
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";

import { t } from "./i18n";
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
  /** 페이지 파일 형식: "png" · "jpg". */
  outputFormat: string;
  /** JPG 품질(0~1, Spine `jpegQuality`). */
  jpegQuality: number;
  grid: boolean;
  /** Spine 4.x 의 Packing 이름(rectangles · grid · polygons). 그리드면 `grid` 도 함께 켠다. */
  packing: string;
  premultiplyAlpha: boolean;
  bleed: boolean;
  bleedIterations: number;
  scale: number[];
  scaleSuffix: string[];
  /** 배율별 리샘플링 방식(Spine 이름). 모자라면 bicubic. */
  scaleResampling: string[];
  /** 한 페이지에 다 들어갈 때까지 배율을 0.01 씩 낮춘다(Spine 의 Auto scale). */
  autoScale: boolean;
  atlasExtension: string;
  combineSubdirectories: boolean;
  flattenPaths: boolean;
  useIndexes: boolean;
  fast: boolean;
  debug: boolean;
  /** 두 번째 페이지부터 이름과 번호 사이에 넣는 글자(이름_2.png). */
  separator: string;
  /** 3.8 이하 런타임이 읽는 옛 atlas 형식으로 쓴다(Spine 의 Legacy output). */
  legacyOutput: boolean;
  /** atlas 를 탭 들여쓰기와 띄어쓰기로 읽기 좋게 쓴다(Spine 의 Pretty print). */
  prettyPrint: boolean;
}

const FILTERS: Filter[] = ["Nearest", "Linear", "MipMap", "MipMapNearestNearest", "MipMapLinearNearest", "MipMapNearestLinear", "MipMapLinearLinear"];
const WRAPS: Wrap[] = ["ClampToEdge", "Repeat", "MirroredRepeat"];
const FORMATS: PixelFormat[] = ["Alpha", "Intensity", "LuminanceAlpha", "RGB565", "RGBA4444", "RGB888", "RGBA8888"];
/** 배율 리샘플링(Spine 의 Resampling). Spine 에서 auto 는 bicubic 과, bilinearProgressive 는 bilinear 와 결과가 같다. */
/** [값, 목록에 보일 이름, , 좁은 버튼에 보일 이름] */
const RESAMPLINGS: [string, string, boolean, string][] = [
  ["auto", "Auto", false, "Auto"],
  ["nearest", "Nearest", false, "NN"],
  ["bilinear", "Bilinear", false, "BL"],
  ["bilinearProgressive", "Bilinear progressive", false, "BL+"],
  ["bicubic", "Bicubic", false, "BC"],
];
const DEFAULT_RESAMPLING = "bicubic";

const esc = (s: string) => s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);

/** 패킹 방식(rectangles · grid · polygons). `grid` 가 켜져 있으면 그리드. */
const packingOf = (p: PackSettings) => (p.grid ? "grid" : p.packing === "polygons" ? "polygons" : "rectangles");

/** 메인 창에 보일 한 줄 요약. */
export function packSummary(p: PackSettings, unity = false): string {
  const parts = [`${p.maxWidth}×${p.maxHeight}`, t("여백 {x},{y}", { x: p.paddingX, y: p.paddingY })];
  if (p.outputFormat === "jpg") parts.push(`JPG ${Math.round(p.jpegQuality * 100)}%`);
  if (p.premultiplyAlpha) parts.push("PMA");
  else if (p.bleed) parts.push(t("블리드"));
  if (p.rotation) parts.push(t("회전"));
  if (p.alias) parts.push(t("에일리어스"));
  if (p.pot) parts.push("POT");
  if (p.multipleOfFour) parts.push(t("4의 배수"));
  if (p.square) parts.push(t("정사각형"));
  if (p.grid) parts.push(t("그리드"));
  else if (p.packing === "polygons") parts.push(t("다각형"));
  const scales = p.scale.filter((s) => s > 0);
  if (scales.length > 1 || (scales.length === 1 && scales[0] !== 1)) {
    parts.push(t("배율 {s}", { s: scales.join(", ") }));
    const names = p.scale
      .map((v, i) => [v, p.scaleResampling?.[i] ?? DEFAULT_RESAMPLING] as const)
      .filter(([v, m]) => v > 0 && v !== 1 && m !== "bicubic" && m !== "auto")
      .map(([, m]) => RESAMPLINGS.find(([k]) => k === m)?.[1] ?? m);
    if (names.length) parts.push(t("리샘플링 {m}", { m: [...new Set(names)].join(", ") }));
  }
  if (p.autoScale) parts.push(t("자동 배율"));
  parts.push(p.atlasExtension + (unity ? " (Unity)" : ""));
  return parts.join(" · ");
}

export interface PackerResult {
  pack: PackSettings;
  unity: boolean;
}

const RESAMPLE_TIP = t("배율을 적용할 때 쓰는 방법. Auto · Bicubic: 넓이 평균(부드러움) · Bilinear: 선형 보간 · Nearest: 가장 가까운 픽셀(도트 그림). Spine 에서 Auto 는 Bicubic 과, Bilinear progressive 는 Bilinear 와 결과가 같습니다");

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
  { key: "alphaThreshold", label: t("알파 임계값"), tip: t("이 알파 이하는 투명으로 보고 잘라냅니다"), min: 0, max: 255 },
  { key: "paddingX", label: t("패딩 X"), tip: t("영역 사이 가로 여백(픽셀)"), min: 0, max: 16, inputMax: 1024 },
  { key: "paddingY", label: t("패딩 Y"), tip: t("영역 사이 세로 여백(픽셀)"), min: 0, max: 16, inputMax: 1024 },
  { key: "minWidth", label: t("최소 폭"), tip: t("페이지 최소 폭"), min: 0, max: 8192, stops: SIZE_STOPS, inputMax: 16384 },
  { key: "minHeight", label: t("최소 높이"), tip: t("페이지 최소 높이"), min: 0, max: 8192, stops: SIZE_STOPS, inputMax: 16384 },
  { key: "maxWidth", label: t("최대 폭"), tip: t("페이지 최대 폭"), min: 16, max: 8192, stops: MAX_SIZE_STOPS, inputMax: 16384 },
  { key: "maxHeight", label: t("최대 높이"), tip: t("페이지 최대 높이"), min: 16, max: 8192, stops: MAX_SIZE_STOPS, inputMax: 16384 },
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
    s.outputFormat = s.outputFormat?.toLowerCase() === "jpg" || s.outputFormat?.toLowerCase() === "jpeg" ? "jpg" : "png";
    s.jpegQuality ??= defaults.jpegQuality;
    if (s.premultiplyAlpha) s.bleed = false; // 둘 다 켜져 있으면 블리드는 쓰이지 않는다
    let unity = currentUnity;
    // Unity 를 끄면 돌아갈 확장자
    let extBeforeUnity = current.atlasExtension === UNITY_ATLAS_EXT ? defaults.atlasExtension : current.atlasExtension;
    const sliders = new Map<string, Slider>();
    const back = document.createElement("div");
    back.className = "modal-backdrop";
    back.innerHTML = `<div class="modal packer" role="dialog" aria-modal="true" aria-label="${t("텍스처 패커 설정")}" tabindex="-1">
      <div class="pk-head">${t("텍스처 패커 설정")}</div>
      <div class="pk-body" id="pk-body"></div>
      <div class="pk-foot">
        <button class="btn" data-act="save" title="${t("pack.json 으로 저장")}">${t("저장")}</button>
        <button class="btn" data-act="load" title="${t("pack.json 불러오기")}">${t("로드")}</button>
        <button class="btn" data-act="defaults" title="${t("기본값으로")}">${t("기본값")}</button>
        <span class="gap"></span>
        <button class="btn primary" data-act="ok" title="${t("적용 (Enter)")}">${t("확인")}</button>
        <button class="btn" data-act="cancel" title="${t("닫기 (Esc)")}">${t("취소")}</button>
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
      const jpg = s.outputFormat === "jpg";
      body.innerHTML = `
        <section class="pk-sec">
          <div class="pk-title">${t("영역")}</div>
          <div class="pk-checks">
            ${chk("stripWhitespaceX", t("화이트스페이스 X 제거"), t("가로 투명 테두리를 잘라냅니다"))}
            ${chk("stripWhitespaceY", t("화이트스페이스 Y 제거"), t("세로 투명 테두리를 잘라냅니다"))}
            ${chk("rotation", t("회전"), t("더 촘촘하면 90도 돌려 넣습니다"))}
            ${chk("alias", t("에일리어스"), t("같은 그림은 한 자리를 함께 씁니다"))}
            ${chk("ignoreBlankImages", t("빈 이미지 무시"), t("완전히 투명한 이미지는 넣지 않습니다"))}
          </div>
          <div class="pk-kv">${slot("alphaThreshold")}</div>
        </section>

        <section class="pk-sec">
          <div class="pk-title">${t("영역 패딩")}</div>
          <div class="pk-kv">${slot("paddingX")}${slot("paddingY")}</div>
          <div class="pk-checks center">
            ${chk("edgePadding", t("에지 패딩"), t("페이지 가장자리에도 여백을 둡니다"))}
            ${chk("duplicatePadding", t("패딩 복제"), t("여백을 영역 가장자리 픽셀로 채웁니다"))}
          </div>
        </section>

        <section class="pk-sec">
          <div class="pk-title">${t("페이지")}</div>
          <div class="pk-kv">${slot("minWidth")}${slot("minHeight")}${slot("maxWidth")}${slot("maxHeight")}</div>
          <div class="pk-checks center">
            ${chk("pot", t("2의 거듭제곱"), t("페이지 크기를 2의 거듭제곱으로 맞춥니다"))}
            ${chk("multipleOfFour", t("4로 나누기"), t("페이지 크기를 4의 배수로 맞춥니다(일부 압축 텍스처 형식에 필요)"))}
            ${chk("square", t("정사각형"), t("페이지를 정사각형으로 맞춥니다"))}
          </div>
        </section>

        <section class="pk-sec">
          <div class="pk-title">${t("런타임")}</div>
          <div class="pk-kv">
            ${sel("filterMin", t("필터 최소화"), FILTERS, t("축소 필터(atlas filter)"))}
            ${sel("filterMag", t("필터 최대화"), FILTERS, t("확대 필터(atlas filter)"))}
            ${sel("wrapX", t("랩 X"), WRAPS, t("가로 랩(atlas repeat)"))}
            ${sel("wrapY", t("랩 Y"), WRAPS, t("세로 랩(atlas repeat)"))}
            ${sel("format", t("형식"), FORMATS, t("런타임이 텍스처를 올릴 픽셀 형식(atlas format). 파일은 PNG 면 RGBA, JPG 면 RGB 로 저장합니다"))}
          </div>
        </section>

        <section class="pk-sec">
          <div class="pk-title">${t("출력")}</div>
          <div class="pk-kv two">
            <span class="lbl">${t("형식")}:</span>
            ${selectHtml('data-k="outputFormat"', [["png", "PNG"], ["jpg", "JPG"]], jpg ? "jpg" : "png", true, t("페이지 파일 형식. JPG 는 알파가 없어 투명한 곳을 검정으로 채우고, 알파 프리멀티플라이·블리드와 상관없이 색에 알파를 곱해 씁니다(Spine 과 같음)"), "png")}
            <span class="lbl">${t("패킹")}:</span>
            ${selectHtml('data-k="packing"', [["grid", t("그리드")], ["rectangles", t("직사각형")], ["polygons", t("다각형")]], packingOf(s), true, t("직사각형: 촘촘히 · 그리드: 같은 크기 칸에 차례로 · 다각형: 메시만 쓰는 이미지를 메시 모양대로 더 촘촘히(느림)"), packingOf(defaults))}
            ${
              jpg
                ? `<span class="lbl" title="${t("JPG 품질(0~1). 높을수록 선명하고 파일이 큽니다")}">${t("JPEG 품질")}:</span>
            <div class="pk-q"><input type="text" inputmode="decimal" class="field mono quality" data-jpeg-quality spellcheck="false" value="${s.jpegQuality}" title="${t("JPG 품질(0~1). 높을수록 선명하고 파일이 큽니다")}" /></div>`
                : ""
            }
          </div>
          <div class="pk-checks center">
            ${chk("premultiplyAlpha", t("알파 프리멀티플라이"), t("미리 곱한 알파로 저장합니다(atlas pma)"))}
            ${chk("bleed", t("블리드"), t("투명 픽셀의 색을 주변 색으로 채웁니다(알파 프리멀티플라이와 함께 쓸 수 없습니다)"))}
          </div>
          <div class="pk-scale">
            <span class="lbl" title="${t("배율마다 atlas 를 하나씩 만듭니다")}">${t("크기 조정")}:</span>
            ${scales.map((v, i) => `<input type="text" inputmode="decimal" class="field mono${v ? "" : " empty"}" data-scale="${i}" placeholder="${i === 0 ? "1" : ""}" spellcheck="false" value="${v ?? ""}" />`).join("")}
            <span class="lbl" title="${t("배율별 파일 이름 접미사(예: @2x)")}">${t("접미사")}:</span>
            ${[0, 1, 2, 3, 4].map((i) => `<input type="text" class="field mono${s.scaleSuffix[i] ? "" : " empty"}" data-suffix="${i}" placeholder="${i === 0 ? "" : ["", "@2x", "@0.5x", "@3x", "@4x"][i]}" spellcheck="false" value="${esc(s.scaleSuffix[i] ?? "")}" />`).join("")}
            <span class="lbl" title="${esc(RESAMPLE_TIP)}">${t("리샘플링")}:</span>
            ${[0, 1, 2, 3, 4].map((i) => selectHtml(`data-resample="${i}"`, RESAMPLINGS, s.scaleResampling[i] ?? DEFAULT_RESAMPLING, i === 0 || (scales[i] ?? 0) > 0, RESAMPLE_TIP, DEFAULT_RESAMPLING)).join("")}
          </div>
        </section>

        <section class="pk-sec">
          <div class="pk-title">${t("옵션")}</div>
          <div class="pk-kv two">
            <span class="lbl">${t("아틀라스 확장자")}:</span>
            <div class="row">
              <input type="text" class="field mono ext" data-k="atlasExtension" spellcheck="false" value="${esc(s.atlasExtension)}" ${unity ? "disabled" : ""} />
              ${unity ? `<span class="tag" title="${t("spine-unity 가 가져오는 형식입니다")}">Unity</span>` : ""}
            </div>
          </div>
          <div class="pk-checks">
            ${chk("unity", "Unity (spine-unity)", t("spine-unity 가져오기 형식: atlas 는 {atlas}, 바이너리는 {skel}", { atlas: UNITY_ATLAS_EXT, skel: UNITY_SKEL_EXT }), true, unity)}
            ${chk("combineSubdirectories", t("하위 폴더 합치기"), t("«이미지 폴더»로 묶을 때: 켜면 하위 폴더의 이미지를 같은 페이지에 섞어 묶고, 끄면 폴더마다 페이지를 따로 씁니다. pack.json 이 있는 하위 폴더는 늘 따로 묶습니다. «첨부 이미지»로 묶을 때는 쓰지 않습니다"))}
            ${chk("flattenPaths", t("경로 병합"), t("영역 이름에서 폴더를 뺍니다"))}
            ${chk("useIndexes", t("인덱스 사용"), t("이름 끝의 _숫자를 index 로 씁니다"))}
            ${chk("fast", t("빠르게"), t("덜 촘촘하지만 빨리 묶습니다"))}
            ${chk("autoScale", t("자동 배율"), t("한 페이지에 다 들어갈 때까지 배율을 0.01 씩 낮춥니다(배율마다 다시 묶어서 느립니다). 줄인 배율은 atlas 의 scale 줄에 적힙니다"))}
            ${chk("debug", t("디버그"), t("영역 테두리를 분홍 선으로 그립니다"))}
            ${chk("prettyPrint", t("보기 좋게 쓰기"), t("atlas 를 탭으로 들여 쓰고 «size: 1024, 512» 처럼 띄어 씁니다(Spine 과 같은 글자). 런타임은 둘 다 읽습니다. 레거시 출력에는 쓰지 않습니다"))}
            ${chk("legacyOutput", t("레거시 출력"), t("3.8 이하 런타임이 읽는 옛 atlas 형식으로 씁니다. 3.8 로 내보내면 꺼져 있어도 옛 형식으로 씁니다"))}
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
            // 알파 프리멀티플라이와 블리드는 하나만 켠다
            const other = k === "premultiplyAlpha" ? "bleed" : k === "bleed" ? "premultiplyAlpha" : null;
            if (other && el.checked) {
              s[other] = false;
              const o = body.querySelector<HTMLInputElement>(`input[data-k="${other}"]`);
              if (o) o.checked = false;
            }
          }),
      );
      body.querySelectorAll<HTMLSelectElement>("select[data-k]").forEach(
        (el) =>
          (el.onchange = () => {
            (s as any)[el.dataset.k!] = el.value;
            if (el.dataset.k === "packing") s.grid = el.value === "grid";
            if (el.dataset.k === "outputFormat") render(); // JPEG 품질 칸을 보이거나 숨긴다
          }),
      );
      const quality = body.querySelector<HTMLInputElement>("[data-jpeg-quality]");
      if (quality)
        quality.onchange = () => {
          const v = Number(quality.value.replace(",", "."));
          if (quality.value.trim() !== "" && Number.isFinite(v)) s.jpegQuality = Math.round(Math.min(1, Math.max(0, v)) * 100) / 100;
          quality.value = String(s.jpegQuality);
        };
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
            // 배율이 생기거나 없어진 칸은 리샘플링 선택을 켜고 끈다
            const pick = body.querySelector<HTMLElement>(`select[data-resample="${i}"]`)?.closest(".select");
            if (pick && i > 0 && pick.classList.contains("disabled") === scales[i] > 0) render();
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
      body.querySelectorAll<HTMLSelectElement>("select[data-resample]").forEach(
        (el) =>
          (el.onchange = () => {
            const methods = [0, 1, 2, 3, 4].map((k) => s.scaleResampling[k] ?? DEFAULT_RESAMPLING);
            methods[Number(el.dataset.resample)] = el.value;
            s.scaleResampling = methods.slice(0, Math.max(1, s.scale.length));
          }),
      );
    }

    // 칸 자리를 그대로 두되, 저장할 때는 배율이 있는 칸까지만.
    function compactScales(scales: number[], suffixes: string[]) {
      let n = 5;
      while (n > 0 && !(scales[n - 1] > 0) && !suffixes[n - 1]) n--;
      s.scale = scales.slice(0, n);
      s.scaleSuffix = suffixes.slice(0, n);
      s.scaleResampling = [0, 1, 2, 3, 4].map((k) => s.scaleResampling[k] ?? DEFAULT_RESAMPLING).slice(0, Math.max(1, n));
    }

    /** 칸별 오류. */
    function errors(): Record<string, string> {
      const e: Record<string, string> = {};
      if (s.maxWidth < 16) e.maxWidth = t("16 이상이어야 합니다.");
      if (s.maxHeight < 16) e.maxHeight = t("16 이상이어야 합니다.");
      if (s.minWidth > s.maxWidth) e.minWidth = t("최대 폭보다 클 수 없습니다.");
      if (s.minHeight > s.maxHeight) e.minHeight = t("최대 높이보다 클 수 없습니다.");
      const used = s.scale.map((v, i) => [v, s.scaleSuffix[i] ?? ""] as const).filter(([v]) => v > 0);
      if (!used.length) e.scale = t("크기 조정을 하나 이상 적으세요(예: 1).");
      const sfx = used.map(([, x]) => x);
      if (new Set(sfx).size !== sfx.length) e.suffix = t("배율마다 다른 접미사를 적으세요(예: @2x).");
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
      const labels: Record<string, string> = { minWidth: t("최소 폭"), minHeight: t("최소 높이"), maxWidth: t("최대 폭"), maxHeight: t("최대 높이"), scale: t("크기 조정"), suffix: t("접미사") };
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
      if (err) return toast("error", t("팩 설정을 확인하세요"), err);
      // 빈 배율 칸은 빼고 저장
      const kept = s.scale.map((v, i) => [v, s.scaleSuffix[i] ?? "", s.scaleResampling[i] ?? DEFAULT_RESAMPLING] as const).filter(([v]) => v > 0);
      close({ pack: { ...s, scale: kept.map(([v]) => v), scaleSuffix: kept.map(([, x]) => x), scaleResampling: kept.map(([, , m]) => m) }, unity });
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
      if (err) return toast("error", t("팩 설정을 확인하세요"), err);
      const path = await saveDialog({ title: t("팩 설정 저장"), defaultPath: "pack.json", filters: [{ name: t("Spine 팩 설정"), extensions: ["json"] }] });
      if (!path) return;
      try {
        await invoke("save_pack_settings", { path, settings: s });
        toast("info", t("팩 설정을 저장했습니다"), path);
      } catch (e) {
        toast("error", t("저장할 수 없습니다"), String(e));
      }
    };
    back.querySelector<HTMLElement>('[data-act="load"]')!.onclick = async () => {
      const path = await openDialog({ title: t("팩 설정 불러오기"), multiple: false, filters: [{ name: t("Spine 팩 설정"), extensions: ["json"] }] });
      if (typeof path !== "string") return;
      try {
        s = await invoke<PackSettings>("load_pack_settings", { path });
        if (s.premultiplyAlpha) s.bleed = false;
        s.outputFormat = s.outputFormat.toLowerCase() === "jpg" || s.outputFormat.toLowerCase() === "jpeg" ? "jpg" : "png";
        // 불러온 확장자가 Unity 형식이면 Unity 를 켠다
        unity = s.atlasExtension === UNITY_ATLAS_EXT;
        if (!unity) extBeforeUnity = s.atlasExtension;
        render();
        toast("info", t("팩 설정을 불러왔습니다"), path);
      } catch (e) {
        toast("error", t("불러올 수 없습니다"), String(e));
      }
    };
    back.addEventListener("keydown", (e) => {
      const tgt = e.target as HTMLElement;
      if (e.key === "Escape") {
        e.stopPropagation();
        close(null);
      } else if (e.key === "Enter" && tgt.tagName !== "BUTTON" && tgt.tagName !== "SELECT" && !tgt.classList.contains("sl-track")) {
        e.preventDefault();
        tgt.blur?.(); // 입력 중인 값을 먼저 반영
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
