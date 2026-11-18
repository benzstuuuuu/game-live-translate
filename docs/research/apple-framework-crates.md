# ความพร้อมของ crate Apple framework ฝั่ง Rust

Research ของ [issue #4](https://github.com/benzstuuuuu/game-live-translate/issues/4) ตรวจเมื่อ 2026-11-18

**คำถาม:** crate ที่ stack เลือกไว้ (`screencapturekit`, `apple-vision`, `translation-rs`, `foundation-models`, `tauri-nspanel`, `swift-rs`) ใช้ได้จริงกับ requirement เฟสแรกแค่ไหน บน macOS 26–27 และ Tauri 2

**วิธีตรวจ:** อ่านข้อมูลจาก crates.io API และ GitHub API (release, commit, issue, contributor) ดาวน์โหลด source ของแต่ละ repo มาอ่านโค้ด Rust และ Swift bridge ตรง ๆ อ่าน Apple developer docs ของ API ที่ crate ห่อ และรัน Swift สั้น ๆ หนึ่งไฟล์บนเครื่องที่ใช้ทำ research นี้ (macOS 26.5.1, Command Line Tools, SDK 26.0) ไม่ใช่เครื่อง dev จริง (MacBook Air M2, macOS 27) ข้อที่ยังไม่ได้ลองบน macOS 27 กำกับว่า **(ยังไม่ยืนยัน)**

## สรุป

- crate ทั้ง 6 ตัวมีอยู่จริง ชื่อตรงตามที่ stack เขียนไว้ ยังดูแลอยู่ (release ล่าสุดภายใน 6 สัปดาห์) license เป็น MIT OR Apache-2.0 ทุกตัว ไม่มีตัวไหนต้องเปลี่ยน
- 4 จาก 6 ตัว (`screencapturekit`, `apple-vision`, `translation-rs`, `foundation-models`) มาจาก org [doom-fish](https://github.com/doom-fish) ซึ่งมีผู้ดูแลคนเดียว (Per Johansson, `1313`) และดูแล binding ของ Apple framework อีกหลายสิบ repo ตัว `screencapturekit` ผู้ใช้เยอะ ส่วนอีก 3 ตัวอายุราว 6 เดือน มี reverse dependency บน crates.io เป็น 0 และเปลี่ยน API แบบ breaking ทุกไม่กี่สัปดาห์
- ช่องว่างที่เจอส่วนใหญ่มาจากข้อจำกัดของ Apple ไม่ใช่ของ crate:
  - Apple Translation แบบไม่มี UI **ดาวน์โหลดชุดภาษาเองไม่ได้** (FR-TRN-11)
  - Foundation Models จะ throw `unsupportedLanguageOrLocale` เมื่อสั่งให้ตอบเป็นภาษาที่ Apple Intelligence ไม่รองรับ และภาษาไทยไม่อยู่ในรายการ ทำให้ AFM อาจแปลเป็นไทยไม่ได้เลย
- ช่องว่างที่เป็นของ crate จริง:
  - `apple-vision` ไม่เปิดให้ตั้ง `recognitionLanguages`, `customWords` และ `minimumTextHeight`
  - `foundation-models` ยังไม่ห่อ API ใหม่ของ macOS 27 (`variant`, Private Cloud Compute, image attachment)
  - `swift-rs` ไม่รองรับ async หรือ closure
- Core Audio process tap และ AVFoundation เป็น C/Objective-C API ใช้ผ่าน `objc2-core-audio` และ `objc2-av-foundation` ได้เลยโดยไม่ต้องผ่าน `swift-rs` ส่วนที่ยังต้องใช้ `swift-rs` จริงคือ SpeechAnalyzer แบบ live

## ตารางสรุป

| Crate | เวอร์ชันล่าสุด | Commit ล่าสุด | ผู้ดูแล | License | Bridge | ขาดเมื่อเทียบกับ FR | Verdict |
| --- | --- | --- | --- | --- | --- | --- | --- |
| [`screencapturekit`](https://crates.io/crates/screencapturekit) | 11.1.2 (2026-11-02) | 2026-11-02 | doom-fish (`1313`) | MIT OR Apache-2.0 | Swift package ที่ `build.rs` คอมไพล์ + `@_cdecl` | ไม่ขาด ส่วน API ที่ macOS 27 เพิ่มมายังห่อไม่ครบ แต่ไม่ได้ใช้ | ใช้ได้ |
| [`apple-vision`](https://crates.io/crates/apple-vision) | 0.17.2 (2026-10-26) | 2026-10-26 | doom-fish (`1313`) | MIT OR Apache-2.0 | Swift package + `@_cdecl` (ห่อ `VN*` ObjC API) | ตั้ง `recognitionLanguages`, `customWords` และ `minimumTextHeight` ไม่ได้ | ใช้ได้แต่มีช่องว่าง |
| [`translation-rs`](https://crates.io/crates/translation-rs) | 0.5.2 (2026-10-26) | 2026-10-26 | doom-fish (`1313`) | MIT OR Apache-2.0 | Swift package + `@_cdecl` | ดาวน์โหลดชุดภาษาโดยไม่มี UI ไม่ได้ (ข้อจำกัดของ Apple) และต้องมี main run loop | ใช้ได้แต่มีช่องว่าง |
| [`foundation-models`](https://crates.io/crates/foundation-models) | 0.12.2 (2026-10-26) | 2026-10-26 | doom-fish (`1313`) | MIT OR Apache-2.0 | Swift package + `@_cdecl` | ไม่มี `variant`, PCC และ image attachment ของ macOS 27 ส่วนการตอบเป็นภาษาไทยติดข้อจำกัดของ Apple | ใช้ได้แต่มีช่องว่าง |
| [`tauri-nspanel`](https://crates.io/crates/tauri-nspanel) | 2.1.1 (2026-10-08) | 2026-10-25 | ahkohd | MIT OR Apache-2.0 | `objc2` + `objc2-app-kit` (ไม่มี Swift) | ไม่ขาด | ใช้ได้ |
| [`swift-rs`](https://crates.io/crates/swift-rs) | 1.0.9 (2026-10-10) | 2026-10-10 | Brendonovich | MIT OR Apache-2.0 | เป็นตัว bridge เอง (`@_cdecl` + `SwiftLinker`) | ไม่รองรับ async หรือ closure ต้องเขียน C callback เอง | ใช้ได้แต่มีช่องว่าง |

ข้อมูลเวอร์ชัน วันที่ และ owner มาจาก crates.io API (`/api/v1/crates/<name>` และ `/owners`) ส่วนวันที่ commit มาจาก GitHub API (`/repos/<owner>/<repo>/commits`) ณ 2026-11-18

## รายละเอียดราย crate

### 1. `screencapturekit`: จับภาพและเสียง

- **สถานะ:** 11.1.2 ออก 2026-11-02 มี 56 release, ยอดดาวน์โหลด 3.59 ล้าน, 245 stars, ไม่มี issue ค้าง และมี reverse dependency 24 ตัว ([crates.io](https://crates.io/crates/screencapturekit), [repo](https://github.com/doom-fish/screencapturekit-rs))
- **Bridge:** คอมไพล์ Swift package ใน `swift-bridge/` ตอน build แล้วเรียกผ่าน `@_cdecl` โดยใช้ type กลางจาก [`apple-cf`](https://crates.io/crates/apple-cf) (CVPixelBuffer, IOSurface, CMSampleBuffer)
- **API ที่ต้องใช้ (FR-CAP-01, 02, 06) มีครบ:**
  - จับเฉพาะ rect: [`with_source_rect`](https://github.com/doom-fish/screencapturekit-rs/blob/main/src/stream/configuration/dimensions.rs) ห่อ [`SCStreamConfiguration.sourceRect`](https://developer.apple.com/documentation/screencapturekit/scstreamconfiguration/sourcerect)
  - ตั้ง fps: [`with_fps` และ `with_minimum_frame_interval`](https://github.com/doom-fish/screencapturekit-rs/blob/main/src/stream/configuration/captured_frames.rs) ห่อ [`minimumFrameInterval`](https://developer.apple.com/documentation/screencapturekit/scstreamconfiguration/minimumframeinterval)
  - จับหน้าต่างเดียว หรือจับทั้งจอแล้วตัดหน้าต่างของแอปเองออก: [`with_window`, `with_excluding_windows`, `with_excluding_applications`](https://github.com/doom-fish/screencapturekit-rs/blob/main/src/stream/content_filter.rs) ห่อ [`init(desktopIndependentWindow:)`](https://developer.apple.com/documentation/screencapturekit/sccontentfilter/init(desktopindependentwindow:)) และ [`init(display:excludingApplications:exceptingWindows:)`](https://developer.apple.com/documentation/screencapturekit/sccontentfilter/init(display:excludingapplications:exceptingwindows:))
  - เสียงของแอป: [`with_captures_audio`, `with_excludes_current_process_audio`, `with_captures_microphone`](https://github.com/doom-fish/screencapturekit-rs/blob/main/src/stream/configuration/audio.rs) ห่อ [`excludesCurrentProcessAudio`](https://developer.apple.com/documentation/screencapturekit/scstreamconfiguration/excludescurrentprocessaudio) ใช้กับ filter แบบ `with_including_applications` เพื่อจับเสียงเฉพาะ viewer ได้
  - มีตัวอย่าง [Tauri 2](https://github.com/doom-fish/screencapturekit-rs/tree/main/examples/22_tauri_app) ใน repo (ไม่ได้รวมอยู่ใน crate ที่ publish)
- **ช่องว่าง:** [COVERAGE.md](https://github.com/doom-fish/screencapturekit-rs/blob/main/COVERAGE.md) ระบุว่าห่อ macOS 26.5 SDK ครบ 41/41 symbol ส่วน macOS 27 ได้ 41/46 ที่ขาดคือ `SCClipBufferingOutput`, `SCRecordingEditor` และ `SCStream.capturing` ซึ่งเฟสแรกไม่ได้ใช้
- **ข้อสังเกต:** 11.1.2 เพิ่งแก้ปัญหาที่ process ล่ม (SIGILL) เมื่อ ScreenCaptureKit คืนค่ากลับมาโดยไม่มีทั้งภาพและ error ([#164](https://github.com/doom-fish/screencapturekit-rs/issues/164)) จึงควรใช้ ≥ 11.1.2
- **Verdict:** ใช้ได้ คำถามที่เหลือคือหน้าต่าง viewer ที่ native full screen บนจอนอกจับแบบ window filter ได้ไหม ซึ่งเป็นคำถามของ #11 ไม่ใช่ของ crate

### 2. `apple-vision`: OCR

- **สถานะ:** 0.17.2 ออก 2026-10-26 มี 22 release ตั้งแต่ 2026-05-15, ยอดดาวน์โหลด 846, 2 stars, contributor คนเดียว และไม่มี reverse dependency ([crates.io](https://crates.io/crates/apple-vision), [repo](https://github.com/doom-fish/vision-rs)) import ในโค้ดด้วยชื่อ `apple_vision`
- **Bridge:** Swift package + `@_cdecl` ห่อ Objective-C API ตระกูล `VN*` ส่วน Swift-only API (`RecognizeTextRequest` และ `RecognizeDocumentsRequest` ของ macOS 26) [ไม่ได้ห่อ](https://github.com/doom-fish/vision-rs/blob/main/README.md)
- **ที่มี:**
  - `TextRecognizer` ตั้ง recognition level (`Accurate`/`Fast`) และ language correction ได้
  - [`recognize_in_pixel_buffer`](https://github.com/doom-fish/vision-rs/blob/main/src/recognize_text/mod.rs) รับ CVPixelBuffer ของ `apple-cf` โดยตรง จึงส่งเฟรมจาก `screencapturekit` ได้โดยไม่ต้องแปลงเป็น PNG (ทั้งสอง crate ใช้ `apple-cf` 0.11 ตรงกัน)
  - ผลลัพธ์มีข้อความ confidence และ bounding box
- **ที่ขาด (FR "ตั้งภาษาและ recognition level"):** ใน [`runOCR`](https://github.com/doom-fish/vision-rs/blob/main/swift-bridge/Sources/VisionBridge/TextRecognition.swift#L72-L83) bridge สร้าง `VNRecognizeTextRequest` แล้วตั้งแค่ `recognitionLevel` กับ `usesLanguageCorrection` ส่วน [`recognitionLanguages`, `customWords`, `minimumTextHeight` และ `automaticallyDetectsLanguage`](https://developer.apple.com/documentation/vision/vnrecognizetextrequest) ตั้งจาก Rust ไม่ได้ ค่าเริ่มต้นของ Vision เป็นภาษาอังกฤษ ซึ่งพอสำหรับ subtitle ภาษาอังกฤษในเฟสแรก แต่ถ้าจะใส่ชื่อเฉพาะของเกมเป็น `customWords` จะทำผ่าน crate นี้ไม่ได้
- **ทางสำรอง:** `VNRecognizeTextRequest` เป็น Objective-C API จึงไม่ต้องใช้ `swift-rs` ใช้ [`objc2-vision`](https://crates.io/crates/objc2-vision) 0.3.2 ได้เลย crate นี้ generate จาก header และมี `setRecognitionLanguages`, `setCustomWords`, `setMinimumTextHeight` และ `setAutomaticallyDetectsLanguage` ครบ (ตรวจจาก source ของ crate) ทางนี้ต้องเขียนโค้ด unsafe มากขึ้น อีกทางคือส่ง PR เพิ่ม setter ให้ `apple-vision`
- **Verdict:** ใช้ได้แต่มีช่องว่าง พอสำหรับ #11 แต่ถ้า OCR ต้องการ `customWords` หรือ `minimumTextHeight` ให้เปลี่ยนไปใช้ `objc2-vision`

### 3. `translation-rs`: Apple Translation

- **สถานะ:** 0.5.2 ออก 2026-10-26 มี 9 release ตั้งแต่ 2026-05-16, ยอดดาวน์โหลด 367, 0 stars, contributor คนเดียว และไม่มี reverse dependency ([crates.io](https://crates.io/crates/translation-rs), [repo](https://github.com/doom-fish/translation-rs)) import ในโค้ดด้วยชื่อ `translation`
- **Bridge:** Swift package + `@_cdecl` ส่วน [`build.rs`](https://github.com/doom-fish/translation-rs/blob/main/build.rs) weak-link `Translation.framework` และเปิดโค้ด macOS 26 เฉพาะเมื่อ SDK ≥ 26 มี feature `async` เป็น future ที่ไม่ผูกกับ executor ตัวไหน
- **ที่มี:** [COVERAGE.md](https://github.com/doom-fish/translation-rs/blob/main/COVERAGE.md) ห่อทุก public symbol ของ macOS 26.5 SDK และ README ระบุว่า [27.0 ไม่ได้เปลี่ยน](https://github.com/doom-fish/translation-rs/blob/main/README.md):
  - `TranslationSession::new` ห่อ [`init(installedSource:target:)`](https://developer.apple.com/documentation/translation/translationsession/init(installedsource:target:)) ซึ่งเป็น session แบบไม่มี UI (macOS 26+)
  - `LanguageAvailability::status_for_language_pair` ห่อ [`LanguageAvailability`](https://developer.apple.com/documentation/translation/languageavailability) ใช้ตรวจชุดภาษา (FR-TRN-11 ครึ่งแรก)
  - `translate`, `translate_batch`, streaming batch, `prepare_translation`, `can_request_downloads` และ `is_ready`
- **ที่ขาด (FR-TRN-11 ครึ่งหลัง: ดาวน์โหลดชุดภาษา):** เป็นข้อจำกัดของ Apple ไม่ใช่ของ crate
  - Apple docs เขียนว่า session ที่สร้างด้วย `init(installedSource:target:)` ["cannot request downloads"](https://developer.apple.com/documentation/translation/translationsession/canrequestdownloads) และจะ throw `notInstalled` ถ้าชุดภาษายังไม่ได้ติดตั้ง
  - การขออนุญาตดาวน์โหลดต้องใช้ session จาก SwiftUI modifier [`translationTask`](https://developer.apple.com/documentation/translation/translationsession) ร่วมกับ [`prepareTranslation()`](https://developer.apple.com/documentation/translation/translationsession/preparetranslation()) เท่านั้น
- **ผลลองจริง** (macOS 26.5.1 บนเครื่องที่ทำ research ไม่ใช่เครื่อง dev): `LanguageAvailability` รายงาน EN→TH เป็น `supported` (รองรับแต่ยังไม่ติดตั้ง) จาก 21 ภาษา และมีภาษาไทย session แบบ `installedSource` ได้ `canRequestDownloads == false` และ `translate` throw `notInstalled`
- **ข้อบังคับเรื่อง thread:** README ระบุว่า Translation.framework [ทำงานเสร็จบน main queue](https://github.com/doom-fish/translation-rs/blob/main/README.md#threading-and-timeouts) ถ้าเรียกจาก thread อื่นต้องมี main run loop ทำงานอยู่ ไม่อย่างนั้นจะได้ `MainRunLoopNotRunning` ภายใน 10 วินาที และ `#[tokio::main]` ที่ block main thread จะทำให้ Translation ค้าง
  - ใน Tauri ไม่มีปัญหาเพราะ main thread รัน AppKit event loop อยู่แล้ว แต่ต้องเรียก Translation จาก worker thread และห้ามเรียกแบบ sync บน main thread
  - bin ของ spike ที่เป็น CLI ต้องให้ main thread รัน run loop เอง ตอนเขียน Swift ทดสอบใน research นี้ โปรแกรมที่ block main thread ด้วย semaphore ค้างจริง
- **ทางสำรองของการดาวน์โหลด (ยังไม่ยืนยันทั้งสองทาง):**
  - (ก) แจ้งให้ผู้ใช้ไปดาวน์โหลดเองที่ System Settings → General → Language & Region → Translation Languages แล้วใช้ `LanguageAvailability` ตรวจซ้ำ
  - (ข) เขียน Swift bridge เล็ก ๆ ผ่าน `swift-rs` ที่วาง `NSHostingView` พร้อม `.translationTask` และ `prepareTranslation()` ลงในหน้าต่างของแอป เพื่อให้ระบบแสดงหน้าขออนุญาตดาวน์โหลด
- **Verdict:** ใช้ได้แต่มีช่องว่าง การแปลและการตรวจสถานะชุดภาษาใช้ crate นี้ได้ ส่วนการดาวน์โหลดต้องใช้ทาง (ก) หรือ (ข)

### 4. `foundation-models`: Apple Foundation Models (AFM 3)

- **สถานะ:** 0.12.2 ออก 2026-10-26 มี 19 release ตั้งแต่ 2026-05-15, ยอดดาวน์โหลด 3,227, 2 stars, contributor คนเดียว และไม่มี reverse dependency ([crates.io](https://crates.io/crates/foundation-models), [repo](https://github.com/doom-fish/foundation-models-rs)) ต้องเปิด feature `macos_26_0`
- **Bridge:** Swift package + `@_cdecl` เรียก Rust tool callback ผ่าน dispatch queue และมี feature `async`
- **ที่มี:**
  - FR-TRN-06 ตรวจ availability: [`SystemLanguageModel::availability()`](https://github.com/doom-fish/foundation-models-rs/blob/main/swift-bridge/Sources/FoundationModelsBridge/ModelBridge.swift#L43-L60) แยกเหตุผล `deviceNotEligible`, `appleIntelligenceNotEnabled` และ `modelNotReady` ตาม [Apple](https://developer.apple.com/documentation/foundationmodels/systemlanguagemodel)
  - ขนาด context: `context_size()` ห่อ [`contextSize`](https://developer.apple.com/documentation/foundationmodels/systemlanguagemodel/contextsize) และนับ token ได้ด้วย `token_count` (overload บางตัวต้อง macOS 26.4+) ตาม [CHANGELOG 0.12.0](https://github.com/doom-fish/foundation-models-rs/blob/main/CHANGELOG.md)
  - ตรวจภาษาได้ด้วย `supported_languages()` และ `supports_locale()`
  - มี session, instructions, streaming, guided generation (dynamic schema) และ tool calling
- **ที่ขาด:** [COVERAGE.md](https://github.com/doom-fish/foundation-models-rs/blob/main/COVERAGE.md) audit กับ macOS 26.5 SDK ส่วน API ที่ macOS 27 เพิ่มมายังไม่มีใน crate (ตรวจด้วย grep ทั้ง source ไม่พบ):
  - [`SystemLanguageModel.variant`](https://developer.apple.com/documentation/foundationmodels/systemlanguagemodel/variant-swift.property) ใช้บอกว่าเครื่องนี้มี AFM Core หรือ AFM Core Advanced ([Apple](https://developer.apple.com/documentation/foundationmodels/detecting-available-apple-foundation-models))
  - [`PrivateCloudComputeLanguageModel`](https://developer.apple.com/documentation/foundationmodels/privatecloudcomputelanguagemodel)
  - image [`Attachment`](https://developer.apple.com/documentation/foundationmodels/attachment)
- **ข้อจำกัดของ Apple ที่หนักกว่าช่องว่างของ crate:** [Apple docs](https://developer.apple.com/documentation/foundationmodels/supporting-languages-and-locales-with-foundation-models) เขียนว่า framework ตรวจภาษาของ prompt และภาษาที่ prompt สั่งให้ตอบ ถ้าเป็นภาษาที่ไม่รองรับจะ throw [`unsupportedLanguageOrLocale`](https://developer.apple.com/documentation/foundationmodels/languagemodelsession/generationerror/unsupportedlanguageorlocale(_:)) ซึ่ง Apple นิยามว่าเป็น error ที่เกิดเมื่อ "the model is prompted to respond in a language that it does not support"
  - บน macOS 26.5.1 ที่ลองจริง `supportedLanguages` มี 23 ภาษาและไม่มีไทย ส่วน `supportsLocale(th_TH)` ได้ `false`
  - ถ้า macOS 27 ยังไม่มีภาษาไทย การสั่ง AFM ให้แปล EN→TH อาจ throw ทุกครั้ง **(ยังไม่ยืนยันบน macOS 27 และเครื่องที่ทดสอบไม่ได้เปิด Apple Intelligence)**
- **ทางเลือก:**
  - [`fm-rs`](https://crates.io/crates/fm-rs) 0.3.0 (blacktop, MIT, 2026-08-16) มี `PrivateCloudComputeLanguageModel`, `context_size` และ `respond_with_attachments` แล้ว ([model.rs](https://github.com/blacktop/fm-rs/blob/main/crates/fm-rs/src/model.rs)) แต่ยังไม่มี `variant` และมี issue เรื่อง SDK 26.4 ค้างอยู่ 2 ตัว
  - PCC ต้องใช้ managed entitlement [`com.apple.developer.private-cloud-compute`](https://developer.apple.com/documentation/foundationmodels/privatecloudcomputelanguagemodel) ซึ่งโปรเจกต์นี้ไม่มีเพราะใช้ self-signed cert ไม่ได้เป็นสมาชิก Apple Developer Program แบบเสียเงิน
  - ถ้าต้องใช้ `variant` ให้เขียน `@_cdecl` เพิ่มหนึ่งฟังก์ชันผ่าน `swift-rs` หรือส่ง PR
  - สำหรับ spike ใช้คำสั่ง `fm` ของ macOS 27 ได้ตามที่ #10 เขียนไว้
- **Verdict:** ใช้ได้แต่มีช่องว่าง crate พอสำหรับ FR-TRN-06 และการวัดขนาด context แต่การใช้ AFM แปลเป็นไทยอาจติดที่ตัว framework

### 5. `tauri-nspanel`: overlay แบบ NSPanel

- **สถานะ:** 2.1.1 ออก 2026-10-08 ([crates.io](https://crates.io/crates/tauri-nspanel)) ก่อนหน้านี้แจกผ่าน git อย่างเดียว เพิ่งขึ้น crates.io ครั้งแรกที่ 2.1.0 (2026-09-19) commit ล่าสุด 2026-10-25 บน branch `v2.1` มี 428 stars และ issue เปิดอยู่ 2 ตัว (ตัวล่าสุดเป็น RUSTSEC เรื่อง `chrono` ที่ไม่มีคนดูแล) ([repo](https://github.com/ahkohd/tauri-nspanel))
  - มีโปรเจกต์จริงใช้อยู่หลายตัว เช่น Cap, screenpipe, Handy และ Overlayed ตาม [showcase](https://github.com/ahkohd/tauri-nspanel#showcase)
  - docs.rs build ไม่ผ่าน (`doc_status: false`) แต่มีเอกสารที่ [docs.aremu.dev](https://docs.aremu.dev/tauri-nspanel/)
- **ความเข้ากันได้กับ Tauri:** [`Cargo.toml`](https://github.com/ahkohd/tauri-nspanel/blob/v2.1/Cargo.toml) ต้องการ `tauri = "2.8.5"` พร้อม feature `macos-private-api` (ช่วง ≥ 2.8.5 และ < 3) จึงใช้กับ Tauri 2.12.6 ที่เป็นตัวล่าสุด (2026-11-16) ได้ แต่ Tauri 3 อยู่ที่ alpha.6 แล้ว ([releases](https://github.com/tauri-apps/tauri/releases)) ถ้าจะย้ายไป Tauri 3 ต้องรอ nspanel
- **Bridge:** `objc2` 0.6 + `objc2-app-kit` 0.3 ไม่มี Swift
- **API ที่ต้องใช้มีครบ** ([key-types.md](https://github.com/ahkohd/tauri-nspanel/blob/v2.1/docs/key-types.md)):
  - `nonactivating_panel()` สำหรับ panel ที่ไม่แย่ง focus (FR-OVL-04)
  - `full_screen_auxiliary()`, `can_join_all_spaces()` และ `stationary()` สำหรับลอยเหนือแอปที่ full screen
  - `set_ignores_mouse_events` สำหรับไม่รับคลิก
  - `.transparent(true)` ผ่าน `macos-private-api`
  - มี [ตัวอย่าง fullscreen](https://github.com/ahkohd/tauri-nspanel/tree/v2.1/examples/fullscreen)
- **เหตุที่ยังต้องใช้ nspanel:** หน้าต่าง Tauri ปกติยังลอยเหนือแอป full screen ไม่ได้ ([tauri#9557](https://github.com/tauri-apps/tauri/issues/9557) ยังเปิดอยู่)
- **Verdict:** ใช้ได้ ใช้ 2.1.1 จาก crates.io แทน git branch และ pin Tauri ไว้ที่ 2.x

### 6. `swift-rs`: เรียก Swift-only API

- **สถานะ:** 1.0.9 ออก 2026-10-10 (tag `1.0.9` มีแล้ว) ยอดดาวน์โหลด 24 ล้าน มี issue และ PR เปิดอยู่ 24 ตัว ผู้ดูแลหลักคือ Brendonovich ([crates.io](https://crates.io/crates/swift-rs), [repo](https://github.com/Brendonovich/swift-rs))
  - ตัว Tauri 2.12.6 เองก็ depend `swift-rs = "1.0.9"` แต่เฉพาะ target iOS/UIKit ไม่ใช่ macOS ([tauri Cargo.toml](https://github.com/tauri-apps/tauri/blob/dev/crates/tauri/Cargo.toml))
- **วิธี bridge:** เขียน Swift package ที่ export ฟังก์ชันด้วย `@_cdecl` แล้ว `build.rs` ใช้ `SwiftLinker` สั่ง build และ link Swift runtime ส่งข้อมูลข้ามฝั่งด้วย `SRString`, `SRData`, `SRArray` และ `SRObject`
- **ข้อจำกัด:**
  - ไม่รองรับ async function หรือ closure ([#31](https://github.com/Brendonovich/swift-rs/issues/31) เปิดตั้งแต่ 2023) ต้องส่ง C function pointer เป็น callback เอง หรือใช้ semaphore รอผลฝั่ง Swift API อย่าง SpeechAnalyzer ที่ทำงานแบบ async stream จึงต้องเขียน glue เอง
  - สร้าง object ฝั่ง Rust แล้วส่งให้ Swift ไม่ได้
- **Xcode 27:** ใน release build ของ Swift Build (Xcode 27) ฟังก์ชัน `@_cdecl` ที่ไม่ได้ประกาศ `public` จะกลายเป็น symbol local แล้ว link ไม่ผ่าน
  - 1.0.9 แก้เฉพาะส่วน runtime shim ของ swift-rs เอง ([PR #85](https://github.com/Brendonovich/swift-rs/pull/85), [#81](https://github.com/Brendonovich/swift-rs/issues/81))
  - Swift package ที่เราเขียนเองต้องประกาศ `@_cdecl` ทุกตัวเป็น `public`
- **API ไหนต้องใช้ `swift-rs` จริง:**
  - **SpeechAnalyzer:** [Swift-only, macOS 26+](https://developer.apple.com/documentation/speech/speechanalyzer) ต้องใช้ crate [`speech`](https://crates.io/crates/speech) ของ doom-fish 0.9.3 ห่อ SpeechAnalyzer แล้ว แต่[ห่อเฉพาะการวิเคราะห์ทั้งไฟล์](https://github.com/doom-fish/speech-rs/blob/main/COVERAGE.md) ไม่มี live input (`AnalyzerInput` และ `start(inputSequence:)`) งานถอดเสียงแบบ live จึงยังต้องเขียน bridge ผ่าน `swift-rs` เอง
  - **Core Audio process tap:** ไม่ต้องใช้ [`AudioHardwareCreateProcessTap`](https://developer.apple.com/documentation/coreaudio/audiohardwarecreateprocesstap(_:_:)) (macOS 14.2+) และ [`CATapDescription`](https://developer.apple.com/documentation/coreaudio/catapdescription) เป็น C/Objective-C API มีใน [`objc2-core-audio`](https://crates.io/crates/objc2-core-audio) 0.3.2 แล้ว (ตรวจจาก source ของ crate) ระบบจะขอสิทธิ์ system audio recording ตอนเริ่มอัดครั้งแรก ([Apple sample](https://developer.apple.com/documentation/coreaudio/capturing-system-audio-with-core-audio-taps))
  - **AVFoundation:** ไม่ต้องใช้ เป็น Objective-C API มีใน [`objc2-av-foundation`](https://crates.io/crates/objc2-av-foundation) 0.3.2 (`AVCaptureDevice`, `AVCaptureSession`, `AVAudioEngine`)
  - **ทางที่อาจใช้:** download prompt ของ Translation (ทาง (ข) ในหัวข้อ 3) และ `SystemLanguageModel.variant` ถ้าจำเป็น
- **Verdict:** ใช้ได้แต่มีช่องว่าง ใช้เฉพาะ SpeechAnalyzer แบบ live และงานเล็ก ๆ ที่ crate ไม่ครอบคลุม

## ความเสี่ยงข้าม crate

1. **Bus factor ของ doom-fish:** 4 crate หลักมีผู้ดูแลคนเดียว ([org](https://github.com/doom-fish)) และ commit ส่วนใหญ่ของ `screencapturekit` มาจากคนเดียว (656 commit) มี Copilot ช่วยบ้าง ส่วน `apple-vision`, `translation-rs` และ `foundation-models` ยังไม่มีใครบน crates.io depend และมี breaking change ทุก minor
   - รับมือด้วยการ pin เวอร์ชันแบบ `=` และ commit `Cargo.lock`
   - เตรียม fork หรือ vendor ไว้ เพราะ license เป็น MIT OR Apache-2.0 จึงทำได้
2. **Swift toolchain ตอน build:** ทั้ง 4 crate รัน `swift build` ใน `build.rs` ([ตัวอย่าง](https://github.com/doom-fish/translation-rs/blob/main/build.rs)) เครื่อง build จึงต้องมี Xcode หรือ Command Line Tools ที่มี macOS SDK ≥ 26 ถ้า SDK เก่ากว่านั้น โค้ด macOS 26 จะถูกปิดและ API จะคืน error แทน
   - ทุก crate เพิ่งปรับ build ให้เข้ากับ SwiftPM build engine ตัวใหม่ (release 2026-10-26)
   - ยังไม่มีใครทดสอบให้เห็นว่า link ทั้ง 4 crate ใน binary เดียวแล้วไม่ชนกัน เคยมี issue symbol ซ้ำกับ `apple-cf` ([#159](https://github.com/doom-fish/screencapturekit-rs/issues/159)) ซึ่งปิดแล้ว **(ยังไม่ยืนยัน)**
3. **Main run loop:** Translation ต้องการ main queue ส่วน Tauri ใช้ main thread ทำ event loop ต้องห้ามเรียกงานแปลแบบ sync จาก command handler ที่รันบน main thread และใน bin ของ spike ต้องรัน `CFRunLoopRun` บน main thread
4. **macOS 27 coverage:** crate ของ doom-fish audit กับ SDK 26.5 เป็นหลัก ส่วน API ที่ macOS 27 เพิ่มมาบางตัวยังไม่มี (ดูหัวข้อ 1 และ 4) ของที่เฟสแรกต้องใช้จริงขาดแค่ `variant`
5. **Tauri 3:** มี alpha แล้ว `tauri-nspanel` ยังผูกกับ Tauri 2 ให้ pin Tauri ไว้ที่ 2.x ตลอดเฟสแรก

## ผลต่อ spike M0

- **[#10](https://github.com/benzstuuuuu/game-live-translate/issues/10) แปลไทย:**
  - ติดตั้งชุดภาษา EN→TH ผ่าน System Settings ก่อนเริ่ม เพราะ `translation-rs` แบบไม่มี UI ดาวน์โหลดเองไม่ได้
  - เพิ่มคำถามว่า SwiftUI `translationTask` bridge (ทาง (ข)) ทำได้ไหม หรือจะยอมใช้ทาง (ก)
  - ส่วน AFM ให้ทดสอบเป็นอย่างแรกว่าสั่ง "respond in Thai" แล้ว throw `unsupportedLanguageOrLocale` ไหม และให้ log `supportedLanguages` กับ `supportsLocale(th_TH)` บน macOS 27 ถ้า throw ก็ไม่ต้องวัดคุณภาพแล้วตัด AFM ออกจากบทบาทตัวแปลได้เลย
- **[#11](https://github.com/benzstuuuuu/game-live-translate/issues/11) capture และ OCR:**
  - ใช้ `recognize_in_pixel_buffer` เพื่อส่งเฟรมจาก `screencapturekit` แบบไม่ copy
  - ถ้าความแม่นไม่ถึง แล้วต้องการ `customWords` หรือ `minimumTextHeight` ให้ลอง `objc2-vision` เพราะ `apple-vision` ตั้งค่าเหล่านี้ไม่ได้
  - เพิ่มการ build bin ที่ link ทั้ง 4 crate ของ doom-fish พร้อมกัน เพื่อจับปัญหา symbol ชนกันตั้งแต่ M0
- **[#12](https://github.com/benzstuuuuu/game-live-translate/issues/12) overlay:** ไม่ต้องเปลี่ยน ใช้ `tauri-nspanel` 2.1.1 จาก crates.io กับ Tauri 2.12 ตั้ง `nonactivating_panel`, `full_screen_auxiliary`, `can_join_all_spaces` และ `set_ignores_mouse_events(true)` แล้วจับภาพด้วย filter ที่ exclude แอปตัวเอง
- **[#13](https://github.com/benzstuuuuu/game-live-translate/issues/13) เสียง:**
  - Core Audio process tap ทำผ่าน `objc2-core-audio` ได้โดยไม่ต้องใช้ `swift-rs` ให้แก้ข้อความใน ticket
  - เสียงจาก ScreenCaptureKit ใช้ `screencapturekit` ที่มีอยู่แล้ว
  - `swift-rs` ใช้เฉพาะ SpeechAnalyzer แบบ live เพราะ crate `speech` ยังห่อแค่แบบทั้งไฟล์ และ Swift package ต้องประกาศ `@_cdecl` เป็น `public` ถึงจะ build ด้วย Xcode 27 ผ่าน

## สิ่งที่ยังไม่ได้ยืนยัน

- AFM บน macOS 27 (เครื่อง dev) throw `unsupportedLanguageOrLocale` เมื่อสั่งให้ตอบภาษาไทยไหม และ `supportedLanguages` ของ macOS 27 มีไทยไหม (ลองได้เฉพาะบน macOS 26.5.1 ที่ไม่ได้เปิด Apple Intelligence)
- `SystemLanguageModel.variant` ของ M2 RAM 8 GB เป็น AFM Core หรือไม่ และ `contextSize` จริงเท่าไร
- ทาง (ข) ของ Translation (SwiftUI `translationTask` ในแอป Tauri) แสดงหน้าขออนุญาตดาวน์โหลดได้จริงไหม
- link crate ของ doom-fish ทั้ง 4 ตัวใน binary เดียวกับ Tauri แล้ว build ผ่าน และไม่มี Swift symbol ชนกัน
- deep link ที่เปิดหน้า Translation Languages ใน System Settings ได้ตรงหน้า
- Info.plist key ที่ process tap ต้องใช้สำหรับข้อความขอสิทธิ์ (Apple sample บอกแค่ว่าระบบจะขอสิทธิ์)

## แหล่งอ้างอิง

- crates.io API: `https://crates.io/api/v1/crates/<name>`, `/owners`, `/reverse_dependencies` (ดึงเมื่อ 2026-11-18)
- Repo: [doom-fish/screencapturekit-rs](https://github.com/doom-fish/screencapturekit-rs), [doom-fish/vision-rs](https://github.com/doom-fish/vision-rs), [doom-fish/translation-rs](https://github.com/doom-fish/translation-rs), [doom-fish/foundation-models-rs](https://github.com/doom-fish/foundation-models-rs), [doom-fish/speech-rs](https://github.com/doom-fish/speech-rs), [ahkohd/tauri-nspanel](https://github.com/ahkohd/tauri-nspanel), [Brendonovich/swift-rs](https://github.com/Brendonovich/swift-rs), [blacktop/fm-rs](https://github.com/blacktop/fm-rs), [madsmtm/objc2](https://github.com/madsmtm/objc2)
- Apple: [ScreenCaptureKit](https://developer.apple.com/documentation/screencapturekit), [VNRecognizeTextRequest](https://developer.apple.com/documentation/vision/vnrecognizetextrequest), [RecognizeTextRequest](https://developer.apple.com/documentation/vision/recognizetextrequest), [TranslationSession](https://developer.apple.com/documentation/translation/translationsession), [Foundation Models](https://developer.apple.com/documentation/foundationmodels), [SpeechAnalyzer](https://developer.apple.com/documentation/speech/speechanalyzer), [Core Audio taps](https://developer.apple.com/documentation/coreaudio/capturing-system-audio-with-core-audio-taps)
