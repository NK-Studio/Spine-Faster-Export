---
name: quick-spine-export
description: Spine 편집기 없이 .spine 프로젝트를 런타임 파일(JSON · .skel · atlas + PNG)로 내보낸다. Quick Spine 앱의 명령줄을 쓴다. .spine 파일에서 스켈레톤을 내보내거나, spine-unity 용 .skel.bytes · .atlas.txt 를 만들거나, Spine 런타임 버전(3.8 · 4.0 ~ 4.3)을 바꿔 다시 내보내거나, .spine 안의 스켈레톤 목록·저장된 내보내기 설정을 볼 때 쓴다.
---

# Quick Spine 으로 .spine 내보내기

Quick Spine 앱의 실행 파일은 첫 인수가 `list` · `export` · `help` 이면 창을 띄우지 않고 명령줄로 동작한다.
Spine 라이선스나 Spine 편집기가 없어도 된다.

## 1. 실행 파일 찾기

| 환경 | 경로 |
|---|---|
| macOS (Apple Silicon) | `/Applications/Quick Spine.app/Contents/MacOS/spine-faster-export` |
| Windows, 설치 프로그램(.exe) | `%LOCALAPPDATA%\Quick Spine\spine-faster-export.exe` |
| Windows, `.msi` | `C:\Program Files\Quick Spine\spine-faster-export.exe` |
| 저장소에서 직접(개발) | `cargo run --release -p spine-export --bin spine-export-cli --` (저장소 루트에서) |

위 경로에 없으면 `Quick Spine` 설치 폴더에서 `spine-faster-export*` 를 찾는다. 그래도 없으면 사용자에게 앱을 설치했는지 묻는다.
아래에서는 이 경로를 `QS` 로 쓴다. 공백이 있으니 따옴표로 감싼다.

```bash
QS="/Applications/Quick Spine.app/Contents/MacOS/spine-faster-export"
```

Windows 릴리스 빌드는 GUI 앱이다. 결과를 받으려면 출력을 파이프로 받는다. Bash 도구는 원래 파이프로 받는다. PowerShell 에서는 `& $QS list a.spine | Out-String` 처럼 쓴다. 파이프로 받지 않으면 터미널이 끝나기를 기다리지 않는다.

## 2. 순서

1. **살펴보기**: `"$QS" list <프로젝트.spine> --json`
   - 받는 것: `formatVersion`(프로젝트 Spine 버전), `skeletons`(이름 목록), `savedSettings`(Spine 에서 마지막으로 쓴 내보내기 설정, 출력 폴더 포함. 없으면 null), `knownVersions`.
2. **미리 보기**: 같은 옵션에 `--dry-run --json` 을 붙여 실행한다.
   - 쓰기 전에 최종 설정(`options`), 출력 폴더, 고른 스켈레톤, 덮어쓸 파일(`wouldOverwrite`)을 본다.
3. **내보내기**: `"$QS" export <프로젝트.spine> <출력폴더> [옵션] --no-overwrite --json`
   - 종료 코드 `3` 이면 덮어쓸 파일이 있어 아무것도 쓰지 않은 것이다. `wouldOverwrite` 를 사용자에게 보여 주고 덮어써도 되는지 물은 뒤, `--no-overwrite` 없이 다시 실행한다.
   - 사용자가 처음부터 덮어쓰라고 했으면 `--no-overwrite` 를 빼도 된다.
4. **결과 보고**: JSON 의 `results[]` 를 읽는다. 스켈레톤마다 `ok` · `files`(절대 경로) · `notes`(경고) · `error` 가 있다. `failed > 0` 이면 종료 코드 1 이다. 실패한 스켈레톤과 `error` 를 그대로 전하고, `notes` 의 경고도 빠뜨리지 않는다.

## 3. 자주 쓰는 옵션

