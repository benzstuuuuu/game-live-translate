# Discord Go Live บน macOS: แชร์หน้าต่าง OBS พร้อมเสียง

- Ticket: [#24 Discord Go Live บน macOS: แชร์หน้าต่าง OBS พร้อมเสียง](https://github.com/benzstuuuuu/game-live-translate/issues/24) (map [#1](https://github.com/benzstuuuuu/game-live-translate/issues/1))
- เกี่ยวข้อง: [#23 Spike: หน่วงภาพและเสียงฝั่งสตรีม 2 วินาที](https://github.com/benzstuuuuu/game-live-translate/issues/23), [#15 Topology ของ MVP](https://github.com/benzstuuuuu/game-live-translate/issues/15)
- วันที่: 2026-11-18
- เครื่องเป้าหมาย: MacBook Air M2 8 GB, macOS 26.5.1, OBS 33.0.1, Discord desktop, account ฟรี (ไม่มี Nitro)
- สถานะ: research จากเอกสาร ซอร์สโค้ด OBS และ forum **ยังไม่ได้ทดลองบนเครื่องจริง** ข้อที่ติด [ต้องวัด] ควรยืนยันด้วย computer use ก่อนปิด gate

ป้ายกำกับ: **[doc]** เอกสารทางการ (Discord support, OBS KB, Apple, Qt) · **[code]** อ่านจากซอร์สโค้ด OBS tag `33.0.1` · **[forum]** forum / บทความบุคคลที่สาม · **[inferred]** อนุมาน ยังไม่ยืนยัน · **[ต้องวัด]** ต้องทดลองกับของจริง

## สรุป

1. **แชร์หน้าต่างได้** Go Live แชร์ "any application window, or your entire screen" ได้ [doc] หน้าต่างที่ควรแชร์คือ **Windowed Projector (Program)** ของ OBS ต้องให้ Discord มีสิทธิ์ **Screen & System Audio Recording** ใน System Settings › Privacy & Security [doc Apple/forum] และ**ต้องปิด "Always on top" ของ projector** เพราะ OBS ยกหน้าต่างไประดับ `NSScreenSaverWindowLevel` ซึ่งตัวเลือกหน้าต่างของแอปแชร์จอมักกรองทิ้ง [code + forum, ต้องวัด]
2. **เสียงไปด้วยเมื่อแชร์หน้าต่างแอป** Discord แชร์เสียงของแอปบน macOS 13 ขึ้นไป [doc] เสียงที่ได้คือ**เสียงที่ process OBS เล่นออกมา** ก็คือเสียง **audio monitoring** ไม่ใช่ mix ของ program output แชร์ทั้งจอแล้วเสียงอาจไม่มา [forum]
3. **ความเสี่ยงใหญ่ที่สุด: monitoring บน macOS ไม่สน Sync Offset** ใน OBS 33.0.1 ตัว monitor ของ Windows หน่วงเสียงตาม `sync_offset` แต่ตัว CoreAudio ของ macOS ไม่หน่วง [code] ถ้า [#23](https://github.com/benzstuuuuu/game-live-translate/issues/23) หน่วงเสียงด้วย Sync Offset เสียงใน Discord จะมาก่อนภาพราว 2 วินาที ต้องหน่วงเสียงใน filter chain ก่อนถึง monitoring แทน
4. **account ฟรี:** สตรีมได้สูงสุด 720p/30fps, ดูพร้อมกันได้ 50 คน (ไม่นับคนสตรีม), เปิดได้ทั้งใน voice channel ของ server และ DM [doc] ไม่พบการจำกัดเวลาในเอกสาร ส่วน bitrate Discord ไม่เปิดเผย
5. **ขนาดตัวอักษรไทย (อนุมาน):** บน canvas 1080p ใช้ **56–64 px ตัวหนา บนแถบพื้นทึบ** ต่ำสุด 48 px ไม่เกิน 2 บรรทัด ราว 35 ตัวต่อบรรทัด (เกณฑ์ Netflix ภาษาไทย [doc]) ตัวเลขนี้ต้องวัดจากฝั่งคนดู
6. **OBS Virtual Camera เป็นทางสำรองด้านภาพเท่านั้น** OBS 30 ขึ้นไปบน macOS 13 ขึ้นไปใช้ camera extension ที่ "compatible with all Mac applications" [doc] ประกาศ format เดียวคือ 1920×1080@60 [code] กล้องไม่มีเสียง เสียงต้องส่งทางไมค์ (เช่น BlackHole) แทน
7. **คำแนะนำ:** ใช้ Go Live แชร์ Windowed Projector (Program) ส่วนเสียงให้มาจาก monitoring ของ OBS โดยหน่วงเสียงใน filter chain ไม่แชร์ทั้งจอ ใช้ Virtual Camera + BlackHole เป็นทางสำรองเมื่อแชร์หน้าต่างไม่ได้

## 1. แชร์หน้าต่าง OBS projector ได้ไหม และต้องให้สิทธิ์อะไร

### สิ่งที่เอกสารยืนยัน

- "Go Live and Screen Share allows you to screen share any application window, or your entire screen in a Discord server or voice call." และขั้นตอนบอกให้ "Select an individual application window to share, or select an entire screen to share." ([Discord: Go Live and Screen Share](https://support.discord.com/hc/en-us/articles/360040816151-Go-Live-and-Screen-Share), แก้ไขล่าสุด 2026-06-19) [doc]
- ใน server ต้องมีสิทธิ์ **Video** ของ voice channel ("The ability to Screen Share on a channel is locked behind the Video permission") ค่าเริ่มต้นเปิดไว้ [doc, บทความเดียวกัน]
- สิทธิ์ macOS: Discord ต้องเปิดอยู่ใน **System Settings › Privacy & Security › Screen & System Audio Recording** (macOS 13 เรียกว่า Screen Recording) ถ้าไม่เปิด จะแชร์ได้แค่ภาพดำหรือ wallpaper และตัวเลือกหน้าต่างไม่มีภาพตัวอย่าง ใช้แบบ "audio only" ไม่พอ ต้องเป็นสิทธิ์จอ ([Apple: Control access to screen and system audio recording](https://support.apple.com/guide/mac-help/control-access-screen-system-audio-recording-mchld6aa7d23/mac) อ้างใน [kagemusha](https://kagemusha.app/learn/discord-mac-not-working-share), [tonal.fm](https://www.tonal.fm/patch/discord-screen-share-no-audio-mac)) [doc + forum]
- ตั้งแต่ macOS 15 แอปที่ใช้ตัวเลือกหน้าต่างของตัวเองแทนตัวเลือกของระบบจะเจอ prompt "…requesting to bypass the system private window picker…" ให้กดอนุญาตซ้ำเดือนละครั้ง ([MacRumors](https://www.macrumors.com/2024/08/15/macos-sequoia-screen-recording-app-permissions/), [TidBITS](https://tidbits.com/2024/08/19/apple-reduces-excessive-sequoia-permission-requests-shifts-to-monthly)) [forum] Discord ยังใช้ตัวเลือกของตัวเอง จึงน่าจะเจอ prompt นี้บน macOS 26 ด้วย [inferred, ต้องวัด]

### Windowed projector ของ OBS

- Projector เป็น `Qt::Window` ธรรมดา ([`OBSProjector.cpp`](https://github.com/obsproject/obs-studio/blob/33.0.1/frontend/widgets/OBSProjector.cpp)) [code] จึงควรขึ้นในรายการหน้าต่างของ Discord ชื่อประมาณ "Windowed Projector (Program)" [inferred]
- **กับดัก:** ถ้าเปิด "Always on top" (ค่า `ProjectorAlwaysOnTop` หรือคลิกขวาที่ projector) OBS จะตั้ง `[[view window] setLevel:NSScreenSaverWindowLevel]` ([`platform-osx.mm`](https://github.com/obsproject/obs-studio/blob/33.0.1/frontend/utility/platform-osx.mm) `SetAlwaysOnTop`) [code] ตัวเลือกหน้าต่างของแอปแชร์จอส่วนมากแสดงเฉพาะหน้าต่าง layer 0 จึงอาจไม่เห็น projector [inferred] ตรงกับรายงานใน forum ว่า windowed projector บน Mac "isn't a real window" ไม่ขึ้นเป็นเป้าแชร์ใน Slack / Google Meet ([OBS forum 175221](https://obsproject.com/forum/threads/windowed-projector-preview-isnt-a-real-window.175221/)) [forum] **ให้ปิด Always on top ก่อนแชร์** [ต้องวัด]
- OBS มีตัวเลือกซ่อนหน้าต่าง OBS จากการจับภาพ แต่บน macOS ยังไม่ทำ (`SetDisplayAffinitySupported()` คืน `false`) และ projector ถูกยกเว้นไว้อยู่แล้ว [code] เรื่องนี้จึงไม่ทำให้ projector หายจากการแชร์
- ขนาดหน้าต่าง: ตั้ง projector ให้มีพื้นที่ภาพ ≥ 1280×720 Discord ย่อลงเป็น 720p เอง ถ้าเล็กกว่านี้ Discord จะส่งภาพที่ละเอียดน้อยลงไปอีก [inferred]
- หน้าต่างที่ถูก minimize จับภาพไม่ได้ (ScreenCaptureKit หยุดส่งเฟรม ดู [#3](https://github.com/benzstuuuuu/game-live-translate/issues/3)) [doc Apple] ส่วนหน้าต่างที่ถูกบังน่าจะยังจับได้ [inferred, ต้องวัด] แต่ไม่รู้ว่า Discord ใช้ API ตัวไหนจับภาพ ([คำขอให้ Discord ใช้ ScreenCaptureKit](https://support.discord.com/hc/en-us/community/posts/10138448234135-Use-ScreenCaptureKit-on-macOS-Discord-for-Screen-Sharing) [forum]; [Spacebar](https://www.spacebar.news/how-to-fix-discord-audio-capture-mac/) บอกว่าใช้ API ของ macOS แล้วตั้งแต่ 13 [forum])

### Fullscreen projector

- OBS เรียก `showFullScreen()` [code] ซึ่งบน macOS Qt เปิดเป็น native full screen ใน Space แยก [doc: [Qt `QWidget::showFullScreen`](https://doc.qt.io/qt-6/qwidget.html#showFullScreen)]
- MacBook เครื่องนี้มีจอเดียว (TV รับภาพจาก loop out ของการ์ด ไม่ได้ต่อกับ Mac ตาม [#15](https://github.com/benzstuuuuu/game-live-translate/issues/15)) fullscreen projector จะบังจอทั้งหมดจนใช้ OBS / Terminal / หน้า `/setup` ไม่ได้ และหน้าต่างที่อยู่ใน Space อื่นอาจไม่ขึ้นในรายการของ Discord [inferred] บทความบุคคลที่สามหลายแห่งบอกว่าแชร์แอปที่เต็มจอแล้วได้ภาพดำ ([pcrisk](https://www.pcrisk.com/blog/mac/13494-how-to-fix-discord-black-screen-when-sharing-your-screen-on-mac)) [forum] **ไม่แนะนำ**

## 2. เสียงเกมไปกับ Go Live ไหม

### สิ่งที่เอกสารยืนยัน

- "Sharing audio from applications and games is supported on macOS 13 and above." ([Discord: macOS Screen Share with Audio](https://support.discord.com/hc/en-us/articles/1500006741102-macOS-Screen-Share-with-Audio), แก้ไข 2024-02-23) [doc]
- "Currently audio can only be captured by the Windows desktop, MacOS desktop, Chrome browser, and mobile clients." [doc, Go Live article] ต้องใช้แอป desktop ไม่ใช่ Discord บน Safari
- macOS 26.5.1 ผ่านเงื่อนไข macOS 13+

### แชร์หน้าต่าง กับ แชร์ทั้งจอ

- บทความบุคคลที่สามตรงกันว่าบน Mac Discord ส่งเสียง**ของแอปที่แชร์** และแนะนำให้แชร์หน้าต่างแอปแทนการแชร์จอ: "capturing audio from your whole Mac doesn't work in Discord … It can only capture audio when you select a specific application or window." ([Spacebar](https://www.spacebar.news/how-to-fix-discord-audio-capture-mac/), [tonal.fm](https://www.tonal.fm/patch/discord-screen-share-no-audio-mac)) [forum] ข้อความของ Spacebar เขียนสมัยยังใช้ Audio Capture Engine ของ Rogue Amoeba จึงอาจล้าสมัย แต่ก็ไม่พบเอกสารทางการว่าแชร์ทั้งจอแล้วได้เสียง [ต้องวัด]
- ผลที่ตามมา: เมื่อแชร์ projector Discord จะได้เสียงทั้งหมดที่ **process OBS** เล่นออก ซึ่งก็คือเสียงจาก **audio monitoring** เพราะ program mix ของ OBS ไม่ได้เล่นออกลำโพง [inferred จาก [code](https://github.com/obsproject/obs-studio/blob/33.0.1/libobs/audio-monitoring/osx/coreaudio-output.c)] forum ก็บอกตรงกันว่า "projectors don't have their own audio" ([OBS forum 162053](https://obsproject.com/forum/threads/obs-audio-minus-host-mic-audio-for-projector-mode.162053/)) [forum]

### กับดัก Monitor Only / Monitor Off

| ตั้งค่า Audio Monitoring ของ source | อยู่ใน program mix (สตรีม/อัด) | OBS เล่นออก (Discord ได้ยิน) |
| --- | --- | --- |
| Monitor Off | ใช่ | **ไม่** |
| Monitor Only (mute output) | ไม่ | **ใช่** |
| Monitor and Output | ใช่ | **ใช่** |

[inferred จากพฤติกรรม monitoring ของ OBS]

- เสียงเกมจากการ์ดต้องตั้งเป็น **Monitor and Output** (หรือ Monitor Only ถ้าไม่ต้องใช้ program mix) แล้วตั้ง Settings › Audio › Monitoring Device เป็น **Default** [inferred]
- ไมค์ของผู้ใช้ (ถ้ามีใน OBS) ต้องเป็น **Monitor Off** ไม่อย่างนั้นเพื่อนจะได้ยินเสียงซ้ำสองทาง ทั้งจากเสียงพูดใน Discord และจากสตรีม ([OBS forum 187670](https://obsproject.com/forum/threads/my-mic-can-be-heard-twice-when-i-stream-obs-over-discord.187670)) [forum]
- เสียง monitoring ออกลำโพง Mac ด้วย ผู้ใช้ฟังเสียงจาก TV อยู่แล้ว จะได้ยินเสียงซ้อน ทางแก้คือหรี่หรือปิดเสียง Mac แต่ไม่รู้ว่าระบบจับเสียงแอปก่อนหรือหลังขั้น volume [ต้องวัด] อีกทางคือตั้ง Monitoring Device เป็น BlackHole แต่ต้องวัดว่า Discord ยังจับเสียง OBS ได้ไหมเมื่อ OBS ส่งเสียงออกอุปกรณ์ที่ไม่ใช่ค่าเริ่มต้น [ต้องวัด]

### ความเสี่ยงใหญ่: monitoring บน macOS ไม่หน่วงตาม Sync Offset

- ฝั่ง Windows ตัว monitor มี `process_audio_delay()` ที่บวก `monitor->source->sync_offset` ([`wasapi-output.c` บรรทัด 52–68](https://github.com/obsproject/obs-studio/blob/33.0.1/libobs/audio-monitoring/win32/wasapi-output.c)) [code]
- ฝั่ง macOS `on_audio_playback()` resample แล้วส่งเข้า AudioQueue ทันที ไม่มีการอ่าน `sync_offset` ทั้งไฟล์ ([`coreaudio-output.c`](https://github.com/obsproject/obs-studio/blob/33.0.1/libobs/audio-monitoring/osx/coreaudio-output.c)) [code] forum ก็พูดตรงกันว่า Sync Offset "only affects output (recorded file, live stream to RTMP server). It doesn't affect audio monitor output" ([OBS forum 84121](https://obsproject.com/forum/threads/use-audio-sync-offset-delay-in-monitoring.84121)) [forum]
- filter Video Delay (Async) ไม่หน่วงเสียง (`DELAY_AUDIO` ถูกปิดไว้ใน [`async-delay-filter.c`](https://github.com/obsproject/obs-studio/blob/33.0.1/plugins/obs-filters/async-delay-filter.c)) [code]
- **ผล:** ถ้า [#23](https://github.com/benzstuuuuu/game-live-translate/issues/23) หน่วงภาพ 2 วินาทีแต่หน่วงเสียงด้วย Sync Offset เพื่อนบน Discord จะได้ยินเสียงก่อนเห็นภาพราว 2 วินาที [inferred จาก code, ต้องวัด]
- ทางที่น่าจะใช้ได้: หน่วงเสียงด้วย **filter เสียงใน chain ของ source** เช่น VST delay ที่ตั้ง dry 0% / wet 100% / 2000 ms หรือ plugin หน่วงเสียงของบุคคลที่สาม เพราะ filter เสียงทำงานใน `filter_async_audio()` ก่อน callback ของ monitoring ([`obs-source.c`](https://github.com/obsproject/obs-studio/blob/33.0.1/libobs/obs-source.c) `obs_source_output_audio`) [code] ใช้แทน Sync Offset เพื่อให้ทั้ง program และ monitoring หน่วงเท่ากัน [inferred, ต้องวัดใน #23]

### ทางสำรองด้านเสียง

- **BlackHole เป็นไมค์ของ Discord:** ตั้ง OBS Monitoring Device = BlackHole 2ch แล้วตั้ง Discord Input Device = BlackHole ทั้ง voice channel จะได้ยินเสียงเกมผ่านช่องเสียงพูด ไม่ใช่เฉพาะคนดูสตรีม ข้อเสีย: ต้องปิด noise suppression / echo cancellation / AGC ของ Discord ไม่งั้นเสียงเกมเพี้ยน เสียงไมค์ของผู้ใช้จะหาย ต้องเอาไมค์เข้า OBS แล้ว monitor ด้วย (หรือใช้ aggregate device) และคุณภาพเสียงขึ้นกับ bitrate ของ voice channel [inferred + [tonal.fm](https://www.tonal.fm/patch/discord-screen-share-no-audio-mac) forum] ข้อดีคือหน่วงเสียงได้เหมือนทางหลัก (ปัญหา Sync Offset ยังอยู่)

## 3. ข้อจำกัดของ account ฟรี

| เรื่อง | ค่า | แหล่ง |
| --- | --- | --- |
| ความละเอียด / fps | "All users can stream up to 720p/30fps. Nitro and Nitro Classic subscribers can stream up to 4k/60fps." | [doc](https://support.discord.com/hc/en-us/articles/360040816151-Go-Live-and-Screen-Share) |
| จำนวนคนดู | "Go Live streams have a maximum of 50 concurrent viewers (50 viewers, plus the broadcaster)" | [doc] |
| คนดูหนึ่งคน | ดูได้ทีละสตรีม ใน voice channel เดียว | [doc] |
| สิทธิ์ใน server | ต้องมีสิทธิ์ Video ใน voice channel (เปิดไว้เป็นค่าเริ่มต้น) | [doc] |
| เวลา | ไม่มีข้อจำกัดเวลาในเอกสาร | [doc, ไม่พบ] |
| Bitrate | Discord ไม่เปิดเผย บทความบุคคลที่สามบอก ~1.5 Mbps สำหรับ account ฟรี ยืนยันไม่ได้ | [forum, ไม่น่าเชื่อถือ] |

### ตัวอักษรไทยต้องใหญ่แค่ไหน

ไม่มีเอกสารของ Discord เรื่องนี้ ตัวเลขด้านล่างอนุมานทั้งหมด [inferred, ต้องวัด]

- canvas 1920×1080 ถูกย่อเหลือ 1280×720 (×0.667) แล้ว encode ที่ bitrate ต่ำ สระบน สระล่าง และวรรณยุกต์ที่ซ้อนกัน (เช่น "ปั้น") จะเบลอก่อนตัวพยัญชนะ
- Netflix (ภาษาไทย): ไม่เกิน 2 บรรทัด, 35 ตัวต่อบรรทัด (ไม่นับสระบน-ล่างและวรรณยุกต์), ขนาดตัว "relative to video resolution and ability to fit 35 characters across the screen", 17 ตัว/วินาที ([Netflix Thai Timed Text Style Guide](https://backlothelp.netflix.com/hc/en-us/articles/220448308)) [doc]
- **ค่าเสนอ:** บน canvas 1080p ใช้ 56–64 px (ประมาณ 37–43 px ที่ 720p) น้ำหนัก SemiBold/Bold พื้นแถบดำทึบ ≥ 80% แทนการใช้แค่เส้นขอบ (เส้นขอบบางจะเละเมื่อบีบอัด) ต่ำสุด 48 px (32 px ที่ 720p) ไม่เกิน 2 บรรทัด ~35 ตัวต่อบรรทัด [inferred]
- ถ้าคนดูเปิดสตรีมเป็น tile เล็กใน Discord ตัวหนังสือจะเล็กลงไปอีก ให้แนะนำเพื่อนกด Pop Out หรือเต็มจอ ([doc: Go Live: Pop Out](https://support.discord.com/hc/en-us/articles/360040816151-Go-Live-and-Screen-Share))
- วิธีวัด: ให้ account ที่สองดูสตรีม แล้วใช้ computer use จับภาพทั้งแบบเต็มจอและแบบ tile ลองขนาด 40 / 48 / 56 / 64 px ด้วยบทพูดที่มีสระซ้อน

## 4. OBS Virtual Camera เป็นทางสำรอง

- "OBS Studio 30.0 and higher running on macOS 13 ("Ventura") and higher include a new virtual camera component that is compatible with all Mac applications." ต้องอนุญาต system extension ครั้งแรกที่ System Settings › General › Login Items & Extensions › Camera Extensions (macOS 15 ขึ้นไป) แล้วเปิด OBS ใหม่ ([OBS KB: Virtual Camera Troubleshooting](https://obsproject.com/kb/virtual-camera-troubleshooting)) [doc]
- ปัญหาเก่าที่ Discord ไม่เห็นกล้องเสมือน (ต้อง re-sign Discord) เป็นของ DAL plugin รุ่นเก่า ([VDO.Ninja](https://docs.vdo.ninja/common-errors-and-known-issues/obs-virtual-camera-not-working), [OBS forum](https://obsproject.com/forum/threads/obs-virtual-cam-to-discord.133437/)) [forum] camera extension ทำงานแยก process จึงไม่น่าติดปัญหานี้แล้ว [inferred, ต้องวัด]
- Format: camera extension ประกาศ format เดียว **1920×1080 @ 60 fps** (`CMVideoDimensions(width: 1920, height: 1080)`, `OBSCameraFrameRate = 60` ใน [`OBSCameraDeviceSource.swift`](https://github.com/obsproject/obs-studio/blob/33.0.1/plugins/mac-virtualcam/src/camera-extension/OBSCameraDeviceSource.swift)) [code] ภาพเป็น output ของ OBS (หน่วงตาม scene ได้)
- Discord ไม่ได้บอกความละเอียดกล้องในเอกสาร บอกแค่ว่าวิดีโอใน server รับได้ 25 คน ([Discord: Video Calls](https://support.discord.com/hc/en-us/articles/360041721052)) [doc] คาดว่าส่งไม่เกิน 720p และอาจถูก crop ให้พอดี tile [inferred, ต้องวัด]
- **ไม่มีเสียง** กล้องส่งแต่ภาพ เสียงเกมต้องไปทางไมค์ (BlackHole ตามหัวข้อ 2) [code: extension มีแต่ video stream]
- ใช้เมื่อ: แชร์ projector ใน Discord ไม่ได้ หรือภาพดำ

## 5. ปัญหาที่รู้แล้ว

- ภาพดำหรือได้แค่ wallpaper: เกือบทุกครั้งเกิดจาก Discord ยังไม่ได้สิทธิ์ Screen & System Audio Recording ให้เปิดสิทธิ์แล้ว Quit & Reopen Discord ([pcrisk](https://www.pcrisk.com/blog/mac/13494-how-to-fix-discord-black-screen-when-sharing-your-screen-on-mac)) [forum] บางคนแก้ด้วยการปิด hardware acceleration ของ Discord [forum]
- Windowed projector ไม่ขึ้นเป็นเป้าแชร์ ([OBS forum 175221](https://obsproject.com/forum/threads/windowed-projector-preview-isnt-a-real-window.175221/)) [forum] สาเหตุที่น่าจะเป็นคือ Always on top (`NSScreenSaverWindowLevel`) [code + inferred]
- แอปที่เต็มจอใน Space แยกได้ภาพดำ [forum] ใช้ windowed projector แทน
- ไม่มีเสียงทั้งที่อยู่บน macOS 13+: บทความแนะนำให้แชร์หน้าต่างแทนจอ, ถอด Discord ออกจากรายการสิทธิ์แล้วอนุญาตใหม่, รีสตาร์ต ([tonal.fm](https://www.tonal.fm/patch/discord-screen-share-no-audio-mac)) [forum] Discord เองบอกให้ติดต่อ support [doc]
- prompt ขออนุญาตใหม่ทุกเดือน (macOS 15+) [forum]

## คำแนะนำสำหรับสายสตรีม

1. **ทางหลัก:** Go Live ใน voice channel แชร์ **Windowed Projector (Program)** ปิด Always on top ขนาด ≥ 1280×720 ไม่ minimize Discord ตั้ง 720p/30fps
2. **เสียง:** ให้มาจาก OBS monitoring source เสียงการ์ดตั้งเป็น Monitor and Output ไมค์ตั้งเป็น Monitor Off **หน่วงเสียงด้วย filter ใน chain ไม่ใช้ Sync Offset** (ส่งต่อให้ [#23](https://github.com/benzstuuuuu/game-live-translate/issues/23))
3. **ไม่แชร์ทั้งจอ** เพราะเสียงอาจไม่มา จอเดียวของ Mac โชว์ทุกอย่าง (Terminal, การแจ้งเตือน) และ fullscreen projector บังจอที่ใช้ทำงาน
4. **ทางสำรอง:** Virtual Camera (ภาพ) + BlackHole เป็นไมค์ Discord (เสียง)
5. **SRS:** แถบไทยใน `/overlay` ตั้งค่าเริ่มต้น 56–64 px ตัวหนา พื้นทึบ ไม่เกิน 2 บรรทัด (เสนอ, ต้องวัด)

## สิ่งที่ต้องวัดบนเครื่องจริง (computer use)

- [ ] Windowed Projector ขึ้นในตัวเลือกหน้าต่างของ Discord ทั้งตอนปิดและเปิด Always on top
- [ ] เพื่อนได้ยินเสียงเกมเมื่อแชร์ projector และแชร์ทั้งจอ
- [ ] เสียงยังมาไหมเมื่อหรี่เสียง Mac จนสุด / เมื่อ Monitoring Device = BlackHole
- [ ] หน่วงเสียงใน Discord ด้วย filter แล้วตรงกับภาพที่หน่วง 2 วินาที (ร่วมกับ #23)
- [ ] projector ที่ถูกหน้าต่างอื่นบังยังส่งภาพ
- [ ] ขนาดตัวอักษรไทยที่อ่านได้ที่ 720p30 ทั้งแบบเต็มจอและ tile
- [ ] Discord เห็น OBS Virtual Camera และส่งที่ความละเอียดเท่าไร
