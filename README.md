# Quick Spine

Spine 프로그램 없이 `.spine` 프로젝트에서 고른 스켈레톤만 런타임 파일(JSON · `.skel` · atlas)로 내보내는 데스크톱 앱입니다. Tauri 2 + Rust.

- 지원 플랫폼: **Apple Silicon 맥(aarch64)**, **Windows(x64)**. 인텔 맥은 지원하지 않습니다.
- 내보내기 코어는 Unity 패키지 `com.nkstudio.spine-event-injector` 의 C# 코어(`Editor/Core`)와 파이썬 PoC(`Tools~/export_poc`)를 Rust 로 옮긴 것입니다.

## 쓰는 법

1. `.spine` 파일을 창에 끌어 놓거나, **열기**(⌘O / Ctrl+O), 또는 파인더·탐색기에서 «다음으로 열기».
   - 프로젝트에 Spine 에서 마지막으로 쓴 JSON·바이너리 내보내기 설정이 저장돼 있고 지금 설정과 다르면 «내보내기 설정을 가져올까요?» 를 묻습니다. **예**를 누르면 그 설정과 출력 폴더를 함께 적용합니다.
2. 왼쪽 목록에서 내보낼 스켈레톤을 체크합니다(처음엔 모두 켜짐). 줄 선택과 체크는 따로입니다.
   - 줄 고르기: 클릭 · `⌘/Ctrl+클릭` 더하기 · `Shift+클릭`·`Shift+↑↓` 범위 · 끌어서 범위 · `⌘/Ctrl+A` 모두 · `Esc` 한 줄로
   - 체크: `Space` 또는 목록 아래 [체크]/[해제] 로 고른 줄을 한꺼번에(하나라도 꺼져 있으면 모두 켬). 고른 줄의 체크 상자를 눌러도 고른 줄 모두가 바뀝니다.
3. 오른쪽에서 버전·형식을 고르고, 텍스처 아틀라스는 **팩 설정…** 에서 Spine 의 *Texture Packer Settings* 와 같은 항목으로 정합니다. 그다음 **내보내기**(⌘E / Ctrl+E). 스켈레톤마다 병렬로 처리합니다.
   - **런타임 버전**은 4.0 ~ 4.3 과 3.8 을 고를 수 있습니다. 3.8 도 JSON 과 바이너리(`.skel`) 모두 됩니다(3.8 바이너리는 Spine 3.8.99 가 내보낸 파일과 바이트 단위로 대조했습니다). 3.8 로 내보낼 때는 프로젝트 버전과 상관없이 Spine 3.8 처럼 atlas 를 늘 스켈레톤별로 만듭니다(«싱글 아틀라스» 선택이 보이지 않습니다). Spine 의 3.8 내보내기와 같게 4.x 프로젝트의 베지어 곡선은 직선으로 바꾸고(계단은 그대로), 4.3 프로젝트는 숫자를 4.3 식으로 씁니다. 3.8 프로젝트는 곡선을 그대로 둡니다(Spine 3.8 편집기의 내보내기와 같음). 물리·슬라이더 제약, 시퀀스, Proportional 간격처럼 3.8 에 없는 기능이 있으면 내보내지 않습니다.

팩 설정의 Options 에 있는 **Unity (spine-unity)** 를 켜면 spine-unity 가 가져오는 형식으로 맞춥니다: atlas 는 `.atlas.txt`, 바이너리는 `.skel.bytes`(끄면 `.skel`). JSON 은 늘 표준 JSON(`.json`)으로 씁니다.

슬라이더는 막대를 누르거나 끌고, 라벨을 좌우로 끌거나, 숫자칸에 직접 적어 바꿉니다. 막대에 포커스가 있으면 방향키(Shift 는 10씩)와 휠도 됩니다. 크기 막대(Min/Max width·height)는 2의 거듭제곱 눈금에 붙고, 그 사이 값은 숫자칸에 적습니다.

