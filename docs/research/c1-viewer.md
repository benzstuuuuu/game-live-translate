# C1: เลือก viewer สำหรับการ์ด UVC บน macOS

- Ticket: [#3 เลือก viewer สำหรับ C1 บน macOS](https://github.com/benzstuuuuu/game-live-translate/issues/3) (map [#1](https://github.com/benzstuuuuu/game-live-translate/issues/1))
- อ้างอิง SRS: หัวข้อ 2 (การต่ออุปกรณ์สำหรับ MVP), FR-CAP-09, FR-OVL-04, FR-GAME-06, NFR-18, NFR-19 (เสนอ: ภาพบน TV หน่วงเพิ่มไม่เกิน 100 ms)
- วันที่: 2026-11-18
- สถานะ: research จากเอกสารและซอร์สโค้ด **ยังไม่มีตัวเลขที่วัดกับฮาร์ดแวร์จริง** เครื่องที่ใช้ทำ research คือ MacBook Air M2 8 GB (Mac14,2) บน macOS 26.5.1 ไม่ใช่ macOS 27 และยังไม่ได้ต่อการ์ด

ป้ายกำกับในเอกสารนี้: **[เอกสาร]** ยืนยันจากเอกสารของผู้ผลิต · **[โค้ด]** อ่านจากซอร์สโค้ด · **[เครื่อง]** ตรวจบนเครื่องที่ใช้ทำ research · **[อนุมาน]** ยังไม่ได้ยืนยัน · **[ต้องวัด]** ต้องวัดกับของจริง

## สรุป

1. **อันดับ 1: [Consolation](https://github.com/centennial-oss/consolation-apple)** (`org.centennialoss.consolation`) เป็น viewer สำหรับการ์ด UVC โดยตรง ฟรีและ open source เลือก resolution กับ fps ได้ ภาพแสดงผ่าน `AVCaptureVideoPreviewLayer` เสียงเล่นด้วย `AVAudioEngine` ใน process ของตัวเอง และปรับ buffer เสียงได้ ใช้ native full screen
2. **อันดับ 2: [OBS Studio](https://github.com/obsproject/obs-studio)** (`com.obsproject.obs-studio`) ใช้ source แบบ Capture Card Device คู่กับ Fullscreen Projector ปรับได้ครบที่สุดและเลือก Monitoring Device เป็น TV ได้ตรง ๆ แต่หนักที่สุดในกลุ่ม และ audio monitoring มี buffer เพิ่ม
3. **QuickTime Player ใช้เป็น baseline วันแรก** เพราะมีอยู่แล้วในเครื่อง แต่เลือก resolution / fps ของ device ไม่ได้ และ HUD ของมันโผล่ทับขอบล่างของภาพตรงที่ subtitle อยู่ ถ้าวัดแล้วได้ 1080p60 และ HUD ไม่ติดในภาพที่จับ ให้ขยับขึ้นเป็นอันดับ 2 แทน OBS ได้ เพราะน่าจะเบากว่า [อนุมาน]
4. **ตัดออก:** VLC (live caching เริ่มต้น 300 ms และเลือกโหมดของ device ไม่ได้), IINA (ไม่รองรับ capture device), แอปของ UGREEN (ไม่พบสำหรับ macOS), Quick Camera (ไม่มีเสียง), Console Link (เสียเงิน ปิดซอร์ส เน้น iPad) ส่วน `ffplay` เก็บไว้ใช้วัดความหน่วงขั้นต่ำของฮาร์ดแวร์เท่านั้น

## บริบทและเกณฑ์

การต่อของ MVP: PS5 → HDMI → การ์ด UGREEN → USB-C → MacBook Air M2 8 GB → USB-C เป็น HDMI → TV ซึ่งเป็นจอนอกจอเดียว viewer ต้องแสดงภาพเต็มจอบน TV และเล่นเสียงจากการ์ดออก TV Yokoten วาง overlay (NSPanel) ทับ viewer จับภาพหน้าต่าง viewer ด้วย ScreenCaptureKit ที่ 2–4 fps และจับเสียงของ viewer ด้วย Core Audio process tap หรือ ScreenCaptureKit (FR-CAP-09, FR-OVL-04)

เกณฑ์ที่ใช้เทียบ: ความหน่วงที่เพิ่ม, RAM และ CPU (NFR-18), เลือก resolution / fps / codec ของ device ได้ไหม, เล่นเสียงออก TV ได้ไหม, bundle ID และชื่อหน้าต่าง (FR-GAME-06), full screen แบบ Space แยกหรือขยายในหน้าต่าง

รุ่นการ์ดและโหมดที่รับได้จริงเป็นงานของ [#2](https://github.com/benzstuuuuu/game-live-translate/issues/2) หน้าร้านเขียนว่า capture ทาง USB-C ได้ 2K@30 MJPEG ส่วนรุ่นพี่น้องอย่าง [UGREEN 25173](https://uk.ugreen.com/products/25173) ระบุว่าต้องปิด HDCP บนเครื่องเกม ช่อง USB-C ไม่รองรับ HDR / VRR และใช้กับ "OBS Studio, XSplit, ..." ได้ [เอกสาร]

## ข้อเท็จจริงฝั่ง macOS ที่ใช้กับทุก viewer

### ScreenCaptureKit จับหน้าต่างที่อยู่ใน full screen Space ได้

- filter แบบ single window ([`SCContentFilter(desktopIndependentWindow:)`](https://developer.apple.com/documentation/screencapturekit/sccontentfilter/init(desktopindependentwindow:))) จับเนื้อหาเต็มของหน้าต่างแม้ถูกบัง อยู่นอกจอ หรือย้ายไปจออื่น เป็นแบบ "display and space independent" และหยุดส่งภาพเมื่อหน้าต่างถูก minimize ([WWDC22: Take ScreenCaptureKit to the next level](https://developer.apple.com/videos/play/wwdc2022/10155/)) [เอกสาร]
- ผลที่ตามมา:
  - viewer จะเป็น native full screen ใน Space แยกหรือเป็นหน้าต่างธรรมดาก็จับได้เหมือนกัน
  - overlay ของ Yokoten ไม่ติดมาในภาพที่จับ เพราะ filter นี้จับเฉพาะหน้าต่าง viewer จึงไม่มีปัญหา OCR อ่านคำแปลไทยของ Yokoten เอง [อนุมานจากเอกสารข้างบน, ต้องยืนยันใน [#11](https://github.com/benzstuuuuu/game-live-translate/issues/11)]
  - แต่ UI ที่วาด **ในหน้าต่าง viewer เอง** จะติดมาด้วยเมื่อมันโผล่ เช่น HUD ของ QuickTime หรือแถบควบคุมของ Consolation [อนุมาน, ต้องวัด]
- [`SCWindow.title`](https://developer.apple.com/documentation/screencapturekit/scwindow/title) และ [`SCRunningApplication.bundleIdentifier`](https://developer.apple.com/documentation/screencapturekit/scrunningapplication/bundleidentifier) ให้ชื่อหน้าต่างและ bundle ID สำหรับจับคู่โปรไฟล์ [เอกสาร]

### เสียง: ScreenCaptureKit กรองได้แค่ระดับแอป

- ScreenCaptureKit ไม่จับเสียงจนกว่าจะเปิด [`capturesAudio`](https://developer.apple.com/documentation/screencapturekit/scstreamconfiguration/capturesaudio) และกรองเสียงได้แค่ระดับแอป เมื่อใช้ single window filter จะได้เสียงทั้งหมดของแอปเจ้าของหน้าต่าง ([WWDC22: Meet ScreenCaptureKit](https://developer.apple.com/videos/play/wwdc2022/10156/), [WWDC22 10155](https://developer.apple.com/videos/play/wwdc2022/10155/)) [เอกสาร]
- Core Audio process tap จับเสียงขาออกของ process ที่ระบุ ใช้ได้ตั้งแต่ macOS 14.2 ต้องมี `NSAudioCaptureUsageDescription` และผู้ใช้ต้องอนุญาต system audio recording ([Capturing system audio with Core Audio taps](https://developer.apple.com/documentation/coreaudio/capturing-system-audio-with-core-audio-taps)) [เอกสาร]
- ผลที่ตามมา: viewer ที่ดีคือ viewer ที่เล่นเสียงจาก process ของตัวเอง ถ้าแอปส่งเสียงผ่าน helper process อื่น ทั้งสองทางจะจับผิดตัว Consolation เล่นเสียงใน process ของตัวเองแน่นอน [โค้ด] ส่วน QuickTime ยังไม่รู้ว่าเสียง monitor ออกจาก process ไหน [ต้องวัด ใน [#13](https://github.com/benzstuuuuu/game-live-translate/issues/13)]

### Full screen บนจอนอก และ overlay

- native full screen ทำให้แอปได้ Space ของตัวเอง ([Use apps in full screen on Mac](https://support.apple.com/guide/mac-help/use-apps-in-full-screen-mchl9c21d2be/mac)) ต้องเปิด "Displays have separate Spaces" ใน Desktop & Dock เพื่อให้แต่ละจอมี Space ของตัวเอง ([Change Desktop & Dock settings](https://support.apple.com/guide/mac-help/change-desktop-dock-settings-mchlp1119/mac)) [เอกสาร]
- NSPanel ของ Yokoten จะอยู่ใน Space เดียวกับแอปที่เต็มจอได้ด้วย [`fullScreenAuxiliary`](https://developer.apple.com/documentation/appkit/nswindow/collectionbehavior-swift.struct/fullscreenauxiliary) ("The window displays on the same space as the full screen window") ร่วมกับ [`canJoinAllSpaces`](https://developer.apple.com/documentation/appkit/nswindow/collectionbehavior-swift.struct/canjoinallspaces) [เอกสาร] ส่วนจะใช้ได้จริงกับ Tauri บน macOS 27 หรือไม่เป็นงานของ [#6](https://github.com/benzstuuuuu/game-live-translate/issues/6) และ [#12](https://github.com/benzstuuuuu/game-live-translate/issues/12)

### เสียงออก TV

- QuickTime และ Consolation เล่นเสียงออก output เริ่มต้นของระบบ จึงต้องตั้ง TV (HDMI ผ่าน adapter) เป็น output ใน System Settings > Sound ([Change the sound output settings](https://support.apple.com/guide/mac-help/change-the-sound-output-settings-mchlp2256/mac)) ซึ่งทำให้เสียงระบบอื่นออก TV ไปด้วย [เอกสาร + อนุมาน] OBS เลือก Monitoring Device แยกได้ (ดูหัวข้อ OBS)

## ตารางเทียบ

| | Consolation | OBS Studio | QuickTime Player | VLC 3.0 | IINA |
|---|---|---|---|---|---|
| bundle ID | `org.centennialoss.consolation` [โค้ด] | `com.obsproject.obs-studio` [โค้ด] | `com.apple.QuickTimePlayerX` [เครื่อง] | `org.videolan.vlc` [โค้ด] | ไม่ได้ตรวจ |
| ชื่อหน้าต่าง | title bar ซ่อน, น่าจะเป็น "Consolation" [อนุมาน] | "Projector - Source: \<ชื่อ source\>" หรือ "Projector - Scene: \<ชื่อ scene\>" [โค้ด] | "Movie Recording" / ไทย "การบันทึกภาพยนตร์" ขึ้นกับภาษาระบบ [เครื่อง] | ไม่ได้ตรวจ | — |
| เลือก resolution / fps / codec | resolution + fps [โค้ด] | resolution, fps, input format, color space, range [โค้ด] | ไม่ได้ เลือกได้แค่ camera, microphone, quality ของไฟล์ที่อัด [เอกสาร] | ไม่ได้ [โค้ด] | ไม่รองรับ capture device |
| เสียงออก TV | `AVAudioEngine` ออก default output, เลือกเสียงของการ์ดให้เอง, buffer 1–64 (เริ่มต้น 8) [โค้ด] | audio monitoring ออก Monitoring Device ที่เลือกได้, AudioQueue 3 × 30 ms [โค้ด] | ปรับ volume control แล้วได้ยินเสียงที่กำลังอัด [เอกสาร] | ผ่าน `input-slave=avaudiocapture://` [โค้ด] | — |
| full screen | native (`toggleFullScreen`) [โค้ด] | Qt `showFullScreen()` บนจอที่เลือก ซึ่งบน macOS ได้ desktop แยก [โค้ด + เอกสาร Qt] | View > Enter Full Screen [เอกสาร] | ค่าเริ่มต้นแบบเก่า ไม่ใช่ native [โค้ด] | — |
| ความหน่วง | preview layer ของระบบ [โค้ด], ตัวเลข [ต้องวัด] | fast path ของ Capture Card Device [โค้ด], ตัวเลข [ต้องวัด] | [ต้องวัด] | live caching เริ่มต้น 300 ms [โค้ด] | — |
| RAM / CPU | น่าจะต่ำ [อนุมาน, ต้องวัด] | สูงสุดในกลุ่ม [อนุมาน, ต้องวัด] | น่าจะต่ำ [อนุมาน, ต้องวัด] | แปลงภาพเป็น BGRA [โค้ด] | — |
| ราคา / ซอร์ส | ฟรี, Apache-2.0, macOS 15+ | ฟรี, GPL | มากับ macOS | ฟรี, GPL | ฟรี, GPL |

## รายละเอียดต่อตัว

### Consolation

- เป็น "no-frills video capture viewer" สำหรับการ์ด UVC บน macOS และ iPadOS ไม่อัด ไม่ stream ไม่ต่อ network ต้องใช้ macOS 15 ขึ้นไป ([README](https://github.com/centennial-oss/consolation-apple/blob/e081cb420795b6ad0ce1384a8c3be3813e2eb286/README.md), [App Store](https://apps.apple.com/us/app/consolation/id1563856788)) [เอกสาร] เวอร์ชันล่าสุด 1.1.x ออกเมษายนถึงพฤษภาคม 2026 ([iTunes lookup](https://itunes.apple.com/lookup?id=1563856788)) [เอกสาร]
- README มีรายการการ์ดที่ทดสอบแล้ว รวม UGREEN รุ่น 40189 ซึ่งได้แค่ 1080p30 รุ่นนั้นไม่ใช่การ์ดของเรา [เอกสาร]
- bundle ID `org.centennialoss.consolation` ([project.pbxproj](https://github.com/centennial-oss/consolation-apple/blob/e081cb420795b6ad0ce1384a8c3be3813e2eb286/Consolation.xcodeproj/project.pbxproj)) [โค้ด]
- **ภาพ:**
  - แสดงด้วย `AVCaptureVideoPreviewLayer` ([MacPreviewView.swift#L11](https://github.com/centennial-oss/consolation-apple/blob/e081cb420795b6ad0ce1384a8c3be3813e2eb286/Consolation/Components/Viewer/MacPreviewView.swift#L11)) [โค้ด]
  - ตั้ง session เป็น `.inputPriority` ([CaptureSessionBackend.swift#L305-L306](https://github.com/centennial-oss/consolation-apple/blob/e081cb420795b6ad0ce1384a8c3be3813e2eb286/Consolation/Capture/Session/CaptureSessionBackend.swift#L305-L306)) แล้วตั้ง `activeFormat` กับ frame duration ตามที่ผู้ใช้เลือก ([CaptureVideoFormatConfiguration.swift#L109-L120](https://github.com/centennial-oss/consolation-apple/blob/e081cb420795b6ad0ce1384a8c3be3813e2eb286/Consolation/Capture/Video/CaptureVideoFormatConfiguration.swift#L109-L120)) จึงบังคับ 1080p60 ได้ถ้าการ์ดมีโหมดนั้น [โค้ด]
  - มี performance stats และเตือนเมื่อ fps ต่ำ ([App Store](https://apps.apple.com/us/app/consolation/id1563856788)) ใช้ยืนยันได้ว่าได้ 60 fps จริง [เอกสาร]
- **เสียง:**
  - เล่น PCM จาก `AVCaptureAudioDataOutput` ผ่าน `AVAudioEngine` และตั้งใจเก็บคิวให้สั้นเพื่อ latency ต่ำ ([CaptureAudioPlayback.swift#L13-L37](https://github.com/centennial-oss/consolation-apple/blob/e081cb420795b6ad0ce1384a8c3be3813e2eb286/Consolation/Capture/Audio/CaptureAudioPlayback.swift#L13-L37)) [โค้ด]
  - ปรับความยาวคิวได้ 1, 2, 4, …, 64 buffer โดยเริ่มต้นที่ 8 ([CaptureAudioUserDefaults.swift#L13-L14](https://github.com/centennial-oss/consolation-apple/blob/e081cb420795b6ad0ce1384a8c3be3813e2eb286/Consolation/Capture/Audio/CaptureAudioUserDefaults.swift#L13-L14)) [โค้ด]
  - เลือก input เสียงของการ์ดเอง และตัดไมค์ในเครื่องออก ([CaptureAudioDeviceSelection.swift](https://github.com/centennial-oss/consolation-apple/blob/e081cb420795b6ad0ce1384a8c3be3813e2eb286/Consolation/Capture/Audio/CaptureAudioDeviceSelection.swift)) [โค้ด]
  - ไม่พบโค้ดเลือก output device จึงออก default output ของระบบ [โค้ด]
  - เสียงเล่นใน process ของ Consolation เอง process tap กับ ScreenCaptureKit จึงจับได้ตรงตัว [โค้ด + อนุมาน]
- **หน้าต่าง:**
  - full screen ใช้ `toggleFullScreen` ซึ่งเป็น native full screen ([ContentViewWindowMac.swift#L84-L91](https://github.com/centennial-oss/consolation-apple/blob/e081cb420795b6ad0ce1384a8c3be3813e2eb286/Consolation/Views/ContentViewWindowMac.swift#L84-L91)) [โค้ด]
  - ตัวเลือก always-on-top ตั้ง level เป็น `.floating` ([#L105](https://github.com/centennial-oss/consolation-apple/blob/e081cb420795b6ad0ce1384a8c3be3813e2eb286/Consolation/Views/ContentViewWindowMac.swift#L105)) ถ้าเปิด overlay ต้องอยู่สูงกว่านั้น [โค้ด]
  - เป็น `WindowGroup` ที่ `.hiddenTitleBar` ([ConsolationApp.swift#L43-L50](https://github.com/centennial-oss/consolation-apple/blob/e081cb420795b6ad0ce1384a8c3be3813e2eb286/Consolation/ConsolationApp.swift#L43-L50)) [โค้ด]
  - แถบควบคุมโผล่เมื่อเมาส์เคลื่อนบนหน้าต่าง ([ContentView.swift#L100](https://github.com/centennial-oss/consolation-apple/blob/e081cb420795b6ad0ce1384a8c3be3813e2eb286/Consolation/Views/ContentView.swift#L100)) และวาดในหน้าต่างเดียวกัน จึงจะติดในภาพที่จับถ้าเมาส์อยู่บน TV [โค้ด + อนุมาน]
- **ความเสี่ยง:**
  - โปรเจกต์เล็กมาก (ราว 12 stars) และยังไม่มีใครวัด latency ให้
  - ถ้าหยุดพัฒนา ซอร์สเป็น Apache-2.0 จึง fork ได้ และเป็นตัวอย่างโค้ดสำหรับ C2 (FR-CAP-10) ได้โดยตรง

### OBS Studio (Capture Card Device + Fullscreen Projector)

- เวอร์ชันล่าสุด [33.0.1](https://github.com/obsproject/obs-studio/releases/tag/33.0.1) (29 ต.ค. 2026) และ bundle ID `com.obsproject.obs-studio` ([helpers.cmake#L57](https://github.com/obsproject/obs-studio/blob/717cfa8b7e633853cd21ad2cc656185b3b4255bb/cmake/macos/helpers.cmake#L57)) [โค้ด]
- **ภาพ:**
  - source แบบ "Capture Card Device" เพิ่มเข้ามาใน [30.1.0](https://github.com/obsproject/obs-studio/releases/tag/30.1.0) ใช้ "fast path" ที่ render ตรงที่ frame rate ของ OBS แทน async frame เพื่อ frame pacing ที่ดีกับเกม ([PR #9344](https://github.com/obsproject/obs-studio/pull/9344)) [โค้ด]
  - ตั้งค่าได้ทั้ง resolution, frame rate, input format, color space และ video range ([plugin-main.m#L79-L131](https://github.com/obsproject/obs-studio/blob/717cfa8b7e633853cd21ad2cc656185b3b4255bb/plugins/mac-avcapture/plugin-main.m#L79-L131), [locale](https://github.com/obsproject/obs-studio/blob/717cfa8b7e633853cd21ad2cc656185b3b4255bb/plugins/mac-avcapture/data/locale/en-US.ini)) [โค้ด]
- **เสียง:**
  - source มี "Enable audio if supported by device" เปิดเป็นค่าเริ่มต้น [โค้ด] ถ้าการ์ดแยกเสียงเป็น device ต่างหาก อาจต้องเพิ่ม Audio Input Capture เอง [ต้องวัด]
  - ตั้ง Audio Monitoring เป็น "Monitor Only (mute output)" และเลือก TV ใน Settings > Advanced > Audio > Monitoring Device ([locale](https://github.com/obsproject/obs-studio/blob/717cfa8b7e633853cd21ad2cc656185b3b4255bb/frontend/data/locale/en-US.ini)) [โค้ด] จึงไม่ต้องเปลี่ยน default output ของระบบ
  - monitoring บน macOS ใช้ AudioQueue 3 buffer ขนาด buffer ละ 30 ms (`samples_per_sec / 100 * 3`) ([coreaudio-output.c#L163-L164](https://github.com/obsproject/obs-studio/blob/717cfa8b7e633853cd21ad2cc656185b3b4255bb/libobs/audio-monitoring/osx/coreaudio-output.c#L163-L164)) ยังไม่รวม buffering ของ libobs เอง [โค้ด] เสียงบน TV จึงมีแนวโน้มช้ากว่าภาพ [อนุมาน, ต้องวัด]
- **หน้าต่าง:**
  - Fullscreen Projector เรียก `setGeometry` ตามจอที่เลือกแล้ว `showFullScreen()` ([OBSProjector.cpp#L125-L130](https://github.com/obsproject/obs-studio/blob/717cfa8b7e633853cd21ad2cc656185b3b4255bb/frontend/widgets/OBSProjector.cpp#L125-L130)) [โค้ด] เอกสาร Qt บอกว่าบน macOS คำสั่งนี้พาแอปเข้า full screen และให้ desktop แยก ([QWidget::showFullScreen](https://doc.qt.io/qt-6/qwidget.html#showFullScreen)) [เอกสาร]
  - มีผู้ใช้ในฟอรัมรายงานว่า projector ยังเห็น menu bar ([OBS forum](https://obsproject.com/forum/threads/moving-fullscreen-projector-output.96870/)) [ยังไม่ได้ตรวจ]
  - ชื่อหน้าต่างเป็น "Projector - Source: %1", "Projector - Scene: %1" หรือ "Projector - Preview" ([OBSProjector.cpp#L338-L363](https://github.com/obsproject/obs-studio/blob/717cfa8b7e633853cd21ad2cc656185b3b4255bb/frontend/widgets/OBSProjector.cpp#L338-L363)) [โค้ด]
  - projector มีตัวเลือก always on top ผ่าน `Qt::WindowStaysOnTopHint` ([#L30-L34](https://github.com/obsproject/obs-studio/blob/717cfa8b7e633853cd21ad2cc656185b3b4255bb/frontend/widgets/OBSProjector.cpp#L30-L34)) [โค้ด]
- **ข้อเสีย:**
  - OBS ทำงานเป็น compositor เต็มตัว render canvas, preview และ projector ตลอดเวลา จึงน่าจะกิน RAM, GPU และความร้อนมากกว่า viewer ล้วนบนเครื่องไม่มีพัดลม 8 GB [อนุมาน, ต้องวัด]
  - ต้องตั้งค่าหลายขั้น: scene, source, monitoring และ canvas 1920×1080@60

### QuickTime Player (New Movie Recording)

- เวอร์ชันบนเครื่องคือ 10.5 และ bundle ID `com.apple.QuickTimePlayerX` [เครื่อง]
- ตัวเลือกใน Options มีแค่ Camera, Microphone และ Quality ซึ่ง High คือ H.264 + AAC ส่วน Maximum คือ ProRes 422 + Linear PCM ของไฟล์ที่อัด ส่วนเสียง Apple เขียนไว้ว่า "Adjust the volume control to hear the audio you're recording" ([Record a movie](https://support.apple.com/guide/quicktime-player/record-a-movie-qtp356b55534/mac)) [เอกสาร]
- **ไม่มีที่เลือก resolution หรือ fps ของ device** การ์ดที่ประกาศโหมด 2K30 อาจถูกเลือกแทน 1080p60 [ต้องวัด]
- full screen ใช้ View > Enter Full Screen ส่วน Float on Top มีเอกสารเฉพาะตอนเล่นไฟล์ ([Choose a screen size](https://support.apple.com/guide/quicktime-player/choose-a-screen-size-qtp31dfa136d/mac)) [เอกสาร] หน้าต่าง Movie Recording ใช้ Float on Top ได้หรือไม่ยังไม่ได้ตรวจ
- ชื่อหน้าต่างมาจาก string "Movie Recording" ซึ่งแปลไทยเป็น "การบันทึกภาพยนตร์" (`Localizable.loctable` ใน bundle) ชื่อจึงเปลี่ยนตามภาษาของระบบ ห้ามใช้จับคู่โปรไฟล์ [เครื่อง]
- HUD ควบคุม (ปุ่มอัด และ volume) อยู่ตรงกลางล่างของภาพ ซึ่งเป็นตำแหน่ง subtitle ยังไม่รู้ว่า HUD เป็นส่วนของหน้าต่างที่ ScreenCaptureKit จับหรือไม่ [ต้องวัด]
- เสียง monitor ออกจาก process ไหน และหน่วงเท่าไร ยังไม่รู้ [ต้องวัด] รายงานจากผู้ใช้บน Reddit ขัดกันเองว่าหน่วงพอ ๆ กับ OBS หรือมากกว่า จึงไม่นับเป็นหลักฐาน

### VLC 3.0

- module `avcapture` ไม่มี option เลือก resolution หรือ fps และขอภาพเป็น `kCVPixelFormatType_32BGRA` ([avcapture.m#L467](https://github.com/videolan/vlc/blob/fb916cd7cd4381bfa1cb5917d1b7a33a688209d2/modules/access/avcapture.m#L467)) [โค้ด]
- ความหน่วงอ่านจาก `live-caching` ([avcapture.m#L336-L338](https://github.com/videolan/vlc/blob/fb916cd7cd4381bfa1cb5917d1b7a33a688209d2/modules/access/avcapture.m#L336-L338)) ซึ่งค่าเริ่มต้นคือ `DEFAULT_PTS_DELAY` = 300 ms ([libvlc-module.c#L1869](https://github.com/videolan/vlc/blob/fb916cd7cd4381bfa1cb5917d1b7a33a688209d2/src/libvlc-module.c#L1869), [vlc_config.h#L68](https://github.com/videolan/vlc/blob/fb916cd7cd4381bfa1cb5917d1b7a33a688209d2/include/vlc_config.h#L68)) เกินเป้า NFR-19 ตั้งแต่ต้น ลดค่าได้แต่ต้องจูนเอง [โค้ด]
- เสียงใช้ `input-slave=avaudiocapture://<UID>` ([VLCOpenWindowController.m#L505](https://github.com/videolan/vlc/blob/fb916cd7cd4381bfa1cb5917d1b7a33a688209d2/modules/gui/macosx/VLCOpenWindowController.m#L505)) [โค้ด]
- full screen เริ่มต้นเป็นแบบเก่าที่ไม่ใช่ native ([macosx.m#L158](https://github.com/videolan/vlc/blob/fb916cd7cd4381bfa1cb5917d1b7a33a688209d2/modules/gui/macosx/macosx.m#L158)) bundle ID `org.videolan.vlc` ([Info.plist.in](https://github.com/videolan/vlc/blob/fb916cd7cd4381bfa1cb5917d1b7a33a688209d2/share/Info.plist.in)) [โค้ด]

### IINA

- ไม่รองรับการเปิด capture device คำขอ feature เปิดค้างตั้งแต่ 2020 ([iina#3265](https://github.com/iina/iina/issues/3265)) [เอกสาร]
- mpv ข้างใต้เปิด `av://avfoundation:...` ได้ และมี profile `low-latency` ([mpv.rst#L1417-L1432](https://github.com/mpv-player/mpv/blob/f92afa75cdb06e077a9ecd9a9ec2eb36b16327c2/DOCS/man/mpv.rst#L1417-L1432), [builtin.conf#L65-L75](https://github.com/mpv-player/mpv/blob/f92afa75cdb06e077a9ecd9a9ec2eb36b16327c2/etc/builtin.conf#L65-L75)) แต่เป็น CLI ไม่มี GUI ให้เลือก device [โค้ด]

### แอปของ UGREEN

ไม่พบแอปของ UGREEN สำหรับ macOS หน้าสินค้ารุ่นใกล้เคียงบอกแค่ว่า plug and play และใช้กับ OBS Studio, XSplit และอื่น ๆ ได้ ([UGREEN 25173](https://uk.ugreen.com/products/25173)) [เอกสาร] ต้องดูคู่มือของรุ่นจริงอีกครั้งใน [#2](https://github.com/benzstuuuuu/game-live-translate/issues/2)

### ตัวอื่นที่เจอ

- **`ffplay` (FFmpeg 9.0.2 มีบนเครื่อง [เครื่อง]):**
  - กำหนด `-video_size`, `-framerate` และ `-pixel_format` ของ avfoundation ได้ ([ffmpeg-devices: avfoundation](https://ffmpeg.org/ffmpeg-devices.html#avfoundation)) และตั้ง `-fs` กับ `-window_title` ได้ ([ffplay](https://ffmpeg.org/ffplay.html)) [เอกสาร]
  - เป็น CLI ที่ไม่มี bundle ID จึงจับคู่โปรไฟล์ไม่ได้ [อนุมาน]
  - ใช้เป็นตัววัด "พื้น" ความหน่วงของการ์ดใน [#14](https://github.com/benzstuuuuu/game-live-translate/issues/14) ได้
- **Quick Camera** (`com.simonguest.QCamera`): README พูดถึงการแสดงภาพจากกล้องอย่างเดียว ไม่มีเสียง จึงไม่ผ่านเกณฑ์ ([quick-camera](https://github.com/simonguest/quick-camera)) [เอกสาร]
- **Console Link** (`com.chunqian.ConsoleLink`): ราคา $7.99 ปิดซอร์ส และคำอธิบายบน App Store เน้น iPad ([App Store](https://apps.apple.com/us/app/console-link/id6448843682), [iTunes lookup](https://itunes.apple.com/lookup?id=6448843682)) [เอกสาร] ยังไม่ได้ยืนยันว่ามีรุ่น Mac แบบ native

## ผลต่อ ticket อื่น

- **FR-GAME-06 (การระบุเกม):**
  - ทุก viewer ให้ bundle ID เดียวกับทุกเกมจาก PS5 และชื่อหน้าต่างคงที่ คือ "Movie Recording", "Projector - Source: …" หรือ "Consolation"
  - bundle ID + ชื่อหน้าต่างบอกได้แค่ว่า "นี่คือภาพจาก PS5 ผ่าน viewer X" **ต้องให้ผู้ใช้เลือกโปรไฟล์เกมเอง** ในโหมด C1
  - ชื่อหน้าต่างของ QuickTime เปลี่ยนตามภาษาระบบ จึงห้ามใช้เป็น key ส่วน OBS เอาชื่อ scene ใส่ในชื่อ projector ได้ ใช้เป็นทางเลือกเสริมได้แต่ไม่ควรออกแบบโดยพึ่งมัน
  - ข้อนี้ปิดข้อ "การระบุเกม" ใน "Not yet specified" ของ map ได้ในทิศทาง "เลือกโปรไฟล์เอง"
- **[#6](https://github.com/benzstuuuuu/game-live-translate/issues/6) และ [#12](https://github.com/benzstuuuuu/game-live-translate/issues/12) (overlay ทับ viewer เต็มจอ):**
  - ตัวเลือกอันดับ 1, 2 และ QuickTime เต็มจอแบบ native Space ทั้งหมด (OBS อ้างจากเอกสาร Qt ยังต้องยืนยัน) overlay จึงต้องใช้ `fullScreenAuxiliary` + `canJoinAllSpaces` และ level สูงกว่า `.floating` เผื่อผู้ใช้เปิด always-on-top ของ viewer
  - ถ้า native Space ใช้ไม่ได้กับ Tauri ทางถอยคือ viewer ที่ขยายเต็มจอโดยไม่สร้าง Space ใหม่ เช่น VLC โหมดเริ่มต้น หรือ fork Consolation ให้ใช้ borderless window
- **[#11](https://github.com/benzstuuuuu/game-live-translate/issues/11) (จับกรอบ subtitle):**
  - ใช้ single window filter แล้ว crop กรอบ subtitle ภาพที่ได้จะไม่มี overlay ของ Yokoten
  - ต้องกันไม่ให้ UI ในหน้าต่าง viewer (HUD หรือแถบควบคุม) โผล่ทับกรอบ subtitle เช่น อย่าให้เมาส์อยู่บน TV
- **[#13](https://github.com/benzstuuuuu/game-live-translate/issues/13) (จับเสียง viewer):**
  - ScreenCaptureKit จับเสียงทั้งแอป สำหรับ OBS คือเสียงทุก source ที่ monitor
  - ทางเลือกที่ไม่ต้องพึ่ง viewer เลยคืออ่าน input เสียงของการ์ด (UAC) ตรง ๆ ซึ่งคาบเกี่ยวกับ C2 ต้องตรวจว่าเปิดพร้อม viewer ได้ [ต้องวัด]
- **[#14](https://github.com/benzstuuuuu/game-live-translate/issues/14) (ความหน่วงและ RAM):** ใช้รายการวัดข้างล่าง

## สิ่งที่ยังยืนยันไม่ได้และต้องวัดบนเครื่อง

ทุกข้อวัดบน MacBook Air M2 8 GB, macOS 27, การ์ดจริง, PS5 และ TV:

1. **โหมดของการ์ด:**
   - ใช้ `ffmpeg -f avfoundation -list_devices true -i ""` เพื่อดูว่าการ์ดชื่ออะไร มี device เสียงแยกไหม
   - ลอง `ffplay -f avfoundation -video_size 1920x1080 -framerate 60 -i "<การ์ด>:<เสียง>"` ถ้าโหมดไม่ตรง FFmpeg จะแสดงรายการโหมดที่รองรับ
   - ดูเมนู resolution ของ Consolation และดูว่า QuickTime เลือกโหมดไหนเองด้วย stats หรือ Window > Show Movie Inspector ตอนอัดไฟล์ทดสอบ
2. **ความหน่วงของภาพ (NFR-19):**
   - ถ่าย slow-motion 240 fps ตอนกดปุ่มบนจอยแล้วดูว่าภาพบน TV เปลี่ยนเมื่อไร เทียบระหว่าง PS5 → TV ตรงกับ PS5 → การ์ด → Mac → TV
   - วัดทีละ viewer ได้แก่ ffplay (พื้น), Consolation, OBS และ QuickTime
3. **A/V sync และความหน่วงของเสียงบน TV:** วัดต่อ viewer และลอง buffer length ของ Consolation ที่ 2, 4 และ 8
4. **RAM และ CPU (NFR-18):**
   - ใช้ `footprint <pid>` หรือ Activity Monitor (Memory, Memory Pressure) และ `top -pid` ระหว่างเล่น 30 นาทีพร้อม Yokoten
   - ดูอุณหภูมิและการ throttle ด้วยว่า fps ตกไหม
5. **ScreenCaptureKit จับหน้าต่าง viewer ได้ไหม:**
   - จับทั้งตอน windowed และ native full screen บน TV
   - ดูว่า HUD ของ QuickTime และแถบควบคุมของ Consolation ติดในภาพที่จับหรือไม่
   - ดูว่า `SCWindow.title` ของ Consolation เป็นอะไร
6. **เสียงของ viewer:**
   - process tap และ ScreenCaptureKit จับเสียงของ Consolation, OBS และ QuickTime ได้ไหม
   - เสียง monitor ของ QuickTime ออกจาก process ไหน
7. **OBS projector บน macOS 27:** เป็น native Space จริง หรือยังเห็น menu bar
8. **Overlay:** NSPanel ของ Tauri อยู่เหนือ viewer ที่เต็มจอบน TV ได้ไหม (ส่งต่อให้ #6 และ #12)

## อันดับและเหตุผล

1. **Consolation:** เป็นตัวเดียวที่รวมครบในแอปเบา ๆ ทั้งการเลือก resolution / fps ของ device, preview ผ่าน layer ของระบบ, เสียงจากการ์ดที่เล่นใน process ของตัวเองพร้อม buffer ที่ปรับได้ และ native full screen ฟรีและ open source จึง fork ได้ถ้าโปรเจกต์หยุด และใช้เป็นแบบของ C2 (FR-CAP-10) ได้ ความเสี่ยงคือโปรเจกต์เล็ก เสียงออกได้แค่ default output และยังไม่มีตัวเลข latency
2. **OBS Studio:** ทำได้ครบทุกข้อและตั้งค่าได้ละเอียดที่สุด ทั้ง Capture Card Device fast path, การเลือกโหมด และ Monitoring Device ที่ชี้ไป TV ได้โดยไม่แตะ default output ชื่อ projector คงที่ และเป็นแอปที่ UGREEN อ้างถึง แต่หนักที่สุดสำหรับเครื่อง 8 GB ที่ไม่มีพัดลม และ audio monitoring เพิ่ม buffer จึงเป็นตัวสำรองเมื่อ Consolation ไม่ผ่าน

QuickTime Player ใช้เป็น baseline เพราะติดตั้งมากับเครื่อง ถ้าผลวัดข้อ 1 และข้อ 5 ออกมาดี (ได้ 1080p60 และ HUD ไม่ติดในภาพ) ให้ขยับขึ้นแทน OBS เป็นอันดับ 2
