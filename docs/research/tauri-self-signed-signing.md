# เซ็นแอป Tauri 2 ด้วย self-signed cert ให้สิทธิ์ TCC ไม่หลุด

> Research สำหรับ issue [#7](https://github.com/benzstuuuuu/game-live-translate/issues/7) ใน map [#1](https://github.com/benzstuuuuu/game-live-translate/issues/1)
> วันที่: 2026-11-18 · Tauri source ที่อ่าน: `tauri-apps/tauri` commit `2fd85731` (branch `dev`, 18 พ.ย. 2026)
> ป้าย **[ทดลองแล้ว]** = รันจริงบน macOS 26.5.1 (Apple Silicon) ในเครื่องนี้ ไม่ใช่ macOS 27 ของเครื่องเป้าหมาย · **[ยังไม่ยืนยัน]** = ยังไม่มีแหล่งหลักหรือยังไม่ได้ทดสอบ

## สรุปสั้น

- TCC จำแอปด้วย **designated requirement (DR)** ของ code signature แอปที่เซ็นแบบ ad-hoc มี DR ผูกกับ `cdhash` ของ build นั้น build ใหม่ก็เป็นแอปใหม่ สิทธิ์จึงหลุด ([TN3127](https://developer.apple.com/documentation/technotes/tn3127-inside-code-signing-requirements))
- เซ็นด้วย self-signed code-signing cert ตัวเดิมทุก build จะได้ DR เป็น `identifier "<bundle id>" and certificate leaf = H"<SHA-1 ของ cert>"` ซึ่งไม่ขึ้นกับเนื้อ binary build ใหม่จึงผ่าน DR ของ build เก่า **[ทดลองแล้ว]**
- ตั้งใน Tauri แค่ `bundle.macOS.signingIdentity` = ชื่อ cert (หรือ env `APPLE_SIGNING_IDENTITY`) Tauri จะรัน `codesign --force -s <identity> --options runtime [--entitlements …]` ให้ตอน `tauri build` ([source](https://github.com/tauri-apps/tauri/blob/2fd85731ceb871bfd20598b781dc711c33b5062a/crates/tauri-macos-sign/src/keychain.rs#L191-L232))
- สิ่งที่ทำให้สิทธิ์หลุดได้แม้เซ็นถูก: เปลี่ยน `identifier` ใน `tauri.conf.json`, สร้าง cert ใหม่ (แม้ชื่อเดิม), cert หมดอายุแล้วต้องออกใหม่ ดังนั้นตั้งอายุ cert ยาว และ export `.p12` เก็บไว้
- Info.plist ต้องมี `NSAudioCaptureUsageDescription` สำหรับ Core Audio tap ([Apple](https://developer.apple.com/documentation/coreaudio/capturing-system-audio-with-core-audio-taps)) ส่วน `NSScreenCaptureUsageDescription` ใส่ไว้ด้วยเพราะเอกสาร ScreenCaptureKit ฉบับปัจจุบันระบุ ([Apple](https://developer.apple.com/documentation/screencapturekit)) แต่ยังไม่มีหน้า reference ของคีย์นี้ **[ยังไม่ยืนยันบน macOS]**
- `tauri dev` ไม่เซ็นอะไรเลย binary ใน `target/debug` มีแค่ linker ad-hoc signature และเมื่อรันจาก terminal ตัว TCC นับ terminal เป็น "responsible code" สิทธิ์จึงไปผูกกับ Terminal/Ghostty/Zed ไม่ใช่กับแอปเรา งานที่ต้องใช้สิทธิ์ TCC จริงควรทดสอบด้วย `tauri build --debug --bundles app` ที่เซ็นแล้ว
- Gatekeeper ไม่ขวางแอปที่ build เองบนเครื่องเดียวกันเพราะไม่มี quarantine attribute แม้ `spctl` จะตอบ `rejected` **[ทดลองแล้ว]**

## 1. ทำไม ad-hoc ทำให้สิทธิ์หลุด และ TCC ผูกกับอะไร

TN3127 อธิบายว่า DR คือ "วิธีที่ code บอกว่าถ้าเจอฉันอีก จะรู้ได้อย่างไรว่าเป็นฉันจริง" และระบุตรง ๆ ว่า "Ad hoc signed code … has a DR but it's tied to that specific version of the code … If the app accesses the microphone, macOS prompts you … If you tweak the code and run it again, macOS repeats that prompt" ([TN3127 § Designated requirement](https://developer.apple.com/documentation/technotes/tn3127-inside-code-signing-requirements#Designated-requirement)) ข้อความเดียวกันยกตัวอย่างว่าระบบใช้ DR ตัดสินว่าแอปสองตัว "share access to privacy-protected resources" หรือไม่ ([TN3127 § Mutually compatible designated requirements](https://developer.apple.com/documentation/technotes/tn3127-inside-code-signing-requirements#Mutually-compatible-designated-requirements)) TCC ใช้ service ชื่อ `kTCCServiceScreenCapture` และ `kTCCServiceAudioCapture` สำหรับสองสิทธิ์ที่เราต้องการ (เห็นจาก strings ใน `/System/Library/PrivateFrameworks/TCC.framework/Support/tccd` บนเครื่องนี้ ไม่ใช่เอกสารทางการ)

ผลทดลองในเครื่องนี้ (temp keychain, แอปทดสอบ `com.example.yokoten-test` สอง build ที่ code ต่างกัน) **[ทดลองแล้ว]**:

| วิธีเซ็น | `codesign -dr -` | build 2 ผ่าน DR ของ build 1? |
|---|---|---|
| linker default (สิ่งที่ `cargo build` ได้บน Apple Silicon) | `flags=0x20002(adhoc,linker-signed)`, `# designated => cdhash H"…"` | ไม่ผ่าน (cdhash ต่างกัน) |
| `codesign -s -` (ad-hoc, เท่ากับ `"signingIdentity": "-"` ของ Tauri) | `# designated => cdhash H"…"` (ต่างกันทุก build) | ไม่ผ่าน |
| self-signed cert + `--options runtime` | `designated => identifier "com.example.yokoten-test" and certificate leaf = H"11adf4…"` | **ผ่าน** (`explicit requirement satisfied`) |

ข้อสังเกตจากตาราง:

- DR แบบ self-signed ผูกกับ **SHA-1 ของ leaf certificate** และ **code signing identifier** (สำหรับ .app คือ `CFBundleIdentifier` ที่ Tauri ตั้งจาก `identifier`) ไม่มี Team ID (`TeamIdentifier=not set`) ต่างจาก DR ของ Apple Development / Developer ID ที่ใช้ `anchor apple generic` กับ Team ID ([TN3127 § Default and Xcode designated requirements](https://developer.apple.com/documentation/technotes/tn3127-inside-code-signing-requirements#Default-and-Xcode-designated-requirements))
- ผลตามมา: ถ้าสร้าง cert ใหม่ แม้ใช้ชื่อเดิม hash เปลี่ยน TCC จะมองเป็นแอปใหม่ ต้องให้สิทธิ์ใหม่หนึ่งครั้ง ถ้าเปลี่ยน bundle identifier ก็เช่นกัน
- การที่ build 2 ผ่าน DR ของ build 1 คือเงื่อนไขที่ TCC ต้องใช้ แต่การทดสอบว่า TCC จำสิทธิ์ข้าม build จริงต้องกดอนุญาตใน System Settings ซึ่งทำแบบอัตโนมัติไม่ได้ **[ยังไม่ยืนยันแบบ end-to-end]** ให้ยืนยันใน spike แรกที่ใช้ ScreenCaptureKit (#11)

## 2. สร้าง self-signed code-signing cert

### ทางหลัก: Keychain Access (ทำครั้งเดียว)

ขั้นตอนตาม [Apple: Create self-signed certificates in Keychain Access](https://support.apple.com/guide/keychain-access/create-self-signed-certificates-kyca8916/mac):

1. เปิด Keychain Access > เมนู Keychain Access > Certificate Assistant > Create a Certificate
2. Name: ตั้งชื่อคงที่ เช่น `Yokoten Self-Signed` (ชื่อนี้คือค่าที่ใส่ใน `signingIdentity`)
3. Identity Type: **Self Signed Root** · Certificate Type: **Code Signing**
4. ติ๊ก **Let me override defaults** แล้วตั้ง Validity Period ให้ยาว (เช่น 3650 วันขึ้นไป) ค่าที่เหลือปล่อยตามเดิม ให้ cert ลงใน **login** keychain
5. Export identity (cert + private key) เป็น `.p12` ใส่รหัสผ่าน แล้วเก็บนอก repo (เช่น password manager) ไว้ใช้ตอนลงเครื่องใหม่ ([Apple: Import and export keychain items](https://support.apple.com/guide/keychain-access/import-and-export-keychain-items-kyca35961/mac))

เหตุผลข้อ 4–5: DR ผูกกับ hash ของ cert (ข้อ 1) การต่ออายุหรือออก cert ใหม่จึงทำให้สิทธิ์หลุดเสมอ พฤติกรรมของ `codesign` เมื่อ cert หมดอายุ (เซ็นไม่ได้หรือเซ็นได้) ยังไม่ได้ทดสอบ **[ยังไม่ยืนยัน]** จึงเลี่ยงด้วยอายุยาว

### เรื่อง trust settings

คู่มือออนไลน์หลายแห่งบอกให้ตั้ง cert เป็น "Always Trust" สำหรับ Code Signing ก่อนใช้ ผลทดลองในเครื่องนี้ **[ทดลองแล้ว]**:

- cert ที่ไม่ได้ตั้ง trust แสดงใน `security find-identity -p codesigning` เป็น `(CSSMERR_TP_NOT_TRUSTED)` และไม่อยู่ใน "Valid identities only"
- แต่ `codesign -s "<ชื่อ>"` **เซ็นได้** และ `codesign --verify --strict -vv` ตอบ `valid on disk` + `satisfies its Designated Requirement` ขอแค่ keychain ที่เก็บ identity อยู่ใน keychain search list (login keychain อยู่ในนั้นอยู่แล้ว) ถ้า keychain ไม่อยู่ใน search list จะได้ `no identity found` แม้ส่ง `--keychain` ก็ตาม
- สรุป: **ไม่จำเป็นต้องตั้ง trust** ถ้าอยากให้ `security find-identity -v -p codesigning` แสดงผล ก็ตั้ง Trust > Code Signing = Always Trust ได้ ([Apple: Change the trust settings of a certificate](https://support.apple.com/guide/keychain-access/change-the-trust-settings-of-a-certificate-kyca11871/mac)) ไม่มีผลต่อ DR การที่ TCC ไม่สนใจ trust ยังไม่ยืนยันแบบ end-to-end **[ยังไม่ยืนยัน]**

### ทางเลือก: CLI (สคริปต์ได้)

ใช้ตอนลงเครื่องใหม่หรืออยากทำซ้ำได้ (ทดลองแล้วด้วย `/usr/bin/openssl` ซึ่งเป็น LibreSSL):

```sh
cat > cert.cnf <<'EOF'
[req]
distinguished_name = dn
x509_extensions = ext
prompt = no
[dn]
CN = Yokoten Self-Signed
[ext]
basicConstraints = critical, CA:false
keyUsage = critical, digitalSignature
extendedKeyUsage = critical, codeSigning
EOF
/usr/bin/openssl req -x509 -newkey rsa:2048 -nodes -keyout key.pem -out cert.pem -days 7300 -config cert.cnf
security import key.pem  -k ~/Library/Keychains/login.keychain-db -T /usr/bin/codesign
security import cert.pem -k ~/Library/Keychains/login.keychain-db
rm key.pem   # อย่า commit, เก็บ .p12 ที่ export ไว้แทน
```

`-T /usr/bin/codesign` ให้ `codesign` ใช้ private key ได้โดยไม่ถามทุกครั้ง ([`security` man page](https://keith.github.io/xcode-man-pages/security.1.html)) ครั้งแรกที่เซ็นอาจมี dialog ขอสิทธิ์ใช้ key ให้กด Always Allow

## 3. ตั้ง Tauri

### `tauri.conf.json`

```json
{
  "identifier": "<bundle id ที่ตกลงกัน ห้ามเปลี่ยนภายหลัง>",
  "bundle": {
    "macOS": {
      "signingIdentity": "Yokoten Self-Signed",
      "entitlements": "./Entitlements.plist",
      "minimumSystemVersion": "26.0"
    }
  }
}
```

สิ่งที่ Tauri ทำจริง (จาก source):

- `signingIdentity` ไปเป็น `codesign --force -s <identity>` ส่วน `hardenedRuntime` (ค่าเริ่มต้น `true`) เพิ่ม `--options runtime` และ `entitlements` เพิ่ม `--entitlements <path>` ไม่มี `--timestamp` และไม่ส่ง `--identifier` (codesign ใช้ `CFBundleIdentifier` เอง) ([keychain.rs](https://github.com/tauri-apps/tauri/blob/2fd85731ceb871bfd20598b781dc711c33b5062a/crates/tauri-macos-sign/src/keychain.rs#L191-L232), [config.rs `MacConfig`](https://github.com/tauri-apps/tauri/blob/2fd85731ceb871bfd20598b781dc711c33b5062a/crates/tauri-utils/src/config.rs))
- env `APPLE_SIGNING_IDENTITY` override ค่าใน config ได้ ([rust.rs](https://github.com/tauri-apps/tauri/blob/2fd85731ceb871bfd20598b781dc711c33b5062a/crates/tauri-cli/src/interface/rust.rs#L1455-L1463)) ส่วน `APPLE_CERTIFICATE` + `APPLE_CERTIFICATE_PASSWORD` ใช้ import `.p12` ลง temp keychain สำหรับ CI ([sign.rs](https://github.com/tauri-apps/tauri/blob/2fd85731ceb871bfd20598b781dc711c33b5062a/crates/tauri-bundler/src/bundle/macos/sign.rs#L19-L44))
- **ถ้าไม่ตั้ง identity เลย Tauri จะไม่เซ็น** (`keychain()` คืน `None`) .app ที่ได้มีแค่ linker ad-hoc signature ของ binary ข้างใน ([sign.rs](https://github.com/tauri-apps/tauri/blob/2fd85731ceb871bfd20598b781dc711c33b5062a/crates/tauri-bundler/src/bundle/macos/sign.rs#L19-L44), [app.rs](https://github.com/tauri-apps/tauri/blob/2fd85731ceb871bfd20598b781dc711c33b5062a/crates/tauri-bundler/src/bundle/macos/app.rs#L113-L150))
- Tauri เซ็น sidecar/framework ก่อน แล้วเซ็นตัว .app ทีหลัง (inside-out) และลบ extended attributes ด้วย `xattr -crs` ก่อนเซ็น ([app.rs](https://github.com/tauri-apps/tauri/blob/2fd85731ceb871bfd20598b781dc711c33b5062a/crates/tauri-bundler/src/bundle/macos/app.rs#L113-L150))
- ถ้าไม่มี credential สำหรับ notarization Tauri แค่เตือน `skipping app notarization` แล้ว build ต่อ ([app.rs](https://github.com/tauri-apps/tauri/blob/2fd85731ceb871bfd20598b781dc711c33b5062a/crates/tauri-bundler/src/bundle/macos/app.rs#L130-L146))
- เอกสาร Tauri อธิบายเฉพาะ cert จาก Apple กับ ad-hoc (`"-"`) และเตือนว่า ad-hoc ยังต้อง allow ใน Privacy & Security ([Tauri: macOS Code Signing](https://v2.tauri.app/distribute/sign/macos/)) ไม่ได้พูดถึง self-signed แต่จาก source ค่า `signingIdentity` ส่งตรงให้ `codesign` จึงใช้ชื่อ self-signed cert ได้เหมือนชื่อ cert อื่น

### `src-tauri/Info.plist` (Tauri merge ให้อัตโนมัติ)

Tauri merge `src-tauri/Info.plist` เข้ากับ Info.plist ที่ CLI สร้าง ([Tauri: macOS Application Bundle § Native configuration](https://v2.tauri.app/distribute/macos-application-bundle/)) หรือชี้ไฟล์อื่นด้วย `bundle.macOS.infoPlist` ([config.rs](https://github.com/tauri-apps/tauri/blob/2fd85731ceb871bfd20598b781dc711c33b5062a/crates/tauri-utils/src/config.rs))

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>NSAudioCaptureUsageDescription</key>
  <string>Yokoten ฟังเสียงจากเกมเพื่อถอดความและแปลเป็นภาษาไทย</string>
  <key>NSScreenCaptureUsageDescription</key>
  <string>Yokoten อ่าน subtitle จากหน้าต่างเกมเพื่อแปลเป็นภาษาไทย</string>
</dict>
</plist>
```

- `NSAudioCaptureUsageDescription`: macOS 14.2+ ([Apple](https://developer.apple.com/documentation/bundleresources/information-property-list/nsaudiocaptureusagedescription)) และ "To capture audio with a tap, you need to include the NSAudioCaptureUsageDescription key in your Info.plist" ระบบจะถามสิทธิ์ "the first time you start recording from an aggregate device that contains a tap" ([Apple: Capturing system audio with Core Audio taps](https://developer.apple.com/documentation/coreaudio/capturing-system-audio-with-core-audio-taps)) ไม่มี public API สำหรับเช็กหรือขอสิทธิ์นี้ล่วงหน้า ([AudioCap README](https://github.com/insidegui/AudioCap) ซึ่งเป็น sample ที่ไม่ใช่ของ Apple)
- `NSScreenCaptureUsageDescription`: เอกสาร ScreenCaptureKit ฉบับปัจจุบันเขียนว่า "add a NSScreenCaptureUsageDescription key with a description of why your app requires screen recording access" ([Apple: ScreenCaptureKit](https://developer.apple.com/documentation/screencapturekit)) แต่หน้า [Protected resources](https://developer.apple.com/documentation/bundleresources/protected-resources) ยังไม่มีคีย์นี้ และ URL reference ของคีย์ตอบ 404 อาจเป็นคีย์ใหม่สำหรับ iOS/tvOS/visionOS 27 ใส่ไว้ไม่เสียหาย **[ยังไม่ยืนยันว่า macOS 27 ใช้คีย์นี้]**

### `src-tauri/Entitlements.plist`

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>com.apple.security.device.audio-input</key>
  <true/>
</dict>
</plist>
```

- ScreenCaptureKit ไม่ต้องมี entitlement สำหรับแอปนอก sandbox สิทธิ์มาจาก TCC อย่างเดียว ([Apple: ScreenCaptureKit](https://developer.apple.com/documentation/screencapturekit))
- `com.apple.security.device.audio-input` เป็น resource-access entitlement ของ hardened runtime ([Apple: Configuring the hardened runtime](https://developer.apple.com/documentation/xcode/configuring-the-hardened-runtime)) AudioCap ใส่ไว้ แต่ AudioCap เป็นแอป sandbox ([AudioCap.entitlements](https://github.com/insidegui/AudioCap/blob/main/AudioCap/AudioCap.entitlements)) ยังไม่มีแหล่งหลักบอกว่า tap ภายใต้ hardened runtime ต้องใช้คีย์นี้ ใส่ไว้กันพลาดเพราะไม่ได้เพิ่มสิทธิ์อะไรเอง (ไมค์ยังต้องผ่าน TCC) **[ยังไม่ยืนยัน, ให้ spike #13 ทดสอบทั้งแบบมีและไม่มี]**
- **ห้ามเปิด** `com.apple.security.app-sandbox` โดยไม่จำเป็น ตัวอย่างในเอกสาร Tauri ใส่ไว้เป็นตัวอย่างเท่านั้น ([Tauri: Entitlements](https://v2.tauri.app/distribute/macos-application-bundle/#entitlements))
- Hardened runtime เปิด library validation ทำให้โหลดได้เฉพาะ library ของ Apple หรือที่มี Team ID เดียวกัน self-signed ไม่มี Team ID ถ้าวันหนึ่งต้องฝัง `.dylib`/`.framework` ของ third-party จะโหลดไม่ขึ้น ทางแก้คือเพิ่ม `com.apple.security.cs.disable-library-validation` หรือตั้ง `"hardenedRuntime": false` (hardened runtime จำเป็นเฉพาะตอน notarize ซึ่งเราไม่ทำ) ([Apple: Configuring the hardened runtime](https://developer.apple.com/documentation/xcode/configuring-the-hardened-runtime)) ตอนนี้ stack (Rust + swift-rs static lib + system frameworks) น่าจะไม่โดน **[ยังไม่ยืนยัน]**

## 4. ตรวจผลหลัง `tauri build`

```sh
APP="src-tauri/target/release/bundle/macos/Yokoten.app"
codesign -dvv "$APP" 2>&1 | grep -E 'Identifier=|Authority|TeamIdentifier|flags'
#   Authority=Yokoten Self-Signed, TeamIdentifier=not set, flags=0x10000(runtime)
codesign -dr - "$APP"
#   designated => identifier "<bundle id>" and certificate leaf = H"<sha1>"
codesign --verify --strict -vv "$APP"
#   valid on disk / satisfies its Designated Requirement
codesign -d --entitlements - --xml "$APP"     # ดู entitlements ที่ฝังจริง
plutil -p "$APP/Contents/Info.plist" | grep UsageDescription

# ยืนยันว่า build ใหม่ยังเป็น "แอปเดิม" ในสายตา TCC:
codesign -dr old.req "$OLD_APP" && sed -i '' 's/designated => //' old.req
codesign --verify -vv -R old.req "$APP"       # ต้องได้ explicit requirement satisfied
```

ทุกคำสั่งข้างบนทดลองกับแอปทดสอบแล้ว **[ทดลองแล้ว]** รูปแบบคำสั่ง `-R` มาจาก [TN3127](https://developer.apple.com/documentation/technotes/tn3127-inside-code-signing-requirements#Mutually-compatible-designated-requirements)

`spctl -a -vv -t exec "$APP"` จะตอบ `rejected` + `origin=Yokoten Self-Signed` เสมอ **[ทดลองแล้ว]** เป็นเรื่องปกติเพราะ Gatekeeper ยอมรับเฉพาะ App Store หรือ Developer ID ที่ notarize แล้ว ([Apple Platform Security: Gatekeeper](https://support.apple.com/guide/security/gatekeeper-and-runtime-protection-sec5599b66df/web)) ไม่ได้แปลว่าเปิดแอปไม่ได้ (ดูข้อ 6)

ถ้าต้องล้างสิทธิ์เพื่อทดสอบ prompt ใหม่: `tccutil reset ScreenCapture <bundle id>` (ชื่อ service `AudioCapture` สำหรับเสียงอนุมานจาก `kTCCServiceAudioCapture` **[ยังไม่ยืนยัน]**) ระวังอย่าลืมใส่ bundle id ไม่อย่างนั้นจะล้างของทุกแอป

## 5. `tauri dev`: สิทธิ์ผูกกับอะไร

ข้อเท็จจริงจาก source:

- `tauri dev` เรียก `cargo run` (ส่วน `tauri build` เรียก `cargo build`) แล้วรัน binary ใน `target/debug/` ตรง ๆ ไม่สร้าง .app และไม่เรียก `codesign` ([desktop.rs](https://github.com/tauri-apps/tauri/blob/2fd85731ceb871bfd20598b781dc711c33b5062a/crates/tauri-cli/src/interface/rust/desktop.rs#L239-L242)) binary จึงมีแค่ linker ad-hoc signature ที่ DR เป็น `cdhash` เปลี่ยนทุกครั้งที่ compile **[ทดลองแล้ว กับ binary ที่ `cc` link]**
- ตอน dev, Tauri ฝัง `src-tauri/Info.plist` (merge กับ `CFBundleName`/version) ลงใน binary ผ่าน `embed_plist` ทำให้ usage description มีผลกับ binary เปล่าได้ แต่ฝังเฉพาะไฟล์ `src-tauri/Info.plist` เท่านั้น ไม่อ่าน `bundle.macOS.infoPlist` และไม่ใส่ `CFBundleIdentifier` ([context.rs](https://github.com/tauri-apps/tauri/blob/2fd85731ceb871bfd20598b781dc711c33b5062a/crates/tauri-codegen/src/context.rs#L283-L330)) **ข้อนี้เป็นเหตุผลให้ใช้ชื่อไฟล์ `src-tauri/Info.plist` ไม่ใช่ path อื่น**

TCC ใช้ "responsible code" ในการตัดสินว่าใครขอสิทธิ์: Quinn (Apple DTS) อธิบายว่า "The MAC privilege mechanism is heavily dependent on the concept of responsible code" ([On File System Permissions](https://developer.apple.com/forums/thread/678819)) และ "The exact algorithm … is not documented, has changed in the past, and may well change in the future" ([thread 756510](https://developer.apple.com/forums/thread/756510)) ในเคสที่ใกล้เรามาก command-line tool ที่ใช้ screen capture รันด้วย `./tool` จาก Terminal แล้วไม่ขึ้น prompt (`CGRequestScreenCaptureAccess()` คืน false) แต่รันด้วย `open -n tool` แล้วขึ้น prompt ([thread 766455](https://developer.apple.com/forums/thread/766455))

ผลต่อ Yokoten:

1. **Screen Recording ตอน dev:** โปรเซสที่ `tauri dev` เปิดมี terminal (Terminal.app, Ghostty, Zed, Warp หรือ IDE ที่รันคำสั่ง) เป็น responsible code สิทธิ์จึงต้องให้กับแอป terminal นั้น ให้ครั้งเดียวแล้วอยู่ข้าม rebuild เพราะ signature ของ terminal ไม่เปลี่ยน ข้อเสียคือทุกโปรแกรมที่รันจาก terminal นั้นได้สิทธิ์อัดหน้าจอไปด้วย **[ตรรกะจากแหล่งข้างบน ยังไม่ได้ทดสอบกับ Tauri จริง]**
2. **System audio (Core Audio tap) ตอน dev:** ไม่มี terminal ตัวไหนในเครื่องนี้มี `NSAudioCaptureUsageDescription` ใน Info.plist (Terminal.app ไม่มีคีย์ usage เลย, Ghostty/Warp/Zed มีแค่ `NSMicrophoneUsageDescription`) **[ตรวจแล้วด้วย `plutil`]** ถ้า TCC ใช้ Info.plist ของ responsible app ตัว tap อาจไม่ขึ้น prompt หรือได้เสียงเงียบ **[ยังไม่ยืนยัน]** อย่าเชื่อผลของ audio tap จาก `tauri dev` จนกว่าจะทดสอบ
3. **ทางที่แนะนำสำหรับงานที่แตะ TCC:** build เป็น .app ที่เซ็นแล้วด้วย `tauri build --debug --bundles app` (ได้ debug build ใน `target/debug/bundle/macos/`) แล้วเปิดด้วย `open` ได้ DR เดียวกับ release ถ้า `identifier` เหมือนกัน สิทธิ์จึงใช้ร่วมกันได้ทั้ง debug และ release (สอดคล้องกับหลัก DR ใน TN3127) ส่วน UI/logic ที่ไม่แตะ TCC ใช้ `tauri dev` ตามปกติ
4. **ทางเลือกถ้าอยากให้ `tauri dev` มีตัวตนคงที่:** เพราะ `tauri dev` ใช้ `cargo run` จึงตั้ง `target.aarch64-apple-darwin.runner` ใน `.cargo/config.toml` ให้ชี้สคริปต์ที่ `codesign --force -s "Yokoten Self-Signed" -i <bundle id>.dev --options runtime "$1"` ก่อน `exec "$@"` ได้ ([Cargo: `target.<triple>.runner`](https://doc.rust-lang.org/cargo/reference/config.html#targettuplerunner)) การเซ็น binary เปล่าด้วย `-i` ให้ DR คงที่จริง (`identifier "….dev" and certificate leaf = H"…"` ผ่านข้าม build) **[ทดลองแล้ว]** แต่ยังโดนข้อ 1: ถ้ารันจาก terminal ตัว responsible code ยังเป็น terminal ต้องเปิดผ่าน LaunchServices (`open`) ซึ่งทำให้ stdout และการ kill/restart ของ `tauri dev` ใช้ไม่ได้ **ไม่แนะนำสำหรับตอนนี้** ใช้ข้อ 3 ง่ายกว่า

## 6. Gatekeeper กับแอปที่เซ็นเองบนเครื่องเดียว

- Gatekeeper ตรวจ "When a user downloads and opens an app … from outside the App Store" และขออนุมัติ "before opening downloaded software for the first time" ([Apple Platform Security: Gatekeeper](https://support.apple.com/guide/security/gatekeeper-and-runtime-protection-sec5599b66df/web)) แอปที่ build บนเครื่องเองไม่ได้ดาวน์โหลด จึงไม่มี `com.apple.quarantine` แอปทดสอบที่เซ็นด้วย self-signed มีแค่ xattr `com.apple.provenance` และ `open` เปิดได้ (exit 0) **[ทดลองแล้ว แต่แอปทดสอบไม่มี GUI จึงไม่ได้ดูว่ามี dialog หรือไม่]**
- แหล่งเดียวกันระบุว่า macOS ตรวจ known malware (XProtect) ตอนเปิดครั้งแรก "regardless of how it arrived on the Mac" ซึ่งไม่ใช่การขวางแอปที่ไม่ notarize
- ถ้าแอปติด quarantine (เช่น ส่งผ่าน AirDrop หรือดาวน์โหลดจาก GitHub Releases) จะโดน alert ทำนอง "Apple cannot check … for malicious software" ต้องไป System Settings > Privacy & Security > **Open Anyway** ([Apple: Safely open apps on your Mac](https://support.apple.com/en-us/102445)) หรือลบ quarantine เองด้วย `xattr -d com.apple.quarantine Yokoten.app`
- `syspolicy_check distribution` ตอบ `Notary Ticket Missing` (Fatal) **[ทดลองแล้ว]** ไม่ต้องสนใจเพราะเราไม่แจกจ่าย

## 7. ข้อควรรู้อื่น

- **Prompt ซ้ำรายเดือนของ Screen Recording:** ตั้งแต่ macOS 15 Sequoia ระบบถามยืนยันสิทธิ์ Screen Recording ซ้ำเป็นระยะ (เปลี่ยนจากรายสัปดาห์เป็นรายเดือนช่วง beta) ([9to5Mac](https://9to5mac.com/2024/08/14/macos-sequoia-screen-recording-prompt-monthly/), [TidBITS](https://tidbits.com/2024/08/19/apple-reduces-excessive-sequoia-permission-requests-shifts-to-monthly)) การเซ็นคงที่ไม่ช่วยเรื่องนี้ entitlement `com.apple.developer.persistent-content-capture` ที่อาจยกเว้นได้ต้องยื่นขอกับ Apple และใช้กับ VNC app ([Apple](https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.developer.persistent-content-capture), [Michael Tsai](https://mjtsai.com/blog/2024/08/08/sequoia-screen-recording-prompts-and-the-persistent-content-capture-entitlement/)) ซึ่งเราไม่มีสิทธิ์ เพราะไม่มี paid program **[พฤติกรรมบน macOS 26/27 ยังไม่ยืนยัน, แหล่งเป็นข่าวไม่ใช่ Apple]**
- **อย่าใช้ `"signingIdentity": "-"`** สำหรับ build ที่ใช้งานจริง เพราะเป็น ad-hoc DR ผูก cdhash (ข้อ 1)
- **อย่า commit** `.p12`, private key หรือรหัสผ่าน ถ้าทำ CI ภายหลัง ให้ใช้ `APPLE_CERTIFICATE` (base64 ของ `.p12`) + `APPLE_CERTIFICATE_PASSWORD` เป็น secret ([Tauri: macOS Code Signing](https://v2.tauri.app/distribute/sign/macos/)) และต้องใช้ `.p12` ไฟล์เดิม ไม่อย่างนั้น DR เปลี่ยน
- **ย้ายแอปหลังให้สิทธิ์:** DR ไม่มี path จึงย้าย `.app` ไป `/Applications` ได้ (สอดคล้องกับ TN3127 ที่ DR มีแค่ identifier + signer) แต่ TCC ใช้ path ประกอบในบางกรณีหรือไม่ **[ยังไม่ยืนยัน]** แนะนำติดตั้งที่ `/Applications/Yokoten.app` ตำแหน่งเดียวเสมอ

## สูตรย่อ (checklist)

1. Keychain Access > Certificate Assistant > Create a Certificate: `Yokoten Self-Signed`, Self Signed Root, Code Signing, override defaults แล้วตั้งอายุยาว ลง login keychain แล้ว export `.p12` เก็บไว้
2. ตรวจ: `security find-identity -p codesigning` ต้องเห็นชื่อ cert (ขึ้น `CSSMERR_TP_NOT_TRUSTED` ได้ ไม่เป็นไร)
3. `tauri.conf.json`: ตรึง `identifier`, ตั้ง `bundle.macOS.signingIdentity = "Yokoten Self-Signed"`, `entitlements = "./Entitlements.plist"`
4. `src-tauri/Info.plist`: `NSAudioCaptureUsageDescription` (+ `NSScreenCaptureUsageDescription`)
5. `src-tauri/Entitlements.plist`: `com.apple.security.device.audio-input` (precaution) ไม่เปิด sandbox
6. `tauri build` (หรือ `tauri build --debug --bundles app` ตอนพัฒนาส่วนที่แตะ TCC) แล้วตรวจด้วย `codesign -dr -` ต้องเห็น `certificate leaf = H"…"` ไม่ใช่ `cdhash`
7. ติดตั้งที่ `/Applications` เปิดด้วย `open` ให้สิทธิ์ครั้งแรก build ครั้งต่อไปต้องไม่ถามซ้ำ (ยืนยันใน spike)
