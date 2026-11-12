//! 명령줄 내보내기. 내용은 [`spine_export::cli`] 에 있다(앱 실행 파일도 같은 것을 쓴다).
//!
//!   spine-export-cli export 입력.spine 출력폴더 --version 4.2.43 [--format json|binary|both] [--atlas] ...
//!   spine-export-cli list 입력.spine [--json]

use std::process::ExitCode;

fn main() -> ExitCode {
    spine_export::cli::run(std::env::args().skip(1).collect())
}
