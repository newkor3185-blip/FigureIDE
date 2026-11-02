# FigureIDE

<p align="center">
  <img src="https://github.com/newkor3185-blip/R_vector_studio/blob/main/assets/app-icon/figureide-logo.png" alt="FigureIDE 로고" width="360">
</p>

**FigureIDE는 VectorCraft 기반의 R 그래프 제작·편집 프로젝트입니다.** R 코드를 데스크톱 벡터 편집기 안에서 실행하고, 생성된 그래프를 문서의 벡터 패널로 연결해 관리하는 것을 목표로 합니다.

이 저장소의 애플리케이션은 Rust로 작성된 VectorCraft 코드베이스를 바탕으로 합니다. R 작업 공간은 별도 개발 브랜치에서 진행되고 있으므로, 아래의 R 기능 설명은 해당 브랜치의 구현을 기준으로 합니다.

## 주요 기능

### 벡터 편집기

기본 앱은 아트보드, 벡터 도구, 레이어, 속성 패널, 가져오기·내보내기 기능을 갖춘 VectorCraft 편집기입니다. 데스크톱 앱과 브라우저용 앱이 같은 Rust 코드베이스를 사용합니다.

### R 작업 공간 — `feature/r-execution-engine`

- 앱 안에서 R 스크립트와 여러 스크립트 탭을 관리하고 실행합니다.
- 로컬 R 프로세스를 유지해 콘솔, 코드 실행, 환경 정보를 연결합니다.
- 실행 중 콘솔 출력을 확인하고 실행 요청을 처리합니다.
- Environment, History, Help, Packages 패널을 제공합니다.
- `ggplot` 및 `pheatmap` 그래프 객체를 찾아 벡터 그래프로 문서에 추가합니다.
- 그래프를 원본 R 객체와 연결된 패널로 관리하고, 코드를 다시 실행해 패널을 갱신합니다.
- Graph Inspector에서 그래프 크기, 스타일, 일부 표시·타이포그래피 설정을 조정합니다.
- 연결된 그래프를 갱신할 때 패널 배치와 변환을 보존하고, 안전하게 처리할 수 없는 직접 편집은 보호하는 구조를 개발 중입니다.

R 작업 공간은 일반 그래프 편집기와 달리 R이 설치된 로컬 환경을 사용합니다. R 패키지와 데이터는 사용자의 R 환경에서 제공해야 합니다.

## 개발 상태

이 프로젝트는 활발히 개발 중입니다. R 기능은 기본 브랜치와 별도로 다음 브랜치에서 개발되어 왔습니다.

- `feature/r-execution-engine` — R 실행 엔진과 작업 공간, 그래프 연결 기능
- `feature/r-linked-plot-user-edits` — 연결 그래프의 편집 보존 관련 작업
- `agent/antigravity-ui` — R 편집기 UI 관련 작업

브랜치에 따라 기능과 검증 상태가 다를 수 있습니다. Graph Inspector 변경 또는 크기 변경 후 MA Plot Reflow가 직접 벡터 편집으로 잘못 분류되어 갱신을 막는 문제도 보고되어 있어, 연결 패널 갱신과 편집 보존은 계속 점검해야 합니다. 모든 ggplot2 구성 요소를 개별적으로 자유 편집하는 기능이 완성된 것으로 간주하지 않습니다.

## 시작하기

### 요구 사항

- Rust 1.95 이상
- R: R 작업 공간 기능을 사용할 경우 설치 필요
- Git

R 그래프 코드가 사용하는 패키지(예: `ggplot2`)와 데이터는 별도로 설치·준비해야 합니다.

### 코드 받기 및 실행

```powershell
git clone https://github.com/newkor3185-blip/R_vector_studio.git
cd R_vector_studio
git switch feature/r-execution-engine
cargo run --release -p vectorcraft
```

기본 브랜치에서 벡터 편집기를 실행하려면 `git switch` 명령을 생략할 수 있습니다. R 작업 공간의 최신 기능을 확인하려면 실행 전에 해당 기능 브랜치가 선택되어 있는지 확인하세요.

R 설치 여부는 PowerShell에서 확인할 수 있습니다.

```powershell
Rscript --version
```

빌드, 테스트, 플랫폼별 설정은 저장소의 [개발 안내](https://github.com/newkor3185-blip/R_vector_studio/blob/main/docs/development.md)를 참고하세요.

## 저장소 구조

| 경로 | 내용 |
| --- | --- |
| `apps/vectorcraft` | 데스크톱 애플리케이션 |
| `apps/vectorcraft-cli` | 명령줄 애플리케이션과 자동화 도구 |
| `apps/vectorcraft-web` | 브라우저용 앱 |
| `crates/ui-egui` | 사용자 인터페이스, R 작업 공간과 Graph Inspector |
| `crates/engine` 및 기타 `crates/*` | 문서 모델, 렌더링, 벡터 도구와 파일 형식 |
| `docs/` | 개발·기능·배포 문서 |
| `assets/app-icon` | 애플리케이션 아이콘 자산 |

## 개발 검증

R 실행·연결 패널 변경은 관련 UI 및 R 작업 공간의 집중 검증을 우선합니다. 전체 테스트 실행 여부는 변경 범위와 개발 작업의 검증 지침을 따릅니다.

프로젝트의 릴리스 파이프라인과 버전 관리에 대한 공식 절차는 [릴리스 안내](https://github.com/newkor3185-blip/R_vector_studio/blob/main/docs/releasing.md)를 참고하세요. 현재 GitHub 워크플로는 `release` 브랜치의 변경을 기준으로 여러 플랫폼용 빌드와 초안 릴리스를 생성하도록 설정되어 있습니다.

## 라이선스

저장소의 Cargo workspace는 `MIT OR Apache-2.0` 라이선스를 선언합니다. 개별 브랜드 및 제3자 자산에는 별도 조건이 적용될 수 있으므로 배포 전 저장소의 `LICENSE`와 자산 고지 문서를 확인하세요.

## 링크

- [GitHub 저장소](https://github.com/newkor3185-blip/R_vector_studio)
- [이슈 및 작업 추적](https://github.com/newkor3185-blip/R_vector_studio/issues)
- [개발 문서](https://github.com/newkor3185-blip/R_vector_studio/tree/main/docs)
- [릴리스 안내](https://github.com/newkor3185-blip/R_vector_studio/blob/main/docs/releasing.md)