스켈레톤 데이터 탭에는 **형식**(JSON · 바이너리 · 둘 다), **비필수 데이터**, **애니메이션 정리**, **Pretty print**(JSON), **모두 내보내기**(Export all: Export 체크가 꺼진 첨부도 포함), **경고**가 있습니다. 텍스처 아틀라스 탭은 Spine 과 같은 배치입니다: **팩** 체크 · 묶을 이미지(**어태치먼트** · **이미지 폴더**) ~ 단위(**스켈레톤별 아틀라스** · **싱글 아틀라스** = 프로젝트 파일 이름), 그 아래 **팩 설정…**.

아래 **설정 불러오기… / 설정 저장…** 은 Spine 내보내기 대화상자의 Save 로 만든 설정 JSON(`"class": "export-json"` · `"export-binary"`)을 읽고 씁니다. 출력 폴더는 불러온 뒤 알림의 «출력 폴더도 적용»을 눌러야 바뀝니다.

팩 설정 대화상자의 **Load / Save** 는 Spine 이 쓰는 `pack.json` 을 그대로 읽고 씁니다. 출력 형식은 PNG 와 JPG 입니다(*JPEG quality* 포함). *Auto scale*(자동 배율)을 켜면 Spine 처럼 묶음(폴더)이 한 페이지에 다 들어갈 때까지 배율을 0.01 씩 낮춰 다시 묶고(0.705 는 0.7 부터, 0.703 은 0.69 부터), 고른 배율을 atlas 의 `scale:` 줄에 적습니다. 배율을 0.01 까지 낮춰도 안 들어가면 Spine 처럼 오류로 멈춥니다. *Alias* 는 Spine 처럼 잘라낸 그림이 같아도 Ordinal 순으로 처음 나온 영역과 오프셋 · 원본 크기가 다르면 합치지 않습니다. JPG 는 알파가 없어 Spine 처럼 투명한 곳을 검정으로 채우고 색에 알파를 곱해 씁니다. *Premultiply alpha* · *Bleed* 는 JPG 픽셀을 바꾸지 않고, atlas 의 `pma:` 줄만 설정을 따릅니다. 양자화 표와 4:2:0 크로마 축소도 Spine 과 같아서 풀어 보면 Spine 의 JPG 와 거의 같습니다(인코더 내부 반올림 차이로 몇 단계까지 다를 수 있습니다). *Scale* 의 배율마다 고르는 *Resampling*(Auto · Nearest · Bilinear · Bilinear progressive · Bicubic)은 Spine 과 픽셀 단위로 같은 결과를 냅니다(Spine 4.2 명령줄 패커와 대조). Spine 처럼 Auto 는 Bicubic(넓이 평균)과, Bilinear progressive 는 Bilinear 와 결과가 같습니다. 줄인 크기의 한 변이 0 으로 반올림될 때는 Spine 과 달리 1픽셀로 둡니다(Spine 은 비우거나 오류로 멈춥니다). *Legacy output* 을 켜면 3.8 이하 런타임이 읽는 옛 atlas 형식으로 씁니다(런타임 버전을 3.8 로 고르면 Spine 처럼 꺼져 있어도 옛 형식으로 씁니다). 팩 설정의 *Pretty print*(보기 좋게 쓰기)를 켜면 Spine 처럼 atlas 의 머리말·영역 줄을 탭으로 들이고 `size: 1024, 512` · `bounds: 2, 2, 40, 33` 처럼 띄어 씁니다. 값은 같고 런타임은 둘 다 읽으며, 옛 형식에는 Spine 처럼 쓰지 않습니다. 메시만 쓰는 이미지는 패킹 방식과 상관없이 Spine 처럼 메시 UV 범위로 자릅니다(배율을 적용해도 Spine 과 같은 범위: f32 로, 세로는 아래에서 재서 구합니다). 배율을 적용한 크기도 Spine 처럼 f32 로 계산합니다(255 × 1.3 = 332). *Packing: Rectangles* · *Grid* 는 Spine 과 같은 자리에 놓습니다: 페이지 수·크기, 영역 좌표·회전까지 같고 페이지 그림도 픽셀 단위로 같습니다(Spine 4.2 명령줄 패커와 대조: *Fast* 를 켜고 끈 것, 회전, POT, 정사각형, 4의 배수, 가장자리 여백, 패딩 복제, 여러 페이지). Spine 이 20분 넘게 걸리는 *Fast* 끈 배치도 몇 초 안에 끝납니다. *Packing: Polygons* 는 메시만 쓰는 이미지를 메시가 덮는 모양대로 묶습니다(영역 첨부가 쓰는 이미지는 사각형 그대로이고, 이 방식의 페이지 안 배치는 Spine 과 다릅니다). Spine 처럼 다각형 모양은 사각형보다 1픽셀 가깝게 놓습니다(여백 2 면 모양 사이 빈 픽셀이 1줄 이상). *Combine subdirectories* 는 «이미지 폴더»로 묶을 때만 씁니다. «이미지 폴더»로 묶으면 Spine 처럼 폴더 안 `pack.json` 을 읽습니다: 폴더의 설정은 가장 가까운 윗폴더의 설정(이미지 폴더 자신은 내보내기 설정)에 그 폴더의 `pack.json` 을 덮은 것이고, 폴더마다 페이지를 따로 쓰며 페이지 머리말(`filter` · `format` · `repeat` · `pma` · Legacy output)도 그 폴더 설정을 따릅니다. 합치기를 켠 폴더도 `pack.json` 이 있는 하위 폴더는 따로 묶고, `"ignore": true` 인 폴더는 넣지 않습니다. 폴더마다 배율이 다르면 그 폴더는 자기 접미사의 atlas 에만 들어갑니다. 페이지는 폴더 나무를 깊이 우선(부모 먼저, 이름은 Ordinal 순)으로 씁니다. 새 형식 atlas 는 Spine 처럼 필터가 Nearest, Nearest 면 `filter:` 줄을 쓰지 않습니다. atlas 안 영역은 Spine 처럼 이름 Ordinal 순(대소문자 구분: `Line_root` 가 `bird` 보다, `a10` 이 `a2` 보다 먼저)으로 쓰고, 같은 이름이면 index 순, index 가 없는 것은 그 뒤에 둡니다. «이미지 폴더»의 파일 이름은 NFC 로 바꿔 영역 이름으로 씁니다(macOS 는 한글 파일 이름을 NFD 로 줍니다). *Format* 은 atlas 의 `format:` 줄만 바꾸고, 파일은 PNG 면 RGBA, JPG 면 RGB 로 저장합니다.

