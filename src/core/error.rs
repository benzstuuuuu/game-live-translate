//! `AppState.error` (SRS 8.4): why translating stopped and how to fix it.
//! Each code pairs with one cause card on /reader and /setup (FR-ERR-04 to 06).

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    ObsUnreachable,
    CardNotFound,
    CardBlack,
    TranslatorUnavailable,
    /// The profile has no subtitle box yet (FR-ONB-06, M1 checklist).
    NoRegion,
    /// Live Translate refused or dropped the audio session (FR-AUD-07, 08).
    LiveTranslateFailed,
    /// macOS does not let Yokoten hear the card: Microphone is off for the
    /// app that opened it (FR-AUD-08).
    MicPermissionDenied,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppError {
    pub code: ErrorCode,
    /// What went wrong, in Thai.
    pub message: String,
    /// What the player should do before pressing retry, in Thai.
    pub fix: String,
    /// Seconds between tries when Yokoten tries again by itself and goes back
    /// to translating once fixed (FR-ERR-05, 06); absent when only the
    /// player's retry helps.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_s: Option<u64>,
}

impl AppError {
    fn new(code: ErrorCode, message: String, fix: &str) -> Self {
        Self {
            code,
            message,
            fix: fix.to_owned(),
            retry_s: None,
        }
    }

    /// The same problem, tried again by itself every `every`.
    pub fn with_retry(self, every: std::time::Duration) -> Self {
        Self {
            retry_s: Some(every.as_secs().max(1)),
            ..self
        }
    }

    /// The translator cannot work now, e.g. no en → th language pack (FR-TRN-11).
    pub fn translator_unavailable(message: String, fix: &str) -> Self {
        Self::new(ErrorCode::TranslatorUnavailable, message, fix)
    }

    /// OBS closed, WebSocket Server off, or the link dropped (FR-ERR-05).
    pub fn obs_unreachable() -> Self {
        Self::new(
            ErrorCode::ObsUnreachable,
            "ต่อ OBS ไม่ได้".into(),
            "เปิด OBS แล้วเปิด Tools › WebSocket Server Settings › Enable WebSocket server (พอร์ต 4455) แล้วกดลองอีกครั้ง",
        )
    }

    /// OBS answered but its obs-websocket is too old or too new for obws.
    pub fn obs_version(detail: &str) -> Self {
        Self::new(
            ErrorCode::ObsUnreachable,
            format!("OBS เวอร์ชันนี้ใช้กับ Yokoten ไม่ได้ ({detail})"),
            "อัปเดต OBS เป็นเวอร์ชัน 30.2 ขึ้นไป แล้วกดลองอีกครั้ง",
        )
    }

    /// No video source of the card's device name in OBS (FR-CAP-13).
    pub fn card_not_found(device: &str) -> Self {
        Self::new(
            ErrorCode::CardNotFound,
            format!("ไม่เจอ source ของการ์ด “{device}” ใน OBS"),
            "เสียบการ์ด แล้วเพิ่ม Video Capture Device ที่เลือกการ์ดนี้ใน OBS แล้วกดลองอีกครั้ง",
        )
    }

    /// The card exists but scene `yokoten-ocr` is missing or lost it.
    pub fn scene_missing() -> Self {
        Self::new(
            ErrorCode::CardNotFound,
            format!("scene {} ไม่มี source การ์ด", super::obs::OCR_SCENE),
            "เปิด /setup บน Mac แล้วกด “ตั้ง OBS ให้” แล้วกดลองอีกครั้ง",
        )
    }

    /// Scene `yokoten-ocr` shows the card but its crop or other items do not
    /// match the profile, e.g. after switching profile.
    pub fn scene_differs() -> Self {
        Self::new(
            ErrorCode::CardNotFound,
            format!(
                "{} หรือ {} ไม่ตรงกับกรอบของโปรไฟล์นี้",
                super::obs::OCR_SCENE,
                super::obs::CROP_FILTER
            ),
            "เปิด /setup บน Mac แล้วกด “ตั้ง OBS ให้” แล้วกดลองอีกครั้ง",
        )
    }

    /// The profile has no subtitle box to read (FR-ONB-06).
    pub fn no_region(profile: &str) -> Self {
        Self::new(
            ErrorCode::NoRegion,
            format!("โปรไฟล์ “{profile}” ยังไม่มีกรอบ subtitle"),
            "เปิด /setup บน Mac แล้วลากกรอบบนภาพจากการ์ด แล้วกดลองอีกครั้ง",
        )
    }

    /// OBS offers no audio device of the card's name for `Yokoten card audio`
    /// (FR-ONB-07).
    pub fn card_audio_not_found(device: &str) -> Self {
        Self::new(
            ErrorCode::CardNotFound,
            format!("OBS ไม่มีอุปกรณ์เสียง “{device}”"),
            "เสียบการ์ด แล้วกด “ตั้ง OBS ให้” อีกครั้ง",
        )
    }

    /// No audio input of the card's name on this Mac, or it was unplugged
    /// while translating (FR-AUD-01).
    pub fn card_audio_missing(device: &str) -> Self {
        Self::new(
            ErrorCode::CardNotFound,
            format!("ไม่เจออุปกรณ์เสียง “{device}” บน Mac"),
            "เสียบสาย USB ของการ์ดให้แน่น แล้วรอสักครู่ หรือกดลองอีกครั้ง",
        )
    }