| 하고 싶은 것 | 옵션 |
|---|---|
| 일부 스켈레톤만 | `--skeleton <이름>` 을 여러 번(이름은 `list` 그대로, 대소문자 구분) |
| 형식 | `--format json` · `binary` · `both`(기본) |
| 런타임 버전 | `--version 4.2.43` (3.8.xx, 4.0.xx ~ 4.3.xx. 빼면 프로젝트 버전) |
| 텍스처 아틀라스 | `--atlas` (+ `--pack-settings pack.json`, `--max-size 2048`, `--pma` / `--bleed`) |
| 아틀라스를 하나로 | `--atlas --pack-target single` (이름은 프로젝트 파일 이름) |
| spine-unity 용 | `--unity` (바이너리 `.skel.bytes`, atlas `.atlas.txt`) |
| Spine 에서 하던 그대로 | `--project-settings` (출력폴더를 빼면 저장된 출력 폴더로) |
| Spine 설정 파일로 | `--export-settings <Spine Export 대화상자에서 Save 한 JSON>` |
| 이미지 폴더 지정 | `--images <폴더>` (빼면 프로젝트에 적힌 경로) |

전체 옵션은 `"$QS" help` 로 본다.

설정은 이 순서로 덮인다(뒤가 이긴다): 기본값 → `--preset` → `--project-settings` → `--export-settings` → 나머지 옵션(적은 순서) → `--unity`.

## 4. 판단 기준

- **Unity 프로젝트에 넣는 경우**(spine-unity): `--format binary --atlas --unity` 가 보통이다. 버전은 프로젝트의 spine-unity 패키지 버전에 맞춘다. 예를 들어 `Packages/manifest.json` 이나 `Packages/com.esotericsoftware.spine.spine-unity/package.json` 의 버전이 4.2.x 면 `--version 4.2.43` 이다.
  - 출력 폴더는 사용자가 정해 준 `Assets/…` 폴더로 한다.
- **바이너리로 낼 수 없는 기능**이 있으면 다르게 끝난다.
  - `--format binary`: 그 스켈레톤이 실패한다(`error` 에 이유).
  - `--format both`: `.skel` 만 건너뛰고 `notes` 에 이유를 적는다.
  - 어느 쪽이든 JSON 으로 내보내자고 제안한다.
- **3.8 로 내보내기**: atlas 는 늘 스켈레톤별로 만들어진다(`--pack-target single` 은 무시된다).
- **덮어쓰기**: 사용자가 만든 파일을 덮을 수 있으니 확인 없이 덮지 않는다. 특히 `--project-settings` 의 저장된 출력 폴더를 쓸 때 조심한다.
- `list` 의 `savedSettings.output` 이 다른 사람의 컴퓨터 경로일 수 있다(예: `/Users/다른사람/…`). 그런 경로는 쓰지 말고 사용자에게 출력 폴더를 묻는다.
  - `--dry-run` 결과에 "출력 폴더가 아직 없어 새로 만듭니다" 가 나오면 경로가 맞는지 확인한다.

## 5. 종료 코드

| 코드 | 뜻 |
|---|---|
| 0 | 모두 성공 |
| 1 | 일부 스켈레톤 실패(`results[].error`) |
| 2 | 사용법·입력 오류(없는 파일, 없는 스켈레톤, 잘못된 옵션 값). `--json` 이면 `{"ok": false, "error": …}` |
| 3 | `--no-overwrite` 인데 덮어쓸 파일이 있음(`wouldOverwrite`) |

## 예

```bash
"$QS" list ~/Art/hero.spine --json
"$QS" export ~/Art/hero.spine ~/Game/Assets/Spine/Hero --skeleton hero --skeleton hero_fx \
  --format binary --atlas --unity --version 4.2.43 --dry-run --json
"$QS" export ~/Art/hero.spine ~/Game/Assets/Spine/Hero --skeleton hero --skeleton hero_fx \
  --format binary --atlas --unity --version 4.2.43 --no-overwrite --json
```
