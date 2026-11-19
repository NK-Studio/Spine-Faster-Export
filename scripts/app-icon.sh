#!/bin/sh
# 꽉 찬 정사각형 그림 하나로 앱 아이콘 전부를 다시 만든다.
#   1. 모서리를 둥글게 깎아 src-tauri/icons/app-icon.png 로 (scripts/app-icon-mask.swift)
#   2. `tauri icon` 으로 icns · ico · 여러 크기 PNG (모바일용 android/ios 폴더는 지운다)
#   3. 내려받기 페이지 아이콘 site/icon.png (256px)
# 사용: sh scripts/app-icon.sh [꽉 찬 정사각형 PNG]
#   경로를 안 주면 원본 src-tauri/icons/app-icon-source.png 로 만든다(RustRover 의 "아이콘 빌드" 실행 구성).
#   다른 그림을 주면 그 그림을 원본 자리에 복사해 둔 뒤 만든다.
set -e
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ICONS="$ROOT/src-tauri/icons"
SOURCE="$ICONS/app-icon-source.png"

if [ -n "$1" ]; then
  [ -f "$1" ] || { echo "그림이 없습니다: $1" >&2; exit 1; }
  cp "$1" "$SOURCE"
fi
[ -f "$SOURCE" ] || { echo "원본 그림이 없습니다: $SOURCE" >&2; exit 1; }
SRC="$SOURCE"

swift "$ROOT/scripts/app-icon-mask.swift" "$SRC" "$ICONS/app-icon.png"
# tauri icon 은 만드는 파일마다 한 줄씩 stderr 에 찍는다. 성공하면 감추고, 실패하면 보여 준다.
LOG="$(mktemp)"
trap 'rm -f "$LOG"' EXIT
(cd "$ROOT" && npx tauri icon "$ICONS/app-icon.png" -o "$ICONS") >"$LOG" 2>&1 || { cat "$LOG" >&2; exit 1; }
rm -rf "$ICONS/android" "$ICONS/ios"
sips -z 256 256 "$ICONS/app-icon.png" --out "$ROOT/site/icon.png" >/dev/null

echo "아이콘을 다시 만들었습니다: $ICONS, site/icon.png"
echo "확인: pnpm tauri build --bundles app && open \"target/release/bundle/macos/Quick Spine.app\""