    /// The card is there but its audio cannot be read; `why` is CoreAudio's.
    pub fn card_audio_unreadable(device: &str, why: &str) -> Self {
        Self::new(
            ErrorCode::CardNotFound,
            format!("อ่านเสียงจากการ์ด “{device}” ไม่ได้ ({why})"),
            "ถอดการ์ดแล้วเสียบใหม่ แล้วกดลองอีกครั้ง",
        )
    }

    /// Microphone permission is off, so the card's audio cannot be heard
    /// (FR-AUD-08).
    pub fn mic_permission_denied() -> Self {
        Self::new(
            ErrorCode::MicPermissionDenied,
            "macOS ไม่ให้ Yokoten ฟังเสียงจากการ์ด เพราะสิทธิ์ Microphone ถูกปิด".into(),
            "เปิด System Settings › Privacy & Security › Microphone แล้วเปิดสวิตช์ของแอปที่เปิด Yokoten (เช่น Terminal) แล้วกดลองอีกครั้ง",
        )
    }

    /// Program is `yokoten-ocr`, so the overlay has no game scene to go in.
    pub fn program_is_ocr_scene() -> Self {
        Self::new(
            ErrorCode::CardNotFound,
            format!(
                "scene ที่เป็น Program ตอนนี้คือ {} ของ Yokoten",
                super::obs::OCR_SCENE
            ),
            "เลือก scene เกมให้เป็น Program ใน OBS แล้วกด “ตั้ง OBS ให้” อีกครั้ง",
        )
    }

    /// Live Translate refused the session or closed it; `message` says why
    /// in Thai and never holds the key (NFR-07).
    pub fn live_translate_failed(message: String) -> Self {
        Self::new(
            ErrorCode::LiveTranslateFailed,
            message,
            "ตรวจ Gemini API key และอินเทอร์เน็ตใน /setup แล้วกดลองอีกครั้ง",
        )
    }

    /// Card picture stayed black past the limit (FR-CAP-13, FR-ERR-06).
    pub fn card_black() -> Self {
        Self::new(
            ErrorCode::CardBlack,
            "ภาพจากการ์ดดำเกิน 3 วินาที".into(),
            "ปิด HDCP ในเครื่องเกม (PS5: Settings › System › HDMI) หรือถอดการ์ดแล้วเสียบใหม่ แล้วกดลองอีกครั้ง",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_serialize_as_srs_names() {
        let codes = [
            (AppError::obs_unreachable(), "obs_unreachable"),
            (AppError::obs_version("5.0.0"), "obs_unreachable"),
            (AppError::card_not_found("UGREEN 15390"), "card_not_found"),
            (AppError::scene_missing(), "card_not_found"),
            (AppError::card_black(), "card_black"),
            (AppError::scene_differs(), "card_not_found"),
            (AppError::no_region("เกมแรก"), "no_region"),
            (
                AppError::card_audio_not_found("UGREEN 15390"),
                "card_not_found",
            ),
            (AppError::program_is_ocr_scene(), "card_not_found"),
            (
                AppError::card_audio_missing("UGREEN 15390"),
                "card_not_found",
            ),
            (
                AppError::card_audio_unreadable("UGREEN 15390", "busy"),
                "card_not_found",
            ),
            (AppError::mic_permission_denied(), "mic_permission_denied"),
            (
                AppError::live_translate_failed("ปิดการเชื่อมต่อ".into()),
                "live_translate_failed",
            ),
        ];
        for (e, code) in codes {
            let v = serde_json::to_value(&e).unwrap();
            assert_eq!(v["code"], code);
            assert!(!v["message"].as_str().unwrap().is_empty());
            assert!(!v["fix"].as_str().unwrap().is_empty());
        }
    }

    #[test]
    fn retry_is_sent_only_when_yokoten_tries_again_by_itself() {
        let plain = serde_json::to_value(AppError::obs_unreachable()).unwrap();
        assert!(plain.get("retry_s").is_none());
        let e = AppError::obs_unreachable().with_retry(std::time::Duration::from_secs(5));
        assert_eq!(e.retry_s, Some(5));
        assert_eq!(serde_json::to_value(&e).unwrap()["retry_s"], 5);
        assert_eq!(e.code, ErrorCode::ObsUnreachable);
    }

    #[test]
    fn messages_name_what_is_missing() {
        assert!(AppError::card_not_found("UGREEN 15390")
            .message
            .contains("UGREEN 15390"));
        assert!(AppError::obs_version("4.9.1").message.contains("4.9.1"));
        assert!(AppError::scene_missing().message.contains("yokoten-ocr"));
        assert!(AppError::scene_differs().message.contains("yokoten-crop"));
        assert!(AppError::no_region("Genshin").message.contains("Genshin"));
        assert!(AppError::card_audio_not_found("UGREEN 15390")
            .message
            .contains("UGREEN 15390"));
        assert!(AppError::program_is_ocr_scene()
            .message
            .contains("yokoten-ocr"));
        assert!(AppError::card_audio_missing("UGREEN 15390")
            .message
            .contains("UGREEN 15390"));
        let e = AppError::card_audio_unreadable("UGREEN 15390", "busy");
        assert!(e.message.contains("UGREEN 15390") && e.message.contains("busy"));
    }

    #[test]
    fn mic_permission_says_where_to_turn_it_on() {
        let e = AppError::mic_permission_denied();
        assert!(e.message.contains("Microphone"));
        assert!(e
            .fix
            .contains("System Settings › Privacy & Security › Microphone"));
    }
}
