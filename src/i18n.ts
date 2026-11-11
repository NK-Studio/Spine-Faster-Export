// 화면 문구 번역. 한국어 원문을 키로 쓴다(gettext 방식): `t("내보내기")`.
// - `{name}` 자리는 `t("{name} 읽는 중…", { name })` 처럼 채운다.
// - 같은 한국어가 뜻이 다르면 `"스켈레톤|file"` 처럼 `|` 뒤에 구분자를 붙인다(한국어로는 `|` 앞만 보인다).
// - 새 언어는 `Lang` 과 `DICTS` 에 사전을 하나 더 넣으면 된다. 사전에 없는 문구는 한국어로 보인다.
// 메뉴 막대 문구는 Rust(`src-tauri/src/lib.rs` 의 `MenuText`)에 있다.

export type Lang = "ko" | "en";
export type LangPref = "system" | Lang;

/** 설정 창의 언어 목록(이름은 그 언어로). */
export const LANGS: [Lang, string][] = [
  ["ko", "한국어"],
  ["en", "English"],
];

const en: Record<string, string> = {
  // 설정 창
  "설정": "Settings",
  "일반": "General",
  "테마": "Theme",
  "언어": "Language",
  "시스템": "System",
  "다크": "Dark",
  "라이트": "Light",
  "저장": "Save",
  "취소": "Cancel",
  "닫기": "Close",
  "화면 모양": "Appearance",
  "앱 화면과 메뉴에 쓸 언어": "Language for the app window and menus",
  "시스템: 운영체제 설정을 따릅니다": "System: follows your OS setting",
  "언어를 바꾸면 창을 다시 그립니다. 열린 프로젝트는 그대로 둡니다.": "Changing the language redraws the window. The open project stays open.",
  "내보내는 중에는 언어를 바꿀 수 없습니다.": "The language can't be changed while exporting.",

  // 화면 뼈대
  "입력": "Input",
  "프로젝트": "Project",
  ".spine 파일을 이 창에 끌어 놓거나 열기를 누르세요": "Drop a .spine file on this window or click Open",
  "열기 ({mod}O)": "Open ({mod}O)",
  "열기…": "Open…",
  "다시 읽기 ({mod}R)": "Reload ({mod}R)",
  "다시 읽었습니다": "Reloaded",
  "{name} · 스켈레톤 {n}개": "{name} · {n} skeletons",
  "모두 체크/해제": "Check/uncheck all",
  "스켈레톤": "Skeletons",
  "체크 뒤집기": "Invert checks",
  "스켈레톤 검색 ({mod}F)": "Search skeletons ({mod}F)",
  "Spine 의 내보내기 대화상자에서 저장(Save)한 설정 JSON 을 불러옵니다": "Load a settings JSON saved from Spine's Export dialog",
  "설정 불러오기…": "Load Settings…",
  "지금 설정을 Spine 내보내기 설정 JSON 으로 저장합니다(Spine 의 Load · CLI --export 로 읽을 수 있습니다)":
    "Save the current settings as a Spine export settings JSON (readable by Spine's Load and CLI --export)",
  "설정 저장…": "Save Settings…",
  "출력 폴더 열기": "Open Output Folder",

  // 스켈레톤 목록
  "내보냄": "Exported",
  "실패": "Failed",
  "내보내기 체크": "Export check",
  ".spine 파일을 끌어 놓으세요": "Drop a .spine file here",
  "파인더·탐색기의 «다음으로 열기»도 됩니다": "“Open With” in Finder or Explorer works too",
  "맞는 스켈레톤이 없습니다.": "No matching skeletons.",
  "스켈레톤이 없는 프로젝트입니다.": "This project has no skeletons.",
  "내보내는 중…": "Exporting…",
  "내보내기": "Export",
  "내보내기 ({mod}E)": "Export ({mod}E)",
  "<b>{n}</b>줄 선택": "<b>{n}</b> rows selected",
  "선택한 줄을 모두 체크 (Space)": "Check all selected rows (Space)",
  "체크": "Check",
  "선택한 줄을 모두 해제 (Space)": "Uncheck all selected rows (Space)",
  "해제": "Uncheck",
  "클릭 선택 · <kbd>{mod}</kbd>/<kbd>Shift</kbd> 여러 줄": "Click to select · <kbd>{mod}</kbd>/<kbd>Shift</kbd> for multiple",

  // 설정 탭
  "출력": "Output",
  "스켈레톤 데이터": "Skeleton Data",
  "텍스처 아틀라스": "Texture Atlas",
  "내보내기 설정": "Export settings",
  "끔": "Off",

  // 툴팁
  "내보낸 파일을 둘 폴더": "Folder for exported files",
  "게임의 Spine 런타임 버전과 맞추세요": "Match your game's Spine runtime version",
  "JSON: 읽기 쉬움 · 바이너리: 작고 빠름": "JSON: readable · Binary: small and fast",
  "에디터로 다시 가져올 때만 쓰는 데이터. 런타임에는 필요 없습니다": "Data used only when importing back into the editor. Not needed at runtime",
  ".skel.bytes 는 Unity 용": ".skel.bytes is for Unity",
  "JSON 을 쓰는 문법(따옴표 생략 정도)": "JSON syntax to write (how much quoting to omit)",
  "스켈레톤 JSON 파일의 확장자": "Extension for skeleton JSON files",

  // 런타임 버전
  "프로젝트를 열면 그 프로젝트의 Spine 버전으로 내보냅니다.": "Once a project is open, it is exported with that project's Spine version.",
  "«{v}»: 버전 형식이 아닙니다. 4.2.43처럼 적으세요.": "“{v}” is not a version. Enter something like 4.2.43.",
  "{v} 버전은 지원하지 않습니다. 지원하는 버전: {sup}.": "Version {v} is not supported. Supported versions: {sup}.",
  "이 프로젝트의 버전({proj})은 지원하지 않습니다. 지원하는 버전: {sup}. 내보낼 버전을 적으세요(예: 4.2.43).":
    "This project's version ({proj}) is not supported. Supported versions: {sup}. Enter a version to export (e.g. 4.2.43).",
  "프로젝트 버전({proj})으로 내보냅니다.": "Exports with the project version ({proj}).",
  "{v} 버전으로 내보냅니다.": "Exports as version {v}.",
  "프로젝트({proj})와 같은 {mm} 계열이라 차이는 거의 없습니다.": "Same {mm} line as the project ({proj}), so there is almost no difference.",
  "프로젝트({proj})보다 높은 {v} 버전으로 내보냅니다. 버전마다 기능과 동작이 달라 Spine에서 본 모습과 실제 모습이 다를 수 있습니다.":
    "Exports as {v}, newer than the project ({proj}). Features and behavior differ between versions, so the result may look different from Spine.",
  "프로젝트({proj})보다 낮은 {v} 버전으로 내보냅니다. 버전마다 기능과 동작이 달라 Spine에서 본 모습과 실제 모습이 다를 수 있습니다.":
    "Exports as {v}, older than the project ({proj}). Features and behavior differ between versions, so the result may look different from Spine.",
  " 지원하는 버전: {sup}.": " Supported versions: {sup}.",
  "스켈레톤|file": "skeleton",

  // 출력 탭
  "폴더": "Folder",
  "프로젝트 옆 export": "export next to project",
  "직접 지정": "Custom",
  "프로젝트를 열면 정해집니다": "Set when a project is opened",
  "폴더 고르기": "Choose folder",
  "런타임 버전": "Runtime version",
  "프로젝트 버전 따름": "Follow project version",
  "버전 목록": "Version list",
  "프로젝트 버전으로 되돌리기": "Reset to project version",
  "이 프로젝트": "This project",
  "{mm} 계열": "{mm} line",

  // 스켈레톤 데이터 탭
  "형식": "Format",
  "바이너리": "Binary",
  "둘 다": "Both",
  "비필수 데이터": "Nonessential data",
  "바이너리 확장자": "Binary extension",
  "Unity(spine-unity) 형식이 켜져 있어 .skel.bytes 로 고정됩니다. 팩 설정의 Options 에서 끌 수 있습니다":
    "Fixed to .skel.bytes because Unity (spine-unity) format is on. You can turn it off under Options in Pack Settings",
  "Unity(spine-unity) 형식이라 .skel.bytes 로 고정됩니다.": "Fixed to .skel.bytes for Unity (spine-unity) format.",
  "JSON 형식": "JSON format",
  "JSON: 표준 · JavaScript: 이름의 따옴표를 꼭 필요할 때만 · Minimal: 이름·값 대부분에 따옴표 없음(libgdx 같은 너그러운 파서 필요)":
    "JSON: standard · JavaScript: quote names only when needed · Minimal: no quotes on most names and values (needs a lenient parser such as libgdx)",
  "사람이 읽기 좋게 줄을 나눠 씁니다(파일이 조금 커집니다)": "Adds line breaks and indentation for readability (files get slightly larger)",
  "JSON 확장자": "JSON extension",
  "애니메이션 정리": "Clean up animations",
  "값이 바뀌지 않는 불필요한 키를 지웁니다(프로젝트 파일은 바꾸지 않습니다)": "Removes unneeded keys that don't change values (the project file is not modified)",
  "모두 내보내기": "Export all",
  "Export 체크가 꺼진 첨부도 내보냅니다(Spine 의 Export all)": "Also exports attachments whose Export box is unchecked (Spine's Export all)",
  "경고": "Warnings",
  "내보낸 뒤 알림(건너뛴 파일, 찾지 못한 이미지 등)을 보여 줍니다": "Shows notices after export (skipped files, missing images, etc.)",

  // 텍스처 아틀라스 탭
  "팩": "Pack",
  "내보낼 때 텍스처 아틀라스(atlas + 페이지 이미지)도 함께 만듭니다": "Also builds a texture atlas (atlas + page images) when exporting",
  "어태치먼트": "Attachments",
  "이미지 폴더": "Image folders",
  "어태치먼트: 첨부가 쓰는 이미지만 · 이미지 폴더: 스켈레톤 이미지 폴더의 이미지 모두":
    "Attachments: only images used by attachments · Image folders: every image in the skeleton's images folder",
  "스켈레톤별 아틀라스": "Atlas per skeleton",
  "싱글 아틀라스": "Single atlas",
  "스켈레톤별: 스켈레톤 이름으로 하나씩 · 싱글: 고른 스켈레톤의 이미지를 프로젝트 파일 이름의 atlas 하나에":
    "Per skeleton: one atlas per skeleton name · Single: images of all checked skeletons in one atlas named after the project file",
  "팩 설정…": "Pack Settings…",
  "이미지 폴더의 PNG 를 모두 묶습니다{sub}. 폴더 안 pack.json 은 읽지 않습니다.": "Packs every PNG in the images folder{sub}. pack.json files in the folder are not read.",
  "(하위 폴더마다 페이지를 따로 씁니다)": " (separate pages per subfolder)",
  "첫 번째로 고른 스켈레톤 기준": "Based on the first checked skeleton",
  "만들 파일": "Output files",

  // Spine 내보내기 설정 파일
  "Spine 내보내기 설정 불러오기": "Load Spine Export Settings",
  "Spine 내보내기 설정": "Spine export settings",
  "출력 폴더도 적용": "Also use output folder",
  "설정에 적힌 출력 폴더: {p}": "Output folder in the settings: {p}",

  // 프로젝트에 저장된 내보내기 설정
  "내보내기 설정을 가져올까요?": "Import the export settings?",
  "예": "Yes",
  "아니오": "No",
  "내보내기 설정을 가져왔습니다": "Imported the export settings",
  "프로젝트의 내보내기 설정을 읽지 못했습니다": "Couldn't read the project's export settings",
  "Spine {kind} 내보내기 설정을 불러왔습니다": "Loaded Spine {kind} export settings",
  "설정을 불러올 수 없습니다": "Couldn't load the settings",
  "Spine 내보내기 설정 저장": "Save Spine Export Settings",
  " 형식이 «둘 다»라 바이너리 설정으로 저장했습니다(Spine 설정 파일은 한 형식만 담습니다).":
    " The format is “Both”, so the binary settings were saved (a Spine settings file holds only one format).",
  "Spine 내보내기 설정을 저장했습니다": "Saved Spine export settings",
  "설정을 저장할 수 없습니다": "Couldn't save the settings",
  "출력 폴더": "Output Folder",
  "출력 폴더 경로": "Output folder path",
  "Spine 프로젝트 파일(.spine)의 경로를 적으세요.": "Enter the path of a Spine project file (.spine).",

  // 상태 줄·열기
  "포맷 {v} · 스켈레톤 {n}개 · {ms}ms": "Format {v} · {n} skeletons · {ms}ms",
  "열린 프로젝트 없음": "No project open",
  "{name} 읽는 중…": "Reading {name}…",
  "스켈레톤 뒤 편집기 상태 일부를 읽지 못했습니다": "Couldn't read part of the editor state after the skeletons",
  "내보내기에는 영향이 없습니다: {e}": "Export is not affected: {e}",
  "{name} 을(를) 열 수 없습니다": "Couldn't open {name}",
  "Spine 프로젝트 열기": "Open Spine Project",
  "Spine 프로젝트": "Spine project",
  "출력 폴더가 아직 없습니다": "The output folder doesn't exist yet",
  "내보내면 만들어집니다.": "It will be created when you export.",
  "놓으면 엽니다": "Drop to open",
  ".spine 파일이 아닙니다": "Not a .spine file",
  "Spine 프로젝트 파일만 열 수 있습니다": "Only Spine project files can be opened",
  "Spine 프로젝트 파일(.spine)을 끌어 놓으세요.": "Drop a Spine project file (.spine).",

  // 내보내기
  "외 {n}개": "and {n} more",
  "내보내기 결과": "Export Results",
  "{ok}개 성공 · {fail}개 실패 · {s}초": "{ok} succeeded · {fail} failed · {s}s",
  "폴더 열기": "Open Folder",
  "지원하지 않는 버전입니다": "Unsupported version",
  "파일 {n}개를 덮어씁니다": "{n} files will be overwritten",
  "출력 폴더에 같은 이름의 파일이 이미 있습니다.": "Files with the same names already exist in the output folder.",
  "덮어쓰고 내보내기": "Overwrite and Export",
  "내보내는 중… {done}/{total}": "Exporting… {done}/{total}",
  "내보냈습니다": "Exported",
  "내보내지 못했습니다": "Export failed",
  "{n}개를 내보내지 못했습니다": "{n} failed to export",
  "알림 {n}개": "{n} notices",
  "자세히": "Details",
  "출력 폴더를 열 수 없습니다": "Couldn't open the output folder",
  "눌러서 Finder 에서 열기": "Click to show in Finder",
  "눌러서 탐색기에서 열기": "Click to show in Explorer",
  "마지막 내보내기: {ok}개 성공 · {fail}개 실패 · {s}초": "Last export: {ok} succeeded · {fail} failed · {s}s",
  "내보내기를 시작할 수 없습니다": "Couldn't start the export",

  // 팩 설정(packer.ts)
  "여백 {x},{y}": "Padding {x},{y}",
  "회전": "Rotation",
  "4의 배수": "Multiple of 4",
  "정사각형": "Square",
  "배율 {s}": "Scale {s}",

  // 팩 설정 대화상자(Spine 한국어 화면의 이름을 따른다)
  "텍스처 패커 설정": "Texture Packer Settings",
  "영역": "Regions",
  "화이트스페이스 X 제거": "Strip whitespace X",
  "화이트스페이스 Y 제거": "Strip whitespace Y",
  "에일리어스": "Alias",
  "빈 이미지 무시": "Ignore blank images",
  "알파 임계값": "Alpha threshold",
  "영역 패딩": "Region Padding",
  "패딩 X": "Padding X",
  "패딩 Y": "Padding Y",
  "에지 패딩": "Edge padding",
  "패딩 복제": "Duplicate padding",
  "페이지": "Pages",
  "최소 폭": "Min width",
  "최소 높이": "Min height",
  "최대 폭": "Max width",
  "최대 높이": "Max height",
  "2의 거듭제곱": "Power of two",
  "4로 나누기": "Divisible by 4",
  "런타임": "Runtime",
  "필터 최소화": "Filter min",
  "필터 최대화": "Filter mag",
  "랩 X": "Wrap X",
  "랩 Y": "Wrap Y",
  "패킹": "Packing",
  "직사각형": "Rectangles",
  "그리드": "Grid",
  "다각형": "Polygons",
  "알파 프리멀티플라이": "Premultiply alpha",
  "블리드": "Bleed",
  "크기 조정": "Scale",
  "접미사": "Suffix",
  "옵션": "Options",
  "아틀라스 확장자": "Atlas extension",
  "하위 폴더 합치기": "Combine subdirectories",
  "경로 병합": "Flatten paths",
  "인덱스 사용": "Use indexes",
  "빠르게": "Fast",
  "디버그": "Debug",
  "레거시 출력": "Legacy output",
  "로드": "Load",
  "확인": "OK",
  "이 알파 이하는 투명으로 보고 잘라냅니다": "Alpha at or below this is treated as transparent and trimmed",
  "영역 사이 가로 여백(픽셀)": "Horizontal padding between regions (pixels)",
  "영역 사이 세로 여백(픽셀)": "Vertical padding between regions (pixels)",
  "페이지 최소 폭": "Minimum page width",
  "페이지 최소 높이": "Minimum page height",
  "페이지 최대 폭": "Maximum page width",
  "페이지 최대 높이": "Maximum page height",
  "pack.json 불러오기": "Load pack.json",
  "pack.json 으로 저장": "Save as pack.json",
  "기본값으로": "Reset to defaults",
  "적용 (Enter)": "Apply (Enter)",
  "닫기 (Esc)": "Close (Esc)",
  "가로 투명 테두리를 잘라냅니다": "Trims transparent edges horizontally",
  "세로 투명 테두리를 잘라냅니다": "Trims transparent edges vertically",
  "더 촘촘하면 90도 돌려 넣습니다": "Rotates regions 90° when that packs tighter",
  "같은 그림은 한 자리를 함께 씁니다": "Identical images share one region",
  "완전히 투명한 이미지는 넣지 않습니다": "Leaves out fully transparent images",
  "페이지 가장자리에도 여백을 둡니다": "Also pads the page edges",
  "여백을 영역 가장자리 픽셀로 채웁니다": "Fills padding with the region's edge pixels",
  "페이지 크기를 2의 거듭제곱으로 맞춥니다": "Makes page sizes powers of two",
  "페이지 크기를 4의 배수로 맞춥니다(일부 압축 텍스처 형식에 필요)": "Makes page sizes multiples of 4 (needed by some compressed texture formats)",
  "페이지를 정사각형으로 맞춥니다": "Makes pages square",
  "축소 필터(atlas filter)": "Minification filter (atlas filter)",
  "확대 필터(atlas filter)": "Magnification filter (atlas filter)",
  "가로 랩(atlas repeat)": "Horizontal wrap (atlas repeat)",
  "세로 랩(atlas repeat)": "Vertical wrap (atlas repeat)",
  "런타임이 텍스처를 올릴 픽셀 형식(atlas format). 파일은 PNG 면 RGBA, JPG 면 RGB 로 저장합니다": "Pixel format the runtime uploads the texture in (atlas format). Files are saved as RGBA for PNG and RGB for JPG",
  "페이지 파일 형식. JPG 는 알파가 없어 투명한 곳을 검정으로 채우고, 알파 프리멀티플라이·블리드와 상관없이 색에 알파를 곱해 씁니다(Spine 과 같음)":
    "Page file format. JPG has no alpha: transparent areas become black and colors are multiplied by alpha regardless of Premultiply alpha and Bleed (same as Spine)",
  "JPEG 품질": "JPEG quality",
  "JPG 품질(0~1). 높을수록 선명하고 파일이 큽니다": "JPG quality (0–1). Higher is sharper and makes larger files",
  "직사각형: 촘촘히 · 그리드: 같은 크기 칸에 차례로 · 다각형: 메시만 쓰는 이미지를 메시 모양대로 더 촘촘히(느림)":
    "Rectangles: tightly packed · Grid: in order in equal-size cells · Polygons: images used only by meshes are packed by mesh shape, tighter but slower",
  "미리 곱한 알파로 저장합니다(atlas pma)": "Saves with premultiplied alpha (atlas pma)",
  "투명 픽셀의 색을 주변 색으로 채웁니다(알파 프리멀티플라이와 함께 쓸 수 없습니다)": "Fills transparent pixels with nearby colors (can't be used with Premultiply alpha)",
  "배율마다 atlas 를 하나씩 만듭니다": "Builds one atlas per scale",
  "배율별 파일 이름 접미사(예: @2x)": "File name suffix per scale (e.g. @2x)",
  "spine-unity 가 가져오는 형식입니다": "The format spine-unity imports",
  "spine-unity 가져오기 형식: atlas 는 {atlas}, 바이너리는 {skel}": "spine-unity import format: {atlas} for atlases, {skel} for binaries",
  "«이미지 폴더»로 묶을 때: 켜면 하위 폴더의 이미지를 같은 페이지에 섞어 묶고, 끄면 폴더마다 페이지를 따로 씁니다. «첨부 이미지»로 묶을 때는 쓰지 않습니다":
    "When packing “Image folders”: on mixes subfolder images on the same pages, off writes separate pages per folder. Not used when packing attachments",
  "영역 이름에서 폴더를 뺍니다": "Removes folders from region names",
  "이름 끝의 _숫자를 index 로 씁니다": "Uses a trailing _number in the name as the index",
  "덜 촘촘하지만 빨리 묶습니다": "Packs faster but less tightly",
  "영역 테두리를 분홍 선으로 그립니다": "Draws region outlines in pink",
  "3.8 이하 런타임이 읽는 옛 atlas 형식으로 씁니다. 3.8 로 내보내면 꺼져 있어도 옛 형식으로 씁니다":
    "Writes the old atlas format read by 3.8 and earlier runtimes. Exporting for 3.8 always uses the old format, even when this is off",
  "16 이상이어야 합니다.": "Must be 16 or more.",
  "최대 폭보다 클 수 없습니다.": "Can't be larger than Max width.",
  "최대 높이보다 클 수 없습니다.": "Can't be larger than Max height.",
  "크기 조정을 하나 이상 적으세요(예: 1).": "Enter at least one Scale (e.g. 1).",
  "배율마다 다른 접미사를 적으세요(예: @2x).": "Use a different Suffix for each scale (e.g. @2x).",
  "팩 설정을 확인하세요": "Check the pack settings",
  "팩 설정 저장": "Save Pack Settings",
  "Spine 팩 설정": "Spine pack settings",
  "팩 설정을 저장했습니다": "Saved pack settings",
  "저장할 수 없습니다": "Couldn't save",
  "팩 설정 불러오기": "Load Pack Settings",
  "팩 설정을 불러왔습니다": "Loaded pack settings",
  "불러올 수 없습니다": "Couldn't load",

  // controls.ts
  "기본값": "Default",
  "(좌우로 끌어 값 바꾸기)": "(drag left/right to change)",
};

