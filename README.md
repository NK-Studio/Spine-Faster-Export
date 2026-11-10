# Spine Faster Export

Spine 프로그램 없이 `.spine` 프로젝트에서 고른 스켈레톤만 런타임 파일(JSON · `.skel` · atlas)로 내보내는 데스크톱 앱입니다. Tauri 2 + Rust.

- 지원 플랫폼: **Apple Silicon 맥(aarch64)**, **Windows(x64)**. 인텔 맥은 지원하지 않습니다.
- 내보내기 코어는 Unity 패키지 `com.nkstudio.spine-event-injector` 의 C# 코어(`Editor/Core`)와 파이썬 PoC(`Tools~/export_poc`)를 Rust 로 옮긴 것입니다.

## 쓰는 법

1. `.spine` 파일을 창에 끌어 놓거나, **열기**(⌘O / Ctrl+O), 또는 파인더·탐색기에서 «다음으로 열기».
2. 왼쪽 목록에서 내보낼 스켈레톤을 체크합니다(처음엔 모두 켜짐). 줄 선택과 체크는 따로입니다.
   - 줄 고르기: 클릭 · `⌘/Ctrl+클릭` 더하기 · `Shift+클릭`·`Shift+↑↓` 범위 · 끌어서 범위 · `⌘/Ctrl+A` 모두 · `Esc` 한 줄로
   - 체크: `Space` 또는 목록 아래 [체크]/[해제] 로 고른 줄을 한꺼번에(하나라도 꺼져 있으면 모두 켬). 고른 줄의 체크 상자를 눌러도 고른 줄 모두가 바뀝니다.
3. 오른쪽에서 버전·형식을 고르고, 텍스처 아틀라스는 **팩 설정…** 에서 Spine 의 *Texture Packer Settings* 와 같은 항목으로 정합니다. 그다음 **내보내기**(⌘E / Ctrl+E). 스켈레톤마다 병렬로 처리합니다.

팩 설정의 Options 에 있는 **Unity (spine-unity)** 를 켜면 spine-unity 가 가져오는 형식으로 맞춥니다: atlas 는 `.atlas.txt`, 바이너리는 `.skel.bytes`(끄면 켜기 전 확장자로 돌아갑니다).

슬라이더는 막대를 누르거나 끌고, 라벨을 좌우로 끌거나, 숫자칸에 직접 적어 바꿉니다. 막대에 포커스가 있으면 방향키(Shift 는 10씩)와 휠도 됩니다. 크기 막대(Min/Max width·height)는 2의 거듭제곱 눈금에 붙고, 그 사이 값은 숫자칸에 적습니다.

스켈레톤 데이터 탭에는 Spine 내보내기 대화상자의 **JSON 형식**(JSON · JavaScript · Minimal), **Pretty print**, **JSON 확장자**, **모두 내보내기**(Export all: Export 체크가 꺼진 첨부도 포함), **경고**가 있습니다. 텍스처 아틀라스 탭은 Spine 과 같은 배치입니다: **팩** 체크 · 묶을 이미지(**어태치먼트** · **이미지 폴더**) ~ 단위(**스켈레톤별 아틀라스** · **싱글 아틀라스** = 프로젝트 파일 이름), 그 아래 **팩 설정…**.

아래 **설정 불러오기… / 설정 저장…** 은 Spine 내보내기 대화상자의 Save 로 만든 설정 JSON(`"class": "export-json"` · `"export-binary"`)을 읽고 씁니다. 출력 폴더는 불러온 뒤 알림의 «출력 폴더도 적용»을 눌러야 바뀝니다.

팩 설정 대화상자의 **Load / Save** 는 Spine 이 쓰는 `pack.json` 을 그대로 읽고 씁니다. 출력 형식은 PNG 만 되고, *Packing: Polygons* · *Legacy output* · *Auto scale* 은 지원하지 않습니다. *Combine subdirectories* 는 «이미지 폴더»로 묶을 때만 씁니다(폴더 안 `pack.json` 은 읽지 않습니다). *Format* 은 atlas 의 `format:` 줄만 바꾸고 PNG 는 RGBA 로 저장합니다.

## 구조

| 경로 | 내용 |
|---|---|
| `crates/spine-model` | 버전 중립 스켈레톤 모델(C# `SkeletonModel`) |
| `crates/spine-project` | `.spine` 컨테이너·스키마·객체 그래프 읽기 → 모델 |
| `crates/spine-json` | 모델 → 런타임 JSON (4.0 ~ 4.3) |
| `crates/spine-binary` | 모델 → `.skel`, 애니메이션 정리 |
| `crates/spine-atlas` | 이미지 → atlas + PNG (Spine Texture Packer Settings: 여백·회전·POT·격자·배율 등) |
| `crates/spine-export` | 내보내기 파사드 + `spine-export-cli` |
| `src-tauri` | Tauri 백엔드(열기·병렬 내보내기·파일 연결) |
| `src` | 프런트엔드(바닐라 TS, 디자인 시트 토큰) |

## 글꼴

IBM Plex Sans KR(Regular · Medium · SemiBold · Bold)을 `src/assets/fonts/ibm-plex-sans-kr/` 에 싣습니다. SIL Open Font License 1.1(같은 폴더의 `OFL.txt`).

## 개발

```bash
pnpm install
```

```bash
pnpm tauri dev
```

명령줄로 내보내기(파이썬 PoC 와 같은 옵션):

```bash
cargo run --release -p spine-export --bin spine-export-cli -- 입력.spine 출력폴더 --version 4.2.43 --format both --atlas --pack-settings pack.json
```

Spine 내보내기 설정 JSON 으로:

```bash
cargo run --release -p spine-export --bin spine-export-cli -- 입력.spine 출력폴더 --export-settings binary.export.json
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
