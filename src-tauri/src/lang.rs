//! 백엔드가 화면에 보내는 글의 언어. 프런트엔드가 `set_menu_language` 로 알려 준 언어(`ko`/`en`)를 따른다.

use std::sync::atomic::{AtomicBool, Ordering};

static ENGLISH: AtomicBool = AtomicBool::new(false);

pub fn set(lang: &str) {
    ENGLISH.store(lang == "en", Ordering::Relaxed);
}

/// 지금 언어의 글.
pub fn tr(ko: &str, en: &str) -> String {
    if ENGLISH.load(Ordering::Relaxed) { en } else { ko }.to_string()
}