const DICTS: Record<Lang, Record<string, string> | null> = { ko: null, en };

function readPref(): LangPref {
  try {
    const v = JSON.parse(localStorage.getItem("sfe.prefs") ?? "{}")?.lang;
    return v === "ko" || v === "en" ? v : "system";
  } catch {
    return "system";
  }
}

/** 시스템이면 OS 언어(한국어면 ko, 아니면 en). */
export function resolveLang(pref: LangPref): Lang {
  if (pref !== "system") return pref;
  return (navigator.languages[0] ?? navigator.language).toLowerCase().startsWith("ko") ? "ko" : "en";
}

/** 이 창이 쓰는 언어. 창을 다시 그려야 바뀐다(설정 창에서 저장하면 다시 그린다). */
export const LANG: Lang = resolveLang(readPref());
document.documentElement.lang = LANG;

const dict = DICTS[LANG];
const warned = new Set<string>();

export function t(key: string, params?: Record<string, string | number>): string {
  let s = dict?.[key];
  if (s === undefined) {
    if (dict && import.meta.env.DEV && !warned.has(key)) {
      warned.add(key);
      console.warn(`[i18n] ${LANG} 번역 없음: ${key}`);
    }
    const bar = key.indexOf("|");
    s = bar >= 0 ? key.slice(0, bar) : key;
  }
  return params ? s.replace(/\{(\w+)\}/g, (m, k: string) => (k in params ? String(params[k]) : m)) : s;
}
