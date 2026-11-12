// 윈도우 릴리스 빌드에서 콘솔 창을 띄우지 않는다.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::ExitCode;

fn main() -> ExitCode {
    // `spine-faster-export export …` · `list …` · `help`: 창 없이 명령줄로 동작한다.
    // 그 밖의 인수(윈도우 파일 연결로 들어온 .spine 경로 등)는 앱이 받는다.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| spine_export::cli::is_cli_command(a)) {
        #[cfg(windows)]
        attach_console();
        return spine_export::cli::run(args);
    }
    spine_faster_export_lib::run();
    ExitCode::SUCCESS
}

/// 윈도우 릴리스 빌드는 GUI 앱이라 콘솔이 없다. 출력이 파이프·파일로 넘겨지지 않았으면
/// 실행한 터미널의 콘솔에 붙어 그곳에 쓴다(터미널은 끝나기를 기다리지 않으므로, 결과가 필요하면 출력을 파이프로 받는다).
#[cfg(windows)]
fn attach_console() {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetStdHandle(n: u32) -> isize;
        fn AttachConsole(pid: u32) -> i32;
    }
    const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
    const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
    // SAFETY: 인수 없는 Win32 호출이고, 실패해도 출력이 버려질 뿐이다.
    unsafe {
        let h = GetStdHandle(STD_OUTPUT_HANDLE);
        if h == 0 || h == -1 {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}
