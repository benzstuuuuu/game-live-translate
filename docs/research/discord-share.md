# Research: ช่องทางส่งภาพและข้อความเข้า Discord จากแอป Mac

- Ticket: [#20](https://github.com/benzstuuuuu/game-live-translate/issues/20) (อ้างอิง FR-CAP-07, FR-CAP-08, NFR-05) ส่งต่อให้ ticket ออกแบบ [#21](https://github.com/benzstuuuuu/game-live-translate/issues/21)
- วันที่ค้นคว้า: 2026-11-18
- แหล่งข้อมูล: Discord Developer Docs, Discord Support, Apple Developer Docs และ docs.rs ของ crate ที่เกี่ยวข้อง รายการเต็มอยู่ที่ [Sources](#sources) ท้ายไฟล์ ทุกแหล่งอ่านเมื่อ 2026-11-18
- หมายเหตุวิธีอ่าน: `discord.com/developers/docs/*` ตอนนี้ redirect (HTTP 301) ไปที่ `docs.discord.com/developers/*` จึงอ้างลิงก์ปลายทางใหม่ และอ่านเวอร์ชัน Markdown (`.md`) ของแต่ละหน้า ส่วนหน้า `support.discord.com` ตอบ HTTP 403 กับการดึงอัตโนมัติ ข้อมูลจากหน้าเหล่านั้นจึงมาจากข้อความที่ search engine แสดง และติดป้าย **[UNVERIFIED]** ไว้
- ข้อความที่ติด **[UNVERIFIED]** คือข้อที่เอกสารไม่ได้ระบุตรง ๆ หรือเป็นการอนุมานของเรา ต้องลองจริงก่อนใช้ตัดสินใจ

## สรุปสั้น

- **Webhook ของ channel ทำได้ครบ:** ส่ง `content` (สูงสุด 2000 ตัวอักษร) พร้อมไฟล์รูปใน request เดียวด้วย `multipart/form-data` (`files[n]` + `payload_json`) ไม่ต้องมี bot และไม่ต้อง auth อื่น เพราะ token อยู่ใน URL [[S1]](https://docs.discord.com/developers/resources/webhook#execute-webhook) [[S2]](https://docs.discord.com/developers/reference#uploading-files)
- **ขนาดไฟล์สูงสุดต่อไฟล์ = 20 MiB** เป็นค่า default "for all users" และอาจสูงขึ้นตาม Nitro หรือ Boost Tier ของ server [[S2]](https://docs.discord.com/developers/reference#uploading-files) ภาพ 1080p แม้ไม่บีบอัดเลย (RGB ≈ 5.9 MiB) ก็ยังไม่ถึง limit
- **Rate limit ไม่มีตัวเลขตายตัวในเอกสาร** ให้อ่านจาก header `X-RateLimit-*` และเคารพ `retry_after` เมื่อได้ 429 การใช้งานแบบกดส่งเองทีละครั้งจึงไม่น่าชน limit [[S3]](https://docs.discord.com/developers/topics/rate-limits)
- **Bot token** ทำได้เหมือนกันแต่ setup หนักกว่า (สร้าง application, เชิญ bot เข้า server, ให้สิทธิ์ `SEND_MESSAGES`/`ATTACH_FILES`) และ token หลุดแล้วเสียหายกว้างกว่า ข้อได้เปรียบคือเลือก channel ตอนส่งได้ [[S4]](https://docs.discord.com/developers/resources/message#create-message) [[S5]](https://docs.discord.com/developers/topics/permissions)
- **Clipboard ทำได้ และเป็นทางเดียวที่ภาพไม่ออกจากเครื่องโดยแอปเรา** (ผู้ใช้วางเองใน Discord) ไม่ต้องแก้ NFR-05 แต่ผู้ใช้ต้องเปิด Discord (แอปหรือเว็บ) และกดวางเอง [[S16]](https://developer.apple.com/documentation/appkit/nspasteboard/writeobjects(_:)) [[S17]](https://v2.tauri.app/plugin/clipboard/)
- **macOS share sheet ส่งเข้า Discord ไม่ได้** เพราะแอป Discord บน Mac ไม่มี Share Extension (ตรวจจากเครื่องจริง ดูข้อ 3.2)
- **Discord RPC และ Social SDK ไม่มีทางส่งภาพเข้า channel** RPC ไม่มีคำสั่งส่งข้อความ ส่วน Social SDK ส่งได้แค่ข้อความใน DM/lobby และ linked channel ยังเป็น limited access [[S7]](https://docs.discord.com/developers/topics/rpc) [[S8]](https://docs.discord.com/developers/discord-social-sdk/core-concepts/communication-features)
- **วาดคำแปลไทยลงภาพฝั่ง Rust ต้องใช้ตัว shaping** เช่น `cosmic-text` (shaping ด้วย harfrust) ส่วน `imageproc::draw_text_mut` ที่ใช้ `ab_glyph` ไม่มี shaping สระบนและวรรณยุกต์ไทยอาจวางผิด **[UNVERIFIED]** [[S21]](https://docs.rs/imageproc/latest/imageproc/drawing/fn.draw_text_mut.html) [[S22]](https://docs.rs/cosmic-text/latest/cosmic_text/)

## 1. Webhook ของ channel

### 1.1 ส่งรูปพร้อมข้อความใน request เดียว

- Endpoint: `POST /webhooks/{webhook.id}/{webhook.token}` ("Execute Webhook") คืน message object หรือ `204 No Content` ตามค่า query `wait` [[S1]](https://docs.discord.com/developers/resources/webhook#execute-webhook)
- ต้องมีอย่างน้อยหนึ่งใน `content`, `embeds`, `components`, `file` หรือ `poll` [[S1]](https://docs.discord.com/developers/resources/webhook#execute-webhook)
- `content` ยาวได้ "up to 2000 characters" ส่วน `embeds` ใส่ได้ "up to 10" [[S1]](https://docs.discord.com/developers/resources/webhook#execute-webhook) เอกสารไม่ได้บอกว่านับตัวอักษรเป็น code point หรือ UTF-16 ข้อความไทยที่มีสระบน/ล่างและวรรณยุกต์อาจนับมากกว่าที่ตาเห็น **[UNVERIFIED]**
- ควรส่ง `?wait=true` เพราะถ้า `wait=false` (default) "a message that is not saved does not return an error" [[S1]](https://docs.discord.com/developers/resources/webhook#execute-webhook) แอปจะไม่รู้ว่าส่งไม่สำเร็จ
- ตั้ง `username` และ `avatar_url` ทับค่า default ของ webhook ได้ต่อข้อความ [[S1]](https://docs.discord.com/developers/resources/webhook#execute-webhook)
- Discord แนะนำให้ใช้ `allowed_mentions` เมื่อใส่ข้อความที่ไม่ได้ควบคุมเอง เพื่อกัน mention ที่ไม่ตั้งใจ [[S1]](https://docs.discord.com/developers/resources/webhook#execute-webhook) คำแปลจาก LLM อาจมี `@everyone` หลุดมาได้ จึงควรตั้ง `{"parse": []}`
- ถ้า channel ปลายทางเป็น forum/media channel ต้องใส่ `thread_id` หรือ `thread_name` [[S1]](https://docs.discord.com/developers/resources/webhook#execute-webhook)

### 1.2 รูปแบบ multipart

- endpoint ที่รับไฟล์ใช้ body แบบ `multipart/form-data` ไฟล์ใส่ใน field `files[n]` แต่ละ part ต้องมี `Content-Disposition` ที่มี `filename` และ `name` ไม่ซ้ำกัน ส่วนพารามิเตอร์อื่นส่งแยกเป็น form field หรือรวมเป็น JSON ใน field `payload_json` ก็ได้ [[S2]](https://docs.discord.com/developers/reference#uploading-files)
- เลข `n` ใน `files[n]` คือ placeholder id ที่อ้างใน `attachments` (เช่นใส่ `description` เป็น alt text) [[S2]](https://docs.discord.com/developers/reference#uploading-files) [[S4]](https://docs.discord.com/developers/resources/message#attachment-object-attachment-request-structure)
- เอกสาร Create Message ไม่ได้ระบุจำนวนไฟล์สูงสุดต่อข้อความไว้ตรง ๆ **[UNVERIFIED]** สำหรับงานนี้ใช้แค่ 1 ไฟล์ จึงไม่กระทบ

### 1.3 ขนาดไฟล์สูงสุด

- "The file upload size limit applies to each file in a request. The default limit is `20 MiB` for all users, but may be higher for users depending on their Nitro status or by the server's Boost Tier." [[S2]](https://docs.discord.com/developers/reference#uploading-files)
- webhook ไม่มีบัญชีผู้ใช้ จึงไม่ได้ limit ของ Nitro ส่วน Boost Tier ของ server จะยกให้ webhook ด้วยหรือไม่ เอกสารไม่ได้บอก **[UNVERIFIED]** ให้ถือ 20 MiB เป็นเพดาน
- Support article "File Attachments FAQ" ระบุว่า free limit ขึ้นเป็น 20MB เมื่อ ส.ค. 2026 [[S11]](https://support.discord.com/hc/en-us/articles/25444343291031-File-Attachments-FAQ) **[UNVERIFIED: อ่านผ่าน search snippet]** หน้า support เก่าบางหน้ายังเขียน 10MB ให้ยึดตัวเลขใน developer docs

### 1.4 Rate limit

- Discord บอกตรง ๆ ว่า "rate limits should not be hard coded into your app" ให้อ่าน header `X-RateLimit-Limit`, `-Remaining`, `-Reset`, `-Reset-After`, `-Bucket` แทน [[S3]](https://docs.discord.com/developers/topics/rate-limits)
- per-route limit คิดแยกตาม top-level resource ซึ่งรวม "webhooks (`webhook_id` or `webhook_id + webhook_token`)" [[S3]](https://docs.discord.com/developers/topics/rate-limits) แต่ละ webhook จึงมี bucket ของตัวเอง
- global limit 50 requests/วินาที ถ้าไม่มี authorization header จะคิดต่อ IP [[S3]](https://docs.discord.com/developers/topics/rate-limits) (webhook ไม่มี header นี้)
- เมื่อได้ 429 ให้รอตาม `Retry-After` header หรือ field `retry_after` [[S3]](https://docs.discord.com/developers/topics/rate-limits)
- ถ้า request ที่ได้ 401/403/429 เกิน **10,000 ครั้งต่อ 10 นาที** IP จะโดนแบนชั่วคราว และ "If a webhook returns a **404** status you should not attempt to use it again" [[S3]](https://docs.discord.com/developers/topics/rate-limits) แอปจึงควรหยุดใช้ webhook ที่ได้ 404 แล้วให้ผู้ใช้ใส่ URL ใหม่
- ตัวเลข per-webhook ที่พบเห็นกันทั่วไป (ราว 5 request ต่อ 2 วินาที) ไม่มีอยู่ในเอกสาร **[UNVERIFIED]** และไม่สำคัญสำหรับการกดส่งเอง

### 1.5 Embed กับ attachment

| | Attachment + `content` | Embed ที่อ้างรูปแนบ |
| :--- | :--- | :--- |
| วิธีทำ | ส่ง `files[0]` และใส่คำแปลใน `content` | ส่ง `files[0]` และใส่ embed ที่ `image.url = "attachment://<filename>"` [[S2]](https://docs.discord.com/developers/reference#uploading-files) |
| ความยาวข้อความ | `content` ≤ 2000 ตัวอักษร [[S1]](https://docs.discord.com/developers/resources/webhook#execute-webhook) | `title` 256, `description` 4096, `fields` ≤ 25 (name 256 / value 1024), `footer.text` 2048 และรวมทุก embed ไม่เกิน 6000 ตัวอักษร เกินแล้วได้ `Bad Request` [[S4]](https://docs.discord.com/developers/resources/message#embed-object-embed-limits) |
| ข้อจำกัดเฉพาะ webhook | – | ตั้งได้ทุก field ยกเว้น `type` (บังคับเป็น `rich`), `provider`, `video` และ `height`/`width`/`proxy_url` ของรูป [[S1]](https://docs.discord.com/developers/resources/webhook#execute-webhook) |
| ผลต่อ message object | ไฟล์อยู่ใน `attachments` | `attachments` เก็บเฉพาะ "attached files that are not referenced in embeds" [[S4]](https://docs.discord.com/developers/resources/message#message-object) |
| ความซับซ้อน | น้อย | มากกว่า แต่จัดหน้าได้ (เช่น แยกต้นฉบับกับคำแปลเป็น field) |

- ถ้าตั้ง flag `IS_COMPONENTS_V2` จะส่ง `content`, `embeds`, `files[n]` ไม่ได้ [[S1]](https://docs.discord.com/developers/resources/webhook#execute-webhook) ไม่ควรใช้ flag นี้

### 1.6 การเก็บ webhook URL

- webhook "do not require a bot user or authentication to use" [[S1]](https://docs.discord.com/developers/resources/webhook) ใครถือ URL ก็โพสต์ได้ และยังเรียก "Modify Webhook with Token" และ "Delete Webhook with Token" ได้โดย "does not require authentication" [[S1]](https://docs.discord.com/developers/resources/webhook#modify-webhook-with-token) URL ทั้งเส้นจึงเป็น secret
- จึงควรเก็บทั้ง URL ใน macOS Keychain เหมือน API key อื่น ไม่ log ทั้ง URL (redact ส่วน token) และไม่ส่งเข้า webview เกินจำเป็น **[UNVERIFIED: เป็นข้อเสนอของเรา ไม่ใช่ข้อความในเอกสาร]**
- การสร้าง webhook ต้องมีสิทธิ์ `MANAGE_WEBHOOKS` ใน server นั้น [[S1]](https://docs.discord.com/developers/resources/webhook#create-webhook) ถ้าผู้ใช้ไม่ได้เป็นแอดมินของ server เพื่อน ต้องขอให้แอดมินสร้าง URL ให้
- ผู้ใช้สร้างและคัดลอก URL ได้จาก UI ของ Discord (Server Settings > Integrations) และลบ webhook ที่หลุดได้จากหน้าเดียวกัน [[S12]](https://support.discord.com/hc/en-us/articles/228383668-Intro-to-Webhooks) **[UNVERIFIED: อ่านผ่าน search snippet]**
- อีกทางคือ OAuth2 scope `webhook.incoming` ซึ่ง "generates a webhook that is returned in the oauth token response" [[S6]](https://docs.discord.com/developers/topics/oauth2) แต่ endpoint OAuth2 ทุกตัวต้องใช้ `client_secret` [[S6]](https://docs.discord.com/developers/topics/oauth2) แอป desktop ที่ไม่มี backend จะต้องฝัง secret ไว้ในแอป จึงไม่เหมาะ (อนุมานของเรา)

### 1.7 ตัวอย่าง Rust แบบสั้น (reqwest multipart)

```rust
use reqwest::multipart::{Form, Part};

// webhook_url อ่านจาก Keychain; jpeg_bytes มาจาก encoder ฝั่ง Rust
let payload = serde_json::json!({
    "content": thai_text,                       // ≤ 2000 ตัวอักษร
    "allowed_mentions": { "parse": [] },        // กัน @everyone จากคำแปล
    "attachments": [{ "id": 0, "filename": "yokoten.jpg", "description": source_text }]
});
let form = Form::new()
    .part("payload_json", Part::text(payload.to_string()).mime_str("application/json")?)
    .part("files[0]", Part::bytes(jpeg_bytes).file_name("yokoten.jpg").mime_str("image/jpeg")?);
let res = client
    .post(format!("{webhook_url}?wait=true"))
    .multipart(form)
    .send()
    .await?;
// 429 → รอ retry_after แล้วค่อยให้ผู้ใช้ลองใหม่; 404 → webhook ถูกลบ หยุดใช้ URL นี้
```

- `reqwest::multipart::Part` มี `bytes`, `text`, `file_name`, `mime_str` [[S20]](https://docs.rs/reqwest/latest/reqwest/multipart/struct.Part.html) โมดูล `multipart` ต้องเปิด cargo feature `multipart` **[UNVERIFIED สำหรับ reqwest 0.13]**
- รูปแบบ part ของ `payload_json` ตรงกับตัวอย่างในเอกสาร Discord ที่ใส่ `Content-Type: application/json` [[S2]](https://docs.discord.com/developers/reference#uploading-files)

## 2. Bot token เทียบกับ webhook (ผู้ใช้คนเดียว)

| | Webhook | Bot token |
| :--- | :--- | :--- |
| Setup ฝั่งผู้ใช้ | คัดลอก URL ของ channel มาใส่แอป ต้องมี `MANAGE_WEBHOOKS` หรือให้แอดมินสร้างให้ [[S1]](https://docs.discord.com/developers/resources/webhook#create-webhook) | สร้าง application + bot ใน Developer Portal, เชิญ bot เข้า server, ให้สิทธิ์ แล้วนำ token มาใส่แอป [[S2]](https://docs.discord.com/developers/reference#authentication) การเชิญ bot ต้องมีสิทธิ์ Manage Server **[UNVERIFIED]** |
| Endpoint | `POST /webhooks/{id}/{token}` [[S1]](https://docs.discord.com/developers/resources/webhook#execute-webhook) | `POST /channels/{channel.id}/messages` พร้อม header `Authorization: Bot <token>` [[S4]](https://docs.discord.com/developers/resources/message#create-message) [[S2]](https://docs.discord.com/developers/reference#authentication) |
| สิทธิ์ที่ต้องมี | ไม่มี (token อยู่ใน URL) | `SEND_MESSAGES` ใน guild channel [[S4]](https://docs.discord.com/developers/resources/message#create-message) และการปฏิเสธ `SEND_MESSAGES` ทำให้ `ATTACH_FILES` ใช้ไม่ได้ด้วย [[S5]](https://docs.discord.com/developers/topics/permissions) |
| ปลายทาง | ล็อกไว้ที่ channel เดียวตอนสร้าง | ทุก channel ที่ bot มีสิทธิ์ เลือกตอนส่งได้ |
| ขนาดไฟล์ / multipart | 20 MiB, `files[n]` เหมือนกัน [[S2]](https://docs.discord.com/developers/reference#uploading-files) | เหมือนกัน [[S4]](https://docs.discord.com/developers/resources/message#create-message) |
| Rate limit | bucket ต่อ webhook, global คิดต่อ IP [[S3]](https://docs.discord.com/developers/topics/rate-limits) | bucket ต่อ channel, global 50 req/s ต่อ bot [[S3]](https://docs.discord.com/developers/topics/rate-limits) |
| ถ้า secret หลุด | คนอื่นโพสต์/แก้/ลบ webhook นั้นได้ (channel เดียว) | คนอื่นทำทุกอย่างที่ bot มีสิทธิ์ได้ ในทุก server ที่ bot อยู่ |
| ชื่อผู้ส่ง | ตั้ง `username`/`avatar_url` ต่อข้อความได้ [[S1]](https://docs.discord.com/developers/resources/webhook#execute-webhook) | ชื่อและรูปของ bot |

- ทั้งสองทางโพสต์ในนาม webhook/bot ไม่ใช่บัญชีผู้ใช้ การส่งในนามบัญชีผู้ใช้ผ่าน API (self-bot) ผิดนโยบายของ Discord และอาจโดนปิดบัญชี [[S13]](https://support.discord.com/hc/en-us/articles/115002192352-Automated-User-Accounts-Self-Bots) **[UNVERIFIED: อ่านผ่าน search snippet]** จึงไม่ใช่ตัวเลือก
- สำหรับผู้ใช้คนเดียวที่ส่งเข้า channel ประจำ bot เพิ่มงาน setup และความเสี่ยงโดยไม่ได้ประโยชน์เพิ่ม นอกจากต้องการเลือก channel หลายที่ตอนส่ง

## 3. ทางที่ไม่ส่งผ่าน network จากแอปเรา

### 3.1 คัดลอกรูปลง clipboard แล้วผู้ใช้วางใน Discord เอง

- AppKit: `NSPasteboard.writeObjects(_:)` เขียน array ของ object ลง pasteboard [[S16]](https://developer.apple.com/documentation/appkit/nspasteboard/writeobjects(_:)) และ `NSImage` รองรับรูปแบบมาตรฐานรวม TIFF, JPEG, PNG [[S14b]](https://developer.apple.com/documentation/appkit/nsimage)
- Tauri: plugin `clipboard-manager` มีคำสั่ง `write_image` (permission `clipboard-manager:allow-write-image`) [[S17]](https://v2.tauri.app/plugin/clipboard/) ฝั่ง Rust คือ `Clipboard::write_image` ที่ "Writes an image to the system clipboard as RGBA data" [[S18]](https://docs.rs/tauri-plugin-clipboard-manager/latest/tauri_plugin_clipboard_manager/struct.Clipboard.html) หรือใช้ crate `arboard` (`set_image`) [[S19]](https://docs.rs/arboard/latest/arboard/struct.Clipboard.html)
- ผู้ใช้ต้องเปิด Discord (แอป desktop หรือเว็บ) แล้วกด Cmd+V ในช่องแชท การวางรูปจาก clipboard แล้ว Discord อัปโหลดให้เป็นพฤติกรรมของ client Discord ที่เราไม่พบเอกสารทางการระบุชัด **[UNVERIFIED]**
- clipboard ถือรูปและข้อความพร้อมกันได้ แต่ Discord จะรับทั้งสองอย่างจากการวางครั้งเดียวหรือไม่ยังไม่ทราบ **[UNVERIFIED]** ทางที่แน่นอนกว่าคือวาดคำแปลลงภาพ (ข้อ 5) หรือให้ผู้ใช้วางสองครั้ง
- ข้อดีหลัก: ภาพไม่ถูกแอปเราส่งออกนอกเครื่อง ผู้ใช้เป็นคนส่งเองในนามตัวเอง จึงไม่ต้องแก้ NFR-05 และไม่มี secret ใหม่ให้เก็บ

### 3.2 macOS share sheet (NSSharingService / NSSharingServicePicker)

- `NSSharingService` แชร์ text, image, URL หรือไฟล์ไปยังบริการต่าง ๆ เช่น Mail, Messages, AirDrop [[S14]](https://developer.apple.com/documentation/appkit/nssharingservice) ส่วน `NSSharingServicePicker` แสดงรายการบริการให้ผู้ใช้เลือก (macOS 13 ขึ้นไปเป็น popover พร้อม preview) [[S15]](https://developer.apple.com/documentation/appkit/nssharingservicepicker)
- แอปที่จะโผล่ใน share sheet ต้องมี Share Extension ตรวจบนเครื่องจริง (macOS 26.5.1, Discord.app เวอร์ชัน 0.0.423): ไม่มีโฟลเดอร์ `Contents/PlugIns`, ไม่มี `NSServices` ใน `Info.plist` และ `pluginkit -m -p com.apple.share-services` ไม่พบรายการของ Discord **(ตรวจเองบนเครื่อง ไม่มีเอกสารทางการยืนยัน)** สรุปว่าส่งเข้า Discord ผ่าน share sheet ไม่ได้ ได้แค่ Messages/AirDrop/Mail
- Tauri 2 ไม่มี plugin share sheet สำหรับ desktop ที่เราพบ ต้องเรียก AppKit เองผ่าน `objc2-app-kit` **[UNVERIFIED]**

## 4. Discord RPC และ Social SDK

- **RPC (IPC):** Discord แนะนำให้โปรเจกต์ใหม่ใช้ Social SDK แทน และ "We currently do not allow access to RPC for unapproved apps without being on the game's list of testers" (สูงสุด 50 คน) [[S7]](https://docs.discord.com/developers/topics/rpc) คำสั่งที่มีคือ `AUTHORIZE`, `AUTHENTICATE`, `GET_GUILDS`, `GET_GUILD`, `GET_CHANNEL(S)`, voice settings, `SELECT_VOICE_CHANNEL`, `SELECT_TEXT_CHANNEL`, `SUBSCRIBE`, `SET_ACTIVITY`, `SEND_ACTIVITY_JOIN_INVITE` ฯลฯ **ไม่มีคำสั่งส่งข้อความหรืออัปโหลดไฟล์** [[S7]](https://docs.discord.com/developers/topics/rpc)
- **Social SDK:** ส่ง DM (`SendUserMessage` รับ string) และข้อความใน lobby ได้ มี rate limit ช่วงพัฒนา 100 ครั้งต่อ 2 ชั่วโมง [[S8]](https://docs.discord.com/developers/discord-social-sdk/core-concepts/communication-features) [[S9]](https://docs.discord.com/developers/discord-social-sdk/development-guides/sending-direct-messages) เอกสารพูดถึงรูปภาพแค่ฝั่งรับ ("Messages sent on Discord can contain content that may not be renderable in-game, such as images") ไม่มี API แนบรูปให้เห็น [[S9]](https://docs.discord.com/developers/discord-social-sdk/development-guides/sending-direct-messages) **[UNVERIFIED: ไม่พบ ไม่ได้แปลว่าไม่มี]**
- Linked Channels (เชื่อม lobby ในเกมกับ text channel ของ server) ยังเป็น "limited access" ต้องสมัคร [[S10]](https://docs.discord.com/developers/discord-social-sdk/development-guides/linked-channels)
- สรุป: ทั้งสองทางออกแบบมาให้เกมที่เราเป็นเจ้าของ ไม่ใช่ให้แอปภายนอกส่งภาพเข้า channel จึงไม่เหมาะกับงานนี้

## 5. รูปแบบรูปและการวาดคำแปลทับภาพ

### 5.1 ขนาดไฟล์ 1080p

| รูปแบบ | ขนาดโดยประมาณ (1920×1080) | ที่มา |
| :--- | :--- | :--- |
| raw RGBA (เช่นข้อมูลที่ส่งเข้า clipboard) | 8,294,400 bytes ≈ 7.9 MiB | คำนวณ 1920×1080×4 |
| raw RGB | 6,220,800 bytes ≈ 5.9 MiB | คำนวณ 1920×1080×3 |
| PNG ของภาพเกม | ราว 2–5 MB | **[UNVERIFIED: ประมาณการ ขึ้นกับความซับซ้อนของภาพ]** |
| JPEG quality ~85 | ราว 0.3–1 MB | **[UNVERIFIED: ประมาณการ]** |

- ทุกแบบต่ำกว่า 20 MiB [[S2]](https://docs.discord.com/developers/reference#uploading-files) ขนาดไฟล์จึงไม่ใช่ข้อจำกัดสำหรับภาพเดียว แม้จับที่ 1440p (2560×1440×3 ≈ 10.5 MiB แบบ raw) ก็ยังผ่าน
- CDN ของ Discord รองรับทั้ง JPEG และ PNG [[S2]](https://docs.discord.com/developers/reference#image-formatting) PNG คมกว่าสำหรับตัวหนังสือในเกม แต่ใหญ่และอัปโหลดนานกว่า ควรวัดขนาดจริงจากภาพที่ ScreenCaptureKit จับได้ใน prototype

### 5.2 การวาดคำแปลไทยลงภาพฝั่ง Rust

- `imageproc::drawing::draw_text_mut` พึ่ง `ab_glyph` [[S21]](https://docs.rs/imageproc/latest/imageproc/drawing/fn.draw_text_mut.html) ซึ่งเป็น "API for loading, scaling, positioning and rasterizing OpenType font glyphs" ไม่มี text shaping [[S23]](https://docs.rs/ab_glyph/latest/ab_glyph/) สระบน สระล่าง และวรรณยุกต์ไทยจึงอาจซ้อนหรือวางผิดตำแหน่ง **[UNVERIFIED: อนุมานจากเอกสาร ต้องลองจริง]**
- `cosmic-text` "provides abstractions for shaping, font discovery, font fallback, layout, rasterization" โดย "Shaping utilizes harfrust ... rasterization ... utilizes swash" [[S22]](https://docs.rs/cosmic-text/latest/cosmic_text/) ใช้ font ระบบผ่าน `fontdb` ได้ จึงเป็นตัวเลือกที่รองรับไทยได้ถูกต้องกว่า
- ทางเลือกอื่น: วาดด้วย CoreText/CoreGraphics ของ macOS ผ่าน objc2 หรือให้ webview วาดบน canvas แล้วส่ง PNG กลับมา Rust **[UNVERIFIED: ยังไม่ได้ลอง]**
- จาก [#6](https://github.com/benzstuuuuu/game-live-translate/issues/6) overlay ถูกตัดออกจากภาพด้วย `SCContentFilter` ภาพที่จับได้จึงไม่มีคำแปลติดมา ต้องวาดทับเองถ้าต้องการให้ภาพมีคำแปล
- อีกทางคือไม่วาดลงภาพเลย ส่งคำแปลเป็น `content` หรือ embed (ข้อ 1.5) ข้อความใน Discord ค้นหาและคัดลอกได้ และไม่ต้องจัดการ font

## 6. ผลต่อ SRS และ ticket #21

- **NFR-05 (ภาพไม่ออกนอกเครื่อง):** ทาง webhook และ bot ส่งภาพออกนอกเครื่องโดยแอปเรา ต้องเพิ่มข้อยกเว้น เช่น "ยกเว้นเมื่อผู้ใช้กดแชร์เองไปยังปลายทางที่ตั้งไว้" ส่วนทาง clipboard ไม่ต้องแก้ NFR-05
- **Secret ใหม่:** webhook URL (หรือ bot token) ต้องเก็บใน Keychain เหมือน API key อื่น
- **ข้อความที่ต้องตัด:** `content` ≤ 2000 ตัวอักษร หรือ embed `description` ≤ 4096 และรวม ≤ 6000 [[S1]](https://docs.discord.com/developers/resources/webhook#execute-webhook) [[S4]](https://docs.discord.com/developers/resources/message#embed-object-embed-limits) คำแปลทั้งจอ (FR-CAP-07) ที่ยาวมากต้องตัดหรือแบ่ง
- **Error ที่ต้องจัดการ:** 429 (รอ `retry_after`), 404 (webhook ถูกลบ ให้ผู้ใช้ใส่ URL ใหม่), 400 (ข้อความ/embed เกิน limit) [[S3]](https://docs.discord.com/developers/topics/rate-limits) [[S4]](https://docs.discord.com/developers/resources/message#embed-object-embed-limits)
- **สิ่งที่ต้องวัดใน prototype:** ขนาด PNG/JPEG จริง, การวาดไทยด้วย `cosmic-text`, และการวางรูปจาก clipboard ใน Discord desktop

## สรุป trade-off (ไม่ใช่การตัดสินใจ)

| ทาง | ส่งรูป+คำแปลอัตโนมัติ | ต้องแก้ NFR-05 | Setup ผู้ใช้ | ความเสี่ยงหลัก |
| :--- | :--- | :--- | :--- | :--- |
| Webhook | ได้ใน request เดียว | ต้อง | วาง URL ครั้งเดียว (ต้องมี `MANAGE_WEBHOOKS` หรือขอแอดมิน) | URL หลุด = คนอื่นโพสต์/ลบ webhook ได้ |
| Bot token | ได้ และเลือก channel ได้ | ต้อง | หนัก (application, เชิญ bot, สิทธิ์) | token หลุดกระทบทุก server ที่ bot อยู่ |
| Clipboard | ไม่ได้ ผู้ใช้วางเอง | ไม่ต้อง | ไม่มี | การวางรูป+ข้อความครั้งเดียวยังไม่ยืนยัน |
| Share sheet | ส่งเข้า Discord ไม่ได้ | – | – | Discord ไม่มี Share Extension |
| RPC / Social SDK | ไม่มี API ส่งรูปเข้า channel | – | – | ต้องขออนุมัติ และไม่ตรงกับ use case |

ทางที่เป็นไปได้จริงคือ webhook กับ clipboard ซึ่งแลกกันระหว่างการส่งอัตโนมัติ (webhook ต้องแก้ NFR-05 และเก็บ secret) กับความเป็นส่วนตัวและความง่าย (clipboard ไม่ต้องแก้อะไรแต่ผู้ใช้ต้องวางเอง) ทั้งสองทางใช้โค้ดวาดภาพ+คำแปลชุดเดียวกันได้

## Sources

- [S1] Discord Developer Docs — Webhook Resource (Execute Webhook, Create Webhook, Modify/Delete Webhook with Token): https://docs.discord.com/developers/resources/webhook
- [S2] Discord Developer Docs — API Reference (Authentication, Uploading Files, Image Formatting): https://docs.discord.com/developers/reference
- [S3] Discord Developer Docs — Rate Limits: https://docs.discord.com/developers/topics/rate-limits
- [S4] Discord Developer Docs — Message Resource (Create Message, Embed Limits, Attachment): https://docs.discord.com/developers/resources/message
- [S5] Discord Developer Docs — Permissions: https://docs.discord.com/developers/topics/permissions
- [S6] Discord Developer Docs — OAuth2 (`webhook.incoming`): https://docs.discord.com/developers/topics/oauth2
- [S7] Discord Developer Docs — RPC: https://docs.discord.com/developers/topics/rpc
- [S8] Discord Social SDK — Communication Features: https://docs.discord.com/developers/discord-social-sdk/core-concepts/communication-features
- [S9] Discord Social SDK — Sending Direct Messages: https://docs.discord.com/developers/discord-social-sdk/development-guides/sending-direct-messages
- [S10] Discord Social SDK — Linked Channels: https://docs.discord.com/developers/discord-social-sdk/development-guides/linked-channels
- [S11] Discord Support — File Attachments FAQ (อ่านผ่าน search snippet, หน้าตอบ 403): https://support.discord.com/hc/en-us/articles/25444343291031-File-Attachments-FAQ
- [S12] Discord Support — Intro to Webhooks (อ่านผ่าน search snippet, หน้าตอบ 403): https://support.discord.com/hc/en-us/articles/228383668-Intro-to-Webhooks
- [S13] Discord Support — Automated User Accounts (Self-Bots) (อ่านผ่าน search snippet): https://support.discord.com/hc/en-us/articles/115002192352-Automated-User-Accounts-Self-Bots
- [S14] Apple — NSSharingService: https://developer.apple.com/documentation/appkit/nssharingservice
- [S14b] Apple — NSImage: https://developer.apple.com/documentation/appkit/nsimage
- [S15] Apple — NSSharingServicePicker: https://developer.apple.com/documentation/appkit/nssharingservicepicker
- [S16] Apple — NSPasteboard.writeObjects(_:): https://developer.apple.com/documentation/appkit/nspasteboard/writeobjects(_:)
- [S17] Tauri 2 — Clipboard plugin: https://v2.tauri.app/plugin/clipboard/
- [S18] docs.rs — tauri-plugin-clipboard-manager 2.5.0 `Clipboard`: https://docs.rs/tauri-plugin-clipboard-manager/latest/tauri_plugin_clipboard_manager/struct.Clipboard.html
- [S19] docs.rs — arboard 3.7.0 `Clipboard`: https://docs.rs/arboard/latest/arboard/struct.Clipboard.html
- [S20] docs.rs — reqwest 0.13.5 `multipart::Part`: https://docs.rs/reqwest/latest/reqwest/multipart/struct.Part.html
- [S21] docs.rs — imageproc 0.27.0 `draw_text_mut`: https://docs.rs/imageproc/latest/imageproc/drawing/fn.draw_text_mut.html
- [S22] docs.rs — cosmic-text 0.19.0: https://docs.rs/cosmic-text/latest/cosmic_text/
- [S23] docs.rs — ab_glyph 0.2.32: https://docs.rs/ab_glyph/latest/ab_glyph/
