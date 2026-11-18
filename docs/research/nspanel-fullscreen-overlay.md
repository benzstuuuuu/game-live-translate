# วาง NSPanel เหนือ viewer ที่เต็มจอบนจอนอก

- **Ticket:** [#6](https://github.com/benzstuuuuu/game-live-translate/issues/6) · map [#1](https://github.com/benzstuuuuu/game-live-translate/issues/1) · ผลส่งต่อให้ spike [#12](https://github.com/benzstuuuuu/game-live-translate/issues/12)
- **เกี่ยวกับ:** FR-OVL-04 (overlay อยู่เหนือ viewer ที่เต็มจอ ไม่รับคลิก ไม่แย่ง focus), FR-OVL-05 (edit mode ลาก/ปรับขนาด), FR-CAP-02 (ภาพที่จับต้องไม่มี overlay), ความเสี่ยง SRS "Overlay ขึ้นทับ viewer ที่เต็มจอไม่ได้"
- **ค้นเมื่อ:** 2026-11-18 · tauri-nspanel `v2.1` @ [`3ffa93e`](https://github.com/ahkohd/tauri-nspanel/tree/3ffa93ed14e0f8e857e43619b4157f2a685d35a4) (crate 2.1.1) · Tauri 2.12.6

## สรุป

1. **ทำได้ตามเอกสาร แต่ยังไม่มีใครยืนยันบนจอนอกกับ macOS 27** วิศวกร Apple DTS ตอบในฟอรัม (พ.ค. 2026) ว่า overlay จะขึ้นเหนือแอปอื่นที่ native full screen ได้ ต้องครบ 4 อย่าง: app เป็น `.accessory`, หน้าต่างเป็น `NSPanel` ที่มี `.nonactivatingPanel`, `collectionBehavior` มี `.canJoinAllSpaces` `.canJoinAllApplications` `.fullScreenAuxiliary` `.stationary` และ level เป็น `.screenSaver` เขาทดสอบบน macOS 26 กับ QuickTime Player ที่เต็มจอแล้ว ([DTS answer](https://developer.apple.com/forums/thread/826308?answerId=887785022#887785022))
2. **ค่าที่แนะนำ:** `ActivationPolicy::Accessory` · NSPanel (`tauri-nspanel` ≥ 2.1.1) · `add_style_mask(nonactivating_panel)` · level `ScreenSaver` (1000) · `can_join_all_spaces + full_screen_auxiliary + stationary + ignores_cycle + CanJoinAllApplications` · `hides_on_deactivate(false)` · `ignores_mouse_events(true)` · `can_become_key_window: false` · แสดงด้วย `show()` (`orderFrontRegardless`) อย่างเดียว ดู [หัวข้อ 6](#6-config-ที่แนะนำ)
3. **macOSPrivateApi:** ตั้งแต่ Tauri 2.12.1 ไม่ต้องเปิดเพื่อให้ transparency ทำงานแล้ว เพราะเปิดให้ตลอด ([changelog 2.12.1](https://github.com/tauri-apps/tauri/blob/4acdc0a2230ced11567a90962b07eedfa53dc3d2/crates/tauri/CHANGELOG.md#2121), [config ref](https://v2.tauri.app/reference/config/#macosprivateapi)) ข้อจำกัดใน SRS ที่ว่า "ต้องเปิด macOSPrivateApi" จึงล้าสมัยแล้ว
4. **FR-CAP-02:** อย่าพึ่ง `sharingType = .none` เพราะ Apple เรียกค่านี้ว่า "legacy constant that macOS no longer uses" ([doc](https://developer.apple.com/documentation/appkit/nswindow/sharingtype-swift.enum/none)) ให้ตัด overlay ด้วย `SCContentFilter` แทน ใช้ได้สองแบบ: จับเฉพาะหน้าต่าง viewer (`desktopIndependentWindow`) หรือจับจอแล้วตัดแอปตัวเองออก (`excludingApplications`)
5. **native full screen กับขยายในหน้าต่าง:** แบบขยายในหน้าต่างเสี่ยงน้อยกว่า เพราะไม่มี Space ใหม่ให้ overlay ต้องตามเข้าไป แต่ native full screen ก็ควรทำได้ด้วยค่าข้างบน ให้เก็บทางขยายในหน้าต่างไว้เป็นทางสำรอง VLC และ IINA มีโหมด full screen แบบเก่าที่ไม่สร้าง Space ใหม่

## 1. ต้องตั้งอะไรบ้างให้ขึ้นเหนือแอปอื่นที่ native full screen

### 1.1 Activation policy ต้องเป็น accessory

- DTS บอกว่า "macOS doesn't layer regular foreground apps above other apps' full-screen Spaces" และให้ใช้ `NSApp.setActivationPolicy(.accessory)` แบบเดียวกับ DropOver ([DTS answer](https://developer.apple.com/forums/thread/826308?answerId=887785022#887785022))
- Electron ทำแบบเดียวกัน ตอนเรียก `setVisibleOnAllWorkspaces(..., {visibleOnFullScreen: true})` Electron ซ่อน Dock และเปลี่ยน process เป็น UIElement ก่อน คอมเมนต์ในโค้ดเขียนว่า "Apple changed the underlying functionality of NSWindows starting with 10.14 to disallow NSWindows from floating on top of fullscreen apps" ([native_window_mac.mm L1293–1311](https://github.com/electron/electron/blob/97476cbe6fc952849eed4333bfaf6f79570dc8e6/shell/browser/native_window_mac.mm#L1293-L1311), [docs `skipTransformProcessType`](https://github.com/electron/electron/blob/97476cbe6fc952849eed4333bfaf6f79570dc8e6/docs/api/base-window.md?plain=1#L1385-L1397))
- `.accessory` เท่ากับตั้ง `LSUIElement = 1` ใน Info.plist: ไม่มีไอคอนใน Dock และไม่มี menu bar ของแอป ([doc](https://developer.apple.com/documentation/appkit/nsapplication/activationpolicy-swift.enum/accessory))
- Yokoten เป็น menu bar app อยู่แล้ว **ห้ามสลับเป็น `.regular` ระหว่างที่ overlay แสดงอยู่** เช่นตอนเปิดหน้าต่างหลักแล้วอยากให้มีไอคอนใน Dock ไม่อย่างนั้น overlay อาจหลุดจาก Space ของ viewer (อนุมานจากสองแหล่งข้างบน ยังไม่ได้ทดสอบ)

### 1.2 NSPanel + `.nonactivatingPanel`

- `nonactivatingPanel` คือ "a panel ... that does not activate the owning app" ([doc](https://developer.apple.com/documentation/appkit/nswindow/stylemask-swift.struct/nonactivatingpanel))
- DTS อธิบายว่า NSWindow ธรรมดาจะ activate แอปตอน order front ซึ่ง "can cause a full-screen app to exit its Space" แต่ NSPanel ที่มี `.nonactivatingPanel` ไม่ทำแบบนั้น ([DTS answer](https://developer.apple.com/forums/thread/826308?answerId=887785022#887785022))
- SwitchHosts (Tauri app) เคยลองแก้ด้วย NSWindow ธรรมดามาสามรอบแล้วไม่สำเร็จ สุดท้ายต้องแปลงเป็น nonactivating `NSPanel` ([บันทึกการแก้ L300–319](https://github.com/oldj/SwitchHosts/blob/4e428688d20196094307dd541e0be6c6f4c3c6bf/docs/macos-tray-fullscreen-bug.md#L300-L319))
- หน้าต่าง Tauri ธรรมดาที่ตั้ง `alwaysOnTop` กับ `visibleOnAllWorkspaces` ยังขึ้นเหนือแอปที่เต็มจอไม่ได้ ([tauri#9557](https://github.com/tauri-apps/tauri/issues/9557) ยัง open) เพราะ `visible_on_all_workspaces` ตั้งแค่ `CanJoinAllSpaces` ไม่มี `FullScreenAuxiliary` ([SwitchHosts L114–141](https://github.com/oldj/SwitchHosts/blob/4e428688d20196094307dd541e0be6c6f4c3c6bf/docs/macos-tray-fullscreen-bug.md#L114-L141); แหล่งรอง)

### 1.3 `collectionBehavior`

| flag | ความหมายตามเอกสาร | ใช้ไหม |
|---|---|---|
| `canJoinAllSpaces` | "The window can appear in all spaces" ([doc](https://developer.apple.com/documentation/appkit/nswindow/collectionbehavior-swift.struct/canjoinallspaces)) | ใช้ |
| `fullScreenAuxiliary` | "The window displays on the same space as the full screen window" ([doc](https://developer.apple.com/documentation/appkit/nswindow/collectionbehavior-swift.struct/fullscreenauxiliary)) | ใช้ (ข้อหลัก) |
| `canJoinAllApplications` (macOS 13+) | "can join the windows of other apps in full screen spaces when eligible. Use this collection behavior for floating windows and system overlays" ([doc](https://developer.apple.com/documentation/appkit/nswindow/collectionbehavior-swift.struct/canjoinallapplications)) | ใช้ตามที่ DTS แนะนำ (tauri-nspanel ไม่มี method ให้ ต้องใส่ bit เอง ดูหัวข้อ 6) |
| `stationary` | "Mission Control doesn't affect the window, so it stays visible and stationary" ([doc](https://developer.apple.com/documentation/appkit/nswindow/collectionbehavior-swift.struct/stationary)) | ใช้ (DTS ใส่ไว้ด้วย) |
| `ignoresCycle` | ไม่อยู่ในวงของ Cycle Through Windows ([doc](https://developer.apple.com/documentation/appkit/nswindow/collectionbehavior-swift.struct/ignorescycle)) | ใช้ กัน Cmd-` วนมาเจอ overlay |
| `moveToActiveSpace` | ย้ายหน้าต่างไป Space ที่ active ([doc](https://developer.apple.com/documentation/appkit/nswindow/collectionbehavior-swift.struct/movetoactivespace)) | ไม่ใช้ (ซ้ำซ้อนกับ `canJoinAllSpaces`) |

แอป open-source ที่ใช้ชุดนี้:
- Handy (tauri-nspanel): `can_join_all_spaces + full_screen_auxiliary` + `nonactivating_panel` + `PanelLevel::Status` + `no_activate` + `can_become_key_window: false` ([overlay.rs L28–35, L492–516](https://github.com/cjpais/Handy/blob/d2ee87178c04d27a247bf1e7cd30a4049da1702c/src-tauri/src/overlay.rs#L492-L516))
- ตัวอย่าง `fullscreen` ของ tauri-nspanel: `full_screen_auxiliary + can_join_all_spaces` + `nonactivating_panel` + `Floating` และ README เขียนว่า "The panel must be non-activating to display over fullscreen windows" ([main.rs L76–91](https://github.com/ahkohd/tauri-nspanel/blob/3ffa93ed14e0f8e857e43619b4157f2a685d35a4/examples/fullscreen/src-tauri/src/main.rs#L76-L91), [README L112–117](https://github.com/ahkohd/tauri-nspanel/blob/3ffa93ed14e0f8e857e43619b4157f2a685d35a4/examples/fullscreen/README.md#L112-L117))

### 1.4 Window level

- level ที่สูงกว่าจะทับ level ที่ต่ำกว่าเสมอ ("even the bottom window in a level will obscure the top window of the next level down") ([doc](https://developer.apple.com/documentation/appkit/nswindow/level-swift.struct))
- DTS: "You need `.screenSaver` (level 1000) — `.floating` (3) and `.statusBar` (25) both sit below full-screen content" ([DTS answer](https://developer.apple.com/forums/thread/826308?answerId=887785022#887785022))
- **ข้อมูลขัดกัน:** ตัวอย่าง fullscreen ของ tauri-nspanel ใช้ `Floating` ส่วน Handy ใช้ `Status` และทั้งสองบอกว่าขึ้นเหนือ fullscreen ได้ ([main.rs L76](https://github.com/ahkohd/tauri-nspanel/blob/3ffa93ed14e0f8e857e43619b4157f2a685d35a4/examples/fullscreen/src-tauri/src/main.rs#L76), [Handy L498](https://github.com/cjpais/Handy/blob/d2ee87178c04d27a247bf1e7cd30a4049da1702c/src-tauri/src/overlay.rs#L498)) จึงแนะนำ `ScreenSaver` ไว้ก่อน แล้วให้ spike ไล่ทดสอบทุก level
- **ระวัง OBS:** projector ของ OBS ที่เปิด "Always on top" ตั้ง level เป็น `NSScreenSaverWindowLevel` เท่ากับ overlay ([platform-osx.mm L113–129](https://github.com/obsproject/obs-studio/blob/717cfa8b7e633853cd21ad2cc656185b3b4255bb/frontend/utility/platform-osx.mm#L113-L129), [OBSProjector.cpp L30–34](https://github.com/obsproject/obs-studio/blob/717cfa8b7e633853cd21ad2cc656185b3b4255bb/frontend/widgets/OBSProjector.cpp#L30-L34)) ถ้าใช้ OBS เป็น viewer ให้ปิดตัวเลือกนั้น หรือตั้ง overlay เป็น `PanelLevel::Custom(1001)`
- ค่าใน tauri-nspanel: `Floating`=4, `Status`=25, `PopUpMenu`=101, `ScreenSaver`=1000, มี `Custom(i32)` ([builder.rs L104–118](https://github.com/ahkohd/tauri-nspanel/blob/3ffa93ed14e0f8e857e43619b4157f2a685d35a4/src/builder.rs#L104-L118)) โดย `Floating` ใน crate เป็น 4 แต่ DTS ระบุว่า `.floating` = 3 (ยังไม่ได้ตรวจกับ header)

### 1.5 ไม่แย่ง focus และไม่หายเมื่อแอปไม่ active

- **`hidesOnDeactivate` ต้องเป็น `false`** เพราะค่าเริ่มต้นของ NSPanel คือ `true` ([doc](https://developer.apple.com/documentation/appkit/nswindow/hidesondeactivate)) Yokoten แทบไม่เคยเป็นแอปที่ active ถ้าลืมตั้ง overlay จะหายไปเอง DTS ก็ตั้งค่านี้ไว้
- **key window:** NSWindow ที่ไม่มี `.titled` จะ `canBecomeKey = false` อยู่แล้ว ([doc](https://developer.apple.com/documentation/appkit/nswindow/canbecomekey)) แต่ tauri-nspanel override ค่านี้ใน `tauri_panel!` ได้ ให้ตั้ง `can_become_key_window: false` ตรง ๆ
- **วิธีแสดง:** `Panel::show()` ของ tauri-nspanel เรียก `orderFrontRegardless` ซึ่งไม่ทำให้เป็น key ([panel.rs L347–351](https://github.com/ahkohd/tauri-nspanel/blob/3ffa93ed14e0f8e857e43619b4157f2a685d35a4/src/panel.rs#L347-L351)) อย่าใช้ `show_and_make_key()`
- **ตอนสร้าง:** `PanelBuilder::no_activate(true)` จะตั้ง activation policy เป็น `Prohibited` ชั่วคราวระหว่าง build ([builder.rs L981–993](https://github.com/ahkohd/tauri-nspanel/blob/3ffa93ed14e0f8e857e43619b4157f2a685d35a4/src/builder.rs#L981-L993))
- **ตอนเปิดแอป:** Tauri 2.12.0 เพิ่ม `Builder::activate_ignoring_other_apps` ไว้คุมว่าจะ activate ตอนเปิดไหมถ้ามีแอปอื่น active อยู่ ([changelog L108](https://github.com/tauri-apps/tauri/blob/4acdc0a2230ced11567a90962b07eedfa53dc3d2/crates/tauri/CHANGELOG.md?plain=1#L108)) ควรตั้งให้ไม่ activate (ยังไม่ได้ดู signature)
- **ไม่รับคลิก:** `ignoresMouseEvents = true` ทำให้ "the window is transparent to mouse events" ([doc](https://developer.apple.com/documentation/appkit/nswindow/ignoresmouseevents)) Electron ก็ตั้งค่านี้ตรง ๆ ใน `setIgnoreMouseEvents` ([L1181–1188](https://github.com/electron/electron/blob/97476cbe6fc952849eed4333bfaf6f79570dc8e6/shell/browser/native_window_mac.mm#L1181-L1188))

## 2. native full screen (Space แยก) กับขยายเต็มจอในหน้าต่าง

| | native full screen | ขยายเต็มจอในหน้าต่าง (Zoom/Fill หรือ full screen แบบเก่า) |
|---|---|---|
| Space | viewer ได้ Space ใหม่บน TV ([Apple Support](https://support.apple.com/guide/mac-help/use-apps-in-full-screen-mchl9c21d2be/mac)) | อยู่ใน Space ปกติของ TV |
| overlay ต้องมี | ครบทั้ง 4 ข้อในหัวข้อ 1 | level สูงกว่า normal ก็พอ เพราะไม่มีการข้าม Space |
| ความเสี่ยง | พฤติกรรมเปลี่ยนตาม macOS (Electron อ้างถึงการเปลี่ยนใน 10.14), level ที่ใช้ได้ยังขัดกันอยู่ (หัวข้อ 1.4) | Zoom/Fill ยังเห็น menu bar และ Dock บน TV ([Apple Support: Fill](https://support.apple.com/guide/mac-help/change-desktop-dock-settings-mchlp1119/mac)) full screen แบบเก่าของ viewer อาจซ่อนให้ (ยังไม่ได้ตรวจ) |
| ถ้า Yokoten ถูก activate | อาจดึง viewer ออกจาก Space ของมัน ([DTS](https://developer.apple.com/forums/thread/826308?answerId=887785022#887785022)) | แค่หน้าต่างสลับลำดับ ไม่มีการสลับ Space |

**จอนอก:** ต้องเปิด "Displays have separate Spaces" (System Settings > Desktop & Dock > Mission Control) เพื่อให้แต่ละจอมี Space ของตัวเอง ตัวเลือกนี้ต้องเปิดถ้าจะใช้ Split View บนจออื่นหรือใช้ Stage Manager ([Apple Support](https://support.apple.com/guide/mac-help/change-desktop-dock-settings-mchlp1119/mac)) ถ้าปิดไว้ แอปที่เต็มจอบนจอหนึ่งจะทำให้จออื่นมืด ([MacMost](https://macmost.com/macs-with-two-displays-choose-windows-across-displays-or-separate-spaces.html); แหล่งรอง) ซึ่งจะทำให้หน้าต่างอ่านคำแปลบน Mac ใช้ไม่ได้ ส่วน key `defaults read com.apple.spaces spans-displays` (1 = ปิด) ยังไม่ได้ตรวจกับเอกสาร Apple

**กรอบ overlay:** `canJoinAllSpaces` ไม่ได้ย้ายหน้าต่างข้ามจอ ต้องวาง frame ของ panel ไว้ในพิกัดของ `NSScreen` ที่เป็น TV และวางใหม่ทุกครั้งที่จอหลุดหรือเปลี่ยน resolution (ข้อนี้อนุมานเอง ให้ spike ตรวจ)

**full screen แบบเก่าใน viewer ที่เป็นตัวเลือก (ให้ #3 ใช้ประกอบ):**
- VLC มีตัวเลือก "Use the native fullscreen mode" (`macosx-nativefullscreenmode`) ซึ่งในโค้ดปัจจุบันเปิดเป็นค่าเริ่มต้น ถ้าปิดจะใช้ full screen แบบเก่า ([macosx.m L86–87, L139](https://github.com/videolan/vlc/blob/1ca04de00ba74681614a926e4d5eaa3f80da95d4/modules/gui/macosx/main/macosx.m#L86-L139))
- IINA มี "Use legacy full screen" (`useLegacyFullScreen`, ค่าเริ่มต้น `false`) ([Localizable.strings L717](https://github.com/iina/iina/blob/171334e137357285de63128ce5322a9b9b0f9e75/iina/en.lproj/Localizable.strings#L717), [Preference.swift L1083](https://github.com/iina/iina/blob/171334e137357285de63128ce5322a9b9b0f9e75/iina/Preference.swift#L1083))
- OBS fullscreen projector เรียก Qt `showFullScreen()` ([OBSProjector.cpp L126–131](https://github.com/obsproject/obs-studio/blob/717cfa8b7e633853cd21ad2cc656185b3b4255bb/frontend/widgets/OBSProjector.cpp#L126-L131)) ยังไม่ได้ตรวจว่าบน macOS จะเป็น native full screen หรือไม่

## 3. tauri-nspanel กับ Tauri 2

- **รุ่น:** ใช้ crate `tauri-nspanel = "2.1.1"` (ออกบน crates.io 2026-10-08) เพราะรุ่นนี้แก้ปัญหาบน macOS 27 ที่ panel ถูกคลิกแล้วแอปถูก activate ([issue #123](https://github.com/ahkohd/tauri-nspanel/issues/123)) เครื่อง MVP รัน macOS 27 จึงต้องใช้รุ่นนี้ขึ้นไป
- **ใช้กับ Tauri 2:** crate ขึ้นกับ `tauri 2.8.5` และเปิด feature `macos-private-api` ให้เอง ([Cargo.toml L35](https://github.com/ahkohd/tauri-nspanel/blob/3ffa93ed14e0f8e857e43619b4157f2a685d35a4/Cargo.toml#L35))
- **ค่าที่ crate ตั้งให้ได้** ([docs/panel-builder.md](https://github.com/ahkohd/tauri-nspanel/blob/3ffa93ed14e0f8e857e43619b4157f2a685d35a4/docs/panel-builder.md), [builder.rs L173–258](https://github.com/ahkohd/tauri-nspanel/blob/3ffa93ed14e0f8e857e43619b4157f2a685d35a4/src/builder.rs#L173-L258)):

  | ค่าที่ต้องตั้ง | API |
  |---|---|
  | style mask | `add_style_mask(StyleMask::empty().nonactivating_panel())` |
  | level | `level(PanelLevel::ScreenSaver)` / `set_level(i64)` |
  | collection behavior | `CollectionBehavior::new().can_join_all_spaces().full_screen_auxiliary().stationary().ignores_cycle()` และ `from_raw(...)` |
  | ignoresMouseEvents | `ignores_mouse_events(bool)` / `set_ignores_mouse_events(bool)` ([panel.rs L594–598](https://github.com/ahkohd/tauri-nspanel/blob/3ffa93ed14e0f8e857e43619b4157f2a685d35a4/src/panel.rs#L594-L598)) |
  | hidesOnDeactivate | `hides_on_deactivate(false)` |
  | key/main | `tauri_panel! { config: { can_become_key_window: false, ... } }` |
  | transparent | `transparent(true)` (ตั้ง `clearColor` และ `opaque = false`) |
  | ไม่ activate ตอนสร้าง | `no_activate(true)` |

- **ค่าที่ crate ไม่มีให้:** `canJoinAllApplications` (มีใน `objc2_app_kit::NSWindowCollectionBehavior::CanJoinAllApplications` = `1<<18` ([objc2-app-kit 0.3.2](https://github.com/catboost/catboost/blob/master/contrib/crates/objc2-app-kit-0.3.2/src/generated/NSWindow.rs#L180-L181), สำเนา vendored)) ให้ OR bit ลงไปแล้วสร้างด้วย `CollectionBehavior::from_raw` ส่วน `sharingType` ไม่มี method แต่ไม่จำเป็นต้องใช้ (หัวข้อ 5) crate re-export `objc2_app_kit` มาให้แล้ว ([lib.rs](https://github.com/ahkohd/tauri-nspanel/blob/3ffa93ed14e0f8e857e43619b4157f2a685d35a4/src/lib.rs#L10))
- **ข้อควรระวัง:**
  - ใช้ `add_style_mask` เท่านั้น อย่าใช้ `style_mask()`/`set_style_mask()` ที่แทนที่ mask ทั้งชุด เพราะแอปจะ abort ทันทีด้วย non-unwinding panic ([#120](https://github.com/ahkohd/tauri-nspanel/issues/120)) และ `set_style_mask` เรียก private `_setPreventsActivation:` ให้ด้วย ([panel.rs L624–644](https://github.com/ahkohd/tauri-nspanel/blob/3ffa93ed14e0f8e857e43619b4157f2a685d35a4/src/panel.rs#L624-L644))
  - ห้ามเรียก `maximize()` หรือ `fullscreen()` กับ panel นี้เพราะจะ crash และต้อง deny `core:window:deny-internal-toggle-maximize` กันการดับเบิลคลิก drag region ([README L100–117](https://github.com/ahkohd/tauri-nspanel/blob/3ffa93ed14e0f8e857e43619b4157f2a685d35a4/examples/fullscreen/README.md#L100-L117))
  - ทุก method ของ panel ต้องเรียกบน main thread โดย crate ไม่ dispatch ให้ ถ้าเรียกจาก command ให้ใช้ `AppHandle::run_on_main_thread` ([docs/thread-safety.md](https://github.com/ahkohd/tauri-nspanel/blob/3ffa93ed14e0f8e857e43619b4157f2a685d35a4/docs/thread-safety.md))
- **macOSPrivateApi:** config reference เขียนว่า "Enables the transparent background API and sets the `fullScreenEnabled` preference to `true`. No-op in Tauri 2.12.1+ because the APIs are always enabled now" ([config ref](https://v2.tauri.app/reference/config/#macosprivateapi)) ตรงกับ PR [#16166](https://github.com/tauri-apps/tauri/pull/16166) (merge 2026-09-29) ที่ยังเก็บการจัดการ feature flag อัตโนมัติไว้กันของพังตอนอัปเดตไม่พร้อมกัน แนะนำให้ใช้ Tauri ≥ 2.12.1 และตั้ง `"macOSPrivateApi": true` ให้ตรงกับ feature ที่ tauri-nspanel เปิดอยู่ (ให้ spike ยืนยันว่า build ผ่าน)

## 4. สลับ click-through กับ edit mode (FR-OVL-05)

- **click-through (ปกติ):** `set_ignores_mouse_events(true)` คลิกทุกจุดจะทะลุไปถึง viewer
- **edit mode:** `set_ignores_mouse_events(false)` แล้วให้ webview แสดงขอบและที่จับ เมื่อเสร็จให้สลับกลับและบันทึก frame
  - **ลาก:** ใช้ `data-tauri-drag-region` หรือ `getCurrentWindow().startDragging()` โดยต้องมี permission `core:window:allow-start-dragging` ([README L87–103](https://github.com/ahkohd/tauri-nspanel/blob/3ffa93ed14e0f8e857e43619b4157f2a685d35a4/examples/fullscreen/README.md#L87-L103))
  - **ปรับขนาด:** ใช้ `startResizeDragging(direction)` จาก JS หรือเพิ่ม `.resizable()` ด้วย `add_style_mask` (ตัวอย่าง fullscreen ก็ทำแบบนี้)
  - **คลิกแรก:** ตั้ง `accept_first_mouse(true)` ตอนสร้าง เพื่อให้คลิกแรกบน panel ที่ไม่ใช่ key ส่งถึง webview ("Whether clicking an inactive window also clicks through to the webview on macOS", [config ref](https://v2.tauri.app/reference/config/#acceptfirstmouse))
  - **focus:** เพราะ panel เป็น nonactivating และ `can_become_key_window: false` viewer จึงยังเป็นแอปที่ active และถือ key อยู่ตลอด edit mode Tauri เคยมีข้อจำกัดว่าลากหน้าต่างที่ไม่มี focus ไม่ได้ ([tauri#4316](https://github.com/tauri-apps/tauri/issues/4316), บันทึกไว้ที่ `titleBarStyle: Overlay`) จึงต้องให้ spike ทดสอบว่าลากและปรับขนาดได้จริงไหมโดยไม่ทำให้ panel เป็น key
  - **ทางเข้าออก:** ใช้ menu bar item หรือ global shortcut หรือปุ่ม "เสร็จ" บน overlay (คลิกได้เฉพาะใน edit mode)

## 5. ตัด overlay ออกจากภาพ ScreenCaptureKit (FR-CAP-02)

- **อย่าใช้ `sharingType = .none`:** Apple เขียนว่า "A legacy constant that macOS no longer uses ... Don't use this value to hide or omit content from being captured" ([doc](https://developer.apple.com/documentation/appkit/nswindow/sharingtype-swift.enum/none)) มีกระทู้ในฟอรัม Apple หัวข้อ "On macOS 15.4+, NSWindow with kCGWindowSharingStateSharingNone still captured by ScreenCaptureKit" ([thread 792152](https://developer.apple.com/forums/thread/792152), อ่านได้แค่ชื่อกระทู้ เนื้อหาติด bot check) `contentProtected` ของ Tauri และ `setContentProtection` ของ Electron ใช้กลไกเดียวกันนี้ ([Tauri](https://v2.tauri.app/reference/config/#contentprotected), [Electron L1190–1193](https://github.com/electron/electron/blob/97476cbe6fc952849eed4333bfaf6f79570dc8e6/shell/browser/native_window_mac.mm#L1190-L1193)) ถ้าตั้งค่านี้ แอปอัดจออื่นอย่าง OBS ก็อาจไม่เห็นคำแปลด้วย
- **ทาง A: จับเฉพาะหน้าต่าง viewer** ด้วย `SCContentFilter(desktopIndependentWindow:)` ซึ่ง "captures only the specified window" ([doc](https://developer.apple.com/documentation/screencapturekit/sccontentfilter/init(desktopindependentwindow:))) WWDC22 อธิบายว่า single window filter "always includes full window content even when the source window is off-screen or occluded. It's display and space independent ... Pop-up or child windows are not included" ([WWDC22 10155](https://developer.apple.com/videos/play/wwdc2022/10155/)) overlay จึงไม่มีทางอยู่ในภาพ แต่ `sourceRect` ใช้กับ filter แบบนี้ไม่ได้ ("doesn't reference this value when you capture a single window", [doc](https://developer.apple.com/documentation/screencapturekit/scstreamconfiguration/sourcerect)) ต้องจับทั้งหน้าต่างแล้วค่อยตัดกรอบ subtitle เอง
- **ทาง B: จับจอ TV แล้วตัดแอปตัวเองออก** ด้วย `SCContentFilter(display:excludingApplications:exceptingWindows:)` ([doc](https://developer.apple.com/documentation/screencapturekit/sccontentfilter/init(display:excludingapplications:exceptingwindows:))) โดยหา app ของตัวเองจาก bundle ID แบบในตัวอย่าง WWDC22 ที่ใช้กันภาพสะท้อนซ้อน ([WWDC22 10156, 8:32](https://developer.apple.com/videos/play/wwdc2022/10156/)) หน้าต่างใหม่หรือ child window ของแอปที่ถูกตัดก็จะถูกตัดตามไปด้วย ([WWDC22 10155](https://developer.apple.com/videos/play/wwdc2022/10155/)) และใช้ `sourceRect` ตัดเฉพาะกรอบ subtitle ได้ตั้งแต่ต้นทาง จึงประหยัดแรงเครื่องกว่า
  - **ข้อควรระวัง:** `SCShareableContent` คืนเฉพาะ content ที่อยู่บนจอเมื่อขอแบบ `onScreenWindowsOnly: true` ([WWDC22 10156](https://developer.apple.com/videos/play/wwdc2022/10156/)) ถ้าตอนสร้าง filter ยังไม่มีหน้าต่างไหนของ Yokoten บนจอ แอปอาจไม่อยู่ในรายการให้ตัดออก ให้สร้าง filter หลัง `panel.show()` หรือขอด้วย `onScreenWindowsOnly: false` (ยังไม่ได้ทดสอบ)
- **crate `screencapturekit` รองรับทั้งสองทาง:** `with_window(...)` และ `with_display(...).with_excluding_applications(apps, excepting_windows)` ([content_filter.rs](https://github.com/doom-fish/screencapturekit-rs/blob/c11c9a95f672c6fafe34dc31d07749b0d58d6e1e/src/stream/content_filter.rs#L496-L575))
- **ใครเลือก:** การเลือกระหว่างทาง A กับ B เป็นงานของ #11 เพราะขึ้นกับว่าจับหน้าต่าง viewer ตอน native full screen ได้ไหม ผลจาก ticket นี้คือ overlay ถูกตัดออกได้ทั้งสองทาง โดยไม่ต้องพึ่ง `sharingType`

## 6. Config ที่แนะนำ

`src-tauri/Cargo.toml`

```toml
[dependencies]
tauri = { version = "2.12.1", features = ["macos-private-api"] } # >= 2.12.1: transparency ไม่ต้องพึ่ง macOSPrivateApi แล้ว
tauri-nspanel = "2.1.1"                                          # >= 2.1.1: แก้ nonactivating บน macOS 27 (#123)
```

`src-tauri/tauri.conf.json` (บางส่วน; overlay สร้างในโค้ด ไม่ได้ประกาศใน `windows`)

```json
{
  "app": {
    "macOSPrivateApi": true,
    "windows": []
  }
}
```

`src-tauri/capabilities/overlay.json`

```json
{
  "identifier": "overlay",
  "windows": ["overlay"],
  "permissions": [
    "core:event:default",
    "core:window:allow-start-dragging",
    "core:window:allow-start-resize-dragging",
    "core:window:deny-internal-toggle-maximize"
  ]
}
```

`src-tauri/src/overlay.rs`

```rust
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Position, Size, WebviewUrl};
use tauri_nspanel::{
    objc2_app_kit::NSWindowCollectionBehavior, tauri_panel, CollectionBehavior, ManagerExt,
    PanelBuilder, PanelLevel, StyleMask,
};

pub const OVERLAY: &str = "overlay";

tauri_panel! {
    panel!(SubtitleOverlay {
        config: {
            can_become_key_window: false, // viewer ถือ key ตลอด (FR-OVL-04)
            can_become_main_window: false,
            is_floating_panel: true
        }
    })
}

fn overlay_behavior() -> CollectionBehavior {
    let flags = CollectionBehavior::new()
        .can_join_all_spaces()
        .full_screen_auxiliary()
        .stationary()
        .ignores_cycle()
        .value()
        | NSWindowCollectionBehavior::CanJoinAllApplications; // macOS 13+, crate ไม่มี method
    CollectionBehavior::from_raw(flags)
}

/// เรียกบน main thread (เช่นใน `setup`) หลัง `set_activation_policy(Accessory)`
pub fn create_overlay(
    app: &AppHandle,
    pos: LogicalPosition<f64>, // พิกัดบนจอ TV
    size: LogicalSize<f64>,
) -> tauri::Result<()> {
    let panel = PanelBuilder::<_, SubtitleOverlay>::new(app, OVERLAY)
        .url(WebviewUrl::App("overlay.html".into()))
        .position(Position::Logical(pos))
        .size(Size::Logical(size))
        .level(PanelLevel::ScreenSaver) // หรือ Custom(1001) ถ้า viewer เป็น OBS projector แบบ always-on-top
        .add_style_mask(StyleMask::empty().nonactivating_panel()) // อย่าใช้ style_mask() (#120)
        .collection_behavior(overlay_behavior())
        .hides_on_deactivate(false) // ค่าเริ่มต้นของ NSPanel คือ true
        .ignores_mouse_events(true) // click-through
        .transparent(true)
        .has_shadow(false)
        .no_activate(true)
        .with_window(|w| {
            w.decorations(false)
                .transparent(true)
                .focused(false)
                .accept_first_mouse(true) // ให้ edit mode รับคลิกแรกได้
                .skip_taskbar(true)
                .visible(false)
        })
        .build()?;
    panel.show(); // orderFrontRegardless: ไม่เป็น key ไม่ activate
    Ok(())
}

#[tauri::command]
pub fn set_overlay_edit_mode(app: AppHandle, editing: bool) -> Result<(), String> {
    let handle = app.clone();
    app.run_on_main_thread(move || {
        if let Ok(panel) = handle.get_webview_panel(OVERLAY) {
            panel.set_ignores_mouse_events(!editing);
        }
    })
    .map_err(|e| format!("cannot schedule overlay update on main thread: {e}"))?;
    app.emit_to(OVERLAY, "overlay-edit-mode", editing)
        .map_err(|e| format!("cannot notify overlay webview: {e}"))
}
```

`setup` (ส่วนที่เกี่ยวข้อง)

```rust
tauri::Builder::default()
    .plugin(tauri_nspanel::init())
    .setup(|app| {
        #[cfg(target_os = "macos")]
        app.set_activation_policy(tauri::ActivationPolicy::Accessory); // ห้ามสลับเป็น Regular ระหว่าง overlay แสดง
        overlay::create_overlay(app.handle(), tv_origin, bar_size)?;
        Ok(())
    })
```

## 7. ยังไม่ยืนยัน: spike #12 ต้องทดสอบ

ตั้งเครื่องตาม MVP: MacBook Air M2 + macOS 27 + TV เป็นจอนอก + เปิด "Displays have separate Spaces" + Yokoten spike เป็น accessory

1. **ขึ้นเหนือ native full screen บน TV:** ทดสอบกับ viewer ที่ #3 เลือก (อย่างน้อย QuickTime) ไล่ level `Floating` / `Status` / `ScreenSaver` เพราะข้อมูลขัดกันในหัวข้อ 1.4 และลองทั้งมีและไม่มี `CanJoinAllApplications` บันทึกว่าชุดไหนน้อยที่สุดที่ยังขึ้นได้
2. **ช่วงเปลี่ยนสถานะ:** แสดง overlay ก่อนและหลัง viewer เข้า full screen, viewer เข้าออก full screen ซ้ำ, สลับ Space บนจอ Mac, เปิด Mission Control, ถอดและเสียบ HDMI ใหม่, เปลี่ยน resolution ของ TV
3. **focus:** ระหว่างอัปเดตข้อความและสลับ show/hide viewer ต้องยังเป็นแอปที่ active (menu bar ไม่เปลี่ยนเป็น Yokoten), ปุ่มลัดของ viewer ยังทำงาน และ viewer ไม่หลุดออกจาก full screen (ใช้ทดสอบซ้ำ issue #123 บน macOS 27 ด้วย)
4. **click-through:** คลิกตรง overlay แล้ว viewer ต้องได้รับ เช่นแถบควบคุมของ QuickTime ขึ้นมา
5. **edit mode:** ลากด้วย drag region หรือ `startDragging`, ปรับขนาดด้วย `startResizeDragging` หรือ `.resizable()` ทั้งหมดต้องทำได้โดย panel ไม่เป็น key และ viewer ไม่เสีย focus, ดับเบิลคลิกแล้วไม่ maximize และไม่ crash
6. **FR-CAP-02:** stream แบบ A (`desktopIndependentWindow`) และแบบ B (`excludingApplications` + `sourceRect`) ต้องไม่มีข้อความ overlay ในเฟรม ตรวจว่า Yokoten อยู่ใน `SCShareableContent.applications` ตอนสร้าง filter และถ้ามีเวลา ลอง `sharingType = .none` อย่างเดียวบน macOS 27 เพื่อบันทึกว่าตัดไม่ได้
7. **ทางสำรอง:** ทำข้อ 1, 3, 4, 6 ซ้ำกับ viewer แบบขยายในหน้าต่าง (VLC ปิด native fullscreen / IINA legacy full screen / Zoom)
8. **OBS:** projector ที่เปิด "Always on top" กับ overlay `ScreenSaver` เทียบกับ `Custom(1001)`
9. **build:** Tauri ≥ 2.12.1 + tauri-nspanel 2.1.1 ต้อง build ผ่านทั้งตอน `macOSPrivateApi` เป็น `true` และ `false` และพื้นหลังโปร่งใสต้องไม่มี flash สีขาวบน TV
10. **ตัวเลือก:** ใส่ `focusable(false)` ใน `with_window` แบบที่ Handy ทำ ([L508](https://github.com/cjpais/Handy/blob/d2ee87178c04d27a247bf1e7cd30a4049da1702c/src-tauri/src/overlay.rs#L508)) แล้วดูว่าขวางการลากใน edit mode ไหม

## 8. ผลกระทบต่อ ticket อื่น

- **#3 (viewer):** viewer ที่มี full screen แบบไม่สร้าง Space (VLC, IINA) ได้เปรียบตรงที่ใช้เป็นทางสำรองได้ ถ้าเลือก OBS projector ต้องปิด "Always on top" เพราะ level ชนกับ overlay และต้องเปิด "Displays have separate Spaces"
- **#4, #16 (crate/stack):** ปักรุ่น `tauri-nspanel` ≥ 2.1.1 และ `tauri` ≥ 2.12.1 ข้อจำกัด "ต้องเปิด macOSPrivateApi เพื่อ transparency" ใน SRS ล้าสมัยแล้ว
- **#11 (capture):** FR-CAP-02 ทำด้วย `SCContentFilter` ไม่ใช่ `sharingType` ถ้าใช้ `desktopIndependentWindow` จะใช้ `sourceRect` ไม่ได้ ต้องตัดกรอบเองหลังจับภาพ
- **SRS (FR-APP / FR-OVL):** Yokoten ต้องเป็น `.accessory` ตลอดเวลาที่ overlay แสดง ถ้ามีฟีเจอร์ "แสดงไอคอนใน Dock" ต้องทดสอบผลต่อ FR-OVL-04 ก่อน