## 명령줄

앱 실행 파일의 첫 인수가 `list` · `export` · `help` 이면 창을 띄우지 않고 명령줄로 동작합니다. 앱만 설치하면 되고 Spine 은 없어도 됩니다.

- 맥: `/Applications/Quick Spine.app/Contents/MacOS/spine-faster-export`
- 윈도우: 설치 폴더의 `spine-faster-export.exe`(설치 프로그램은 `%LOCALAPPDATA%\Quick Spine`). GUI 앱이라 결과를 보려면 출력을 파이프로 받습니다(예: PowerShell `| Out-String`).

```bash
QS="/Applications/Quick Spine.app/Contents/MacOS/spine-faster-export"
```

```bash
"$QS" list hero.spine --json
```

```bash
"$QS" export hero.spine out --skeleton hero --format binary --atlas --unity --no-overwrite --json
```

- `list`: 스켈레톤 목록, 프로젝트 버전, `.spine` 에 저장된 마지막 내보내기 설정
- `export`: 스켈레톤마다 병렬로 내보냅니다.
  - `--dry-run`: 쓰지 않고, 최종 설정과 덮어쓸 파일만 보여 줍니다.
  - `--no-overwrite`: 덮어쓸 파일이 있으면 아무것도 쓰지 않습니다(종료 코드 3).
  - `--json`: 결과를 JSON 으로 씁니다.
  - `--project-settings`: Spine 에서 마지막으로 쓴 설정을 그대로 씁니다.
- 전체 옵션은 `help` 로 봅니다. 종료 코드: 0 성공 · 1 일부 실패 · 2 사용법·입력 오류 · 3 덮어쓰기 거부.

### Claude Code 스킬

[`skills/quick-spine-export`](skills/quick-spine-export/SKILL.md) 는 이 명령줄의 사용법을 담은 Claude Code 스킬입니다. Unity 프로젝트 등 어디서든 «hero.spine 을 Unity 용으로 내보내 줘» 같은 요청에 쓰입니다. 개인 스킬 폴더에 링크하거나 복사해 설치합니다.

```bash
ln -s "$PWD/skills/quick-spine-export" ~/.claude/skills/quick-spine-export
```

특정 프로젝트에서만 쓰려면 그 프로젝트의 `.claude/skills/` 에 복사합니다.

## 구조

| 경로 | 내용 |
|---|---|
| `crates/spine-model` | 버전 중립 스켈레톤 모델(C# `SkeletonModel`) |
| `crates/spine-project` | `.spine` 컨테이너·스키마·객체 그래프 읽기 → 모델 |
| `crates/spine-json` | 모델 → 런타임 JSON (3.8, 4.0 ~ 4.3) |
| `crates/spine-binary` | 모델 → `.skel`, 애니메이션 정리 |
| `crates/spine-atlas` | 이미지 → atlas + PNG · JPG (Spine Texture Packer Settings: 여백·회전·POT·격자·배율 등) |
| `crates/spine-export` | 내보내기 파사드 + 명령줄(`cli`, 앱과 `spine-export-cli` 가 함께 씀) |
| `src-tauri` | Tauri 백엔드(열기·병렬 내보내기·파일 연결) |
| `src` | 프런트엔드(바닐라 TS, 디자인 시트 토큰) |
| `skills/quick-spine-export` | 명령줄 사용법 Claude Code 스킬 |

## 글꼴

IBM Plex Sans KR(Regular · Medium · SemiBold · Bold)을 `src/assets/fonts/ibm-plex-sans-kr/` 에 싣습니다. SIL Open Font License 1.1(같은 폴더의 `OFL.txt`).

## 사각형 배치

사각형 배치(`crates/spine-atlas/src/maxrects.rs`)는 [libGDX](https://github.com/libgdx/libgdx) 의 `MaxRectsPacker` · `GridPacker`(Apache License 2.0, Nathan Sweet · Jukka Jylänki 의 MaxRects)를 바탕으로 Spine 의 결과에 맞춰 고친 것입니다.

## JPEG 인코더

JPG 페이지는 [`jpeg-encoder`](https://crates.io/crates/jpeg-encoder)(MIT 또는 Apache-2.0, IJG)로 씁니다. This software is based in part on the work of the Independent JPEG Group.

## 개발

```bash
pnpm install
```

```bash
pnpm tauri dev
```

개발 중에는 같은 명령줄을 `spine-export-cli` 로 돌릴 수 있습니다:

```bash
cargo run --release -p spine-export --bin spine-export-cli -- export 입력.spine 출력폴더 --version 4.2.43 --format both --atlas --pack-settings pack.json
```

## 배포 빌드

```bash
pnpm tauri build --target aarch64-apple-darwin
```

윈도우에서는:

```bash
pnpm tauri build --target x86_64-pc-windows-msvc
```

GitHub Actions(`.github/workflows/release.yml`)가 태그(`v*`)를 밀면 두 플랫폼을 빌드해 초안 릴리스에 올립니다.
