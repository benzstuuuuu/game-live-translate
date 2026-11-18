# Research: ราคาและข้อจำกัดของ Gemini กับ Live Translate

- Ticket: [#5](https://github.com/benzstuuuuu/game-live-translate/issues/5) (อ้างอิง FR-TRN-03, FR-TRN-12, FR-TRN-13, FR-AUD-04, FR-AUD-05, NFR-02, SRS หัวข้อ 10)
- วันที่ค้นคว้า: 2026-11-18
- แหล่งข้อมูล: เอกสาร primary ของ Google เท่านั้น (Gemini API docs, Google Blog, DeepMind model card) รายการเต็มอยู่ที่ [Sources](#sources) ท้ายไฟล์ ทุกแหล่งอ่านเมื่อ 2026-11-18
- หมายเหตุวิธีอ่าน: `ai.google.dev/gemini-api/docs/*` ตอนนี้ redirect ไปที่ `aistudio.google.com/docs/*` (เช่น `/docs/models` ตอบ HTTP 200 ที่ปลายทางใหม่) จึงอ้างลิงก์ปลายทางใหม่ และอ่านเวอร์ชัน Markdown (`.md`) ของแต่ละหน้า
- ข้อความที่ติด **[UNVERIFIED]** คือข้อที่เอกสารไม่ได้ระบุตรง ๆ หรือเป็นสมมติฐานของเรา ต้องวัดจริงก่อนใช้ตัดสินใจ

## สรุปสั้น

- model ID ทั้งสี่ตัวใน SRS **มีจริง**: `gemini-3.8-flash`, `gemini-4-hydrogen`, `gemini-4-carbon` (ทั้งสามเป็น stable) และ `gemini-3.5-live-translate-preview` (preview) [[S1]](https://aistudio.google.com/docs/models)
- สาย subtitle: ประมาณ **$0.57/ชม.** ด้วย `gemini-3.8-flash` ที่ thinking `low` (ราคาถึง 31 ธ.ค. 2026) และราคาจะ **ขึ้นเป็น 2 เท่า** ตั้งแต่ 1 ม.ค. 2027 ส่วน `gemini-4-hydrogen` ที่ `low` อยู่ราว $1.13/ชม. และ `gemini-4-carbon` ที่ `medium` อยู่ราว $6.6/ชม. (ราคาตั้งแต่ 1 ธ.ค. 2026)
- สายเสียง (Live Translate): ราคาประกาศ **≈ $0.0368/นาที** (≈ $2.21/ชม. ถ้ามีเสียงพูดตลอด) ถ้ามีเสียงพูดราวครึ่งเวลาจะอยู่ราว $1.26/ชม. และอาจมีค่า transcription เพิ่ม **[UNVERIFIED]**
- ข้อที่ต้องแก้ใน SRS: Live Translate **รับ `systemInstruction` ได้** ตอน setup (ใช้ใส่ glossary/tone ได้) ส่วน connection มีอายุ ~10 นาที และ session แบบไม่บีบอัดอยู่ได้ 15 นาที จึงต้องทำ reconnect/resumption เอกสารบอก latency แค่ว่า "ตามหลังผู้พูดไม่กี่วินาที" จึงใช้ NFR-02 (≤1.5 s) กับสายเสียงไม่ได้

## 1. Model ที่ใช้แปล subtitle (FR-TRN-12)

### 1.1 ตารางเทียบ

| | `gemini-3.8-flash` | `gemini-4-hydrogen` | `gemini-4-carbon` |
| :--- | :--- | :--- | :--- |
| มีจริง / สถานะ | มี, Stable, latest update ก.ย. 2026 [[S6]](https://aistudio.google.com/docs/models/gemini-3.8-flash) | มี, Stable, latest update พ.ย. 2026 [[S8]](https://aistudio.google.com/docs/models/gemini-4-hydrogen) | มี, Stable, latest update พ.ย. 2026 [[S10]](https://aistudio.google.com/docs/models/gemini-4-carbon) |
| Input (Standard, ต่อ 1M tokens) | $0.75 ถึง 31 ธ.ค. 2026 → **$1.50 ตั้งแต่ 1 ม.ค. 2027** [[S2]](https://aistudio.google.com/docs/pricing) | $1.50 [[S2]](https://aistudio.google.com/docs/pricing) | $2.00 ถึง 30 พ.ย. 2026 → **$4.00 ตั้งแต่ 1 ธ.ค. 2026** [[S2]](https://aistudio.google.com/docs/pricing) |
| Output รวม thinking tokens (ต่อ 1M) | $3.75 → **$7.50 ตั้งแต่ 1 ม.ค. 2027** [[S2]](https://aistudio.google.com/docs/pricing) | $7.50 [[S2]](https://aistudio.google.com/docs/pricing) | $10.00 → **$20.00 ตั้งแต่ 1 ธ.ค. 2026** [[S2]](https://aistudio.google.com/docs/pricing) |
| Free tier | มี ("Free of charge") [[S2]](https://aistudio.google.com/docs/pricing) | มี ("Free of charge") [[S2]](https://aistudio.google.com/docs/pricing) | **ไม่มี** ("not available on the Free tier") [[S2]](https://aistudio.google.com/docs/pricing) |
| Thinking levels ที่รับ | `low`, `medium` (default), `high` และ `minimal` จะ error [[S5]](https://aistudio.google.com/docs/thinking?generate-content=true) [[S7]](https://aistudio.google.com/docs/whats-new-gemini-3.8?generate-content=true) | `low`, `medium`, `high` (default), `xhigh` และ `minimal` จะ error [[S5]](https://aistudio.google.com/docs/thinking?generate-content=true) [[S9]](https://aistudio.google.com/docs/whats-new-gemini-4-hydrogen?generate-content=true) | `low`, `medium` (default), `high` [[S5]](https://aistudio.google.com/docs/thinking?generate-content=true) |
| Context / max output | input 1M แยกกับ output 64K [[S9]](https://aistudio.google.com/docs/whats-new-gemini-4-hydrogen?generate-content=true) | window รวม 1,048,576, output 65,536 ต่อ request [[S8]](https://aistudio.google.com/docs/models/gemini-4-hydrogen) | window รวม 1,048,576, output 65,536 ต่อ request [[S10]](https://aistudio.google.com/docs/models/gemini-4-carbon) |
| Latency ที่ประกาศ | ไม่มีตัวเลข มีแค่ว่า "Low thinking effort: Reduces time-to-answer for ... real-time chat" [[S7]](https://aistudio.google.com/docs/whats-new-gemini-3.8?generate-content=true) | ไม่มีตัวเลข มีแค่ "Fast and efficient" [[S9]](https://aistudio.google.com/docs/whats-new-gemini-4-hydrogen?generate-content=true) | ไม่มีตัวเลข และระบุว่า `high` = "Most tokens, slowest first token" และ Carbon ใช้ thinking tokens ต่อ turn มากกว่า Argon ทำให้ latency สูงกว่า [[S11]](https://aistudio.google.com/docs/whats-new-gemini-4-carbon?generate-content=true) |

### 1.2 ข้อสังเกตที่กระทบ implementation

- `thinkingLevel` เป็นพารามิเตอร์ที่ **จำเป็น (required) สำหรับ Gemini 4** และแนะนำให้ใส่สำหรับ Gemini 3 [[S5]](https://aistudio.google.com/docs/thinking?generate-content=true) UI ของ FR-TRN-12 จึงต้องจำกัดตัวเลือก thinking ตาม model ID เพราะ `minimal` ใช้ไม่ได้กับทั้งสามตัว และ `xhigh` ใช้ได้เฉพาะ Hydrogen
- thinking tokens คิดราคาเท่า output ("Output price (including thinking tokens)") [[S2]](https://aistudio.google.com/docs/pricing) ค่าใช้จ่ายจึงขึ้นกับ thinking level มากกว่าความยาวคำแปล
- Hydrogen คือ "Recommended upgrade from Gemini 3.8 Flash" [[S9]](https://aistudio.google.com/docs/whats-new-gemini-4-hydrogen?generate-content=true) และตั้งแต่ 1 ม.ค. 2027 ราคา 3.8 Flash จะเท่ากับ Hydrogen ($1.50/$7.50) [[S2]](https://aistudio.google.com/docs/pricing)
- Gemini 4 ไม่ใช้ `temperature`, `top_p`, `top_k`, `candidate_count` แล้ว ถ้าส่งค่าในช่วงที่รับได้ server จะ ignore แต่ถ้าส่งค่านอกช่วงจะได้ `400` [[S9]](https://aistudio.google.com/docs/whats-new-gemini-4-hydrogen?generate-content=true)
- Hydrogen/Carbon รองรับเฉพาะ implicit caching ซึ่งทำงานกับ prompt ตั้งแต่ 4,096 tokens ขึ้นไป [[S9]](https://aistudio.google.com/docs/whats-new-gemini-4-hydrogen?generate-content=true) prompt ของเรายาวราว 560 tokens จึงไม่ได้ส่วนลด cache
- Hydrogen แนะนำให้ตั้ง HTTP timeout อย่างน้อย 20 นาทีต่อ call [[S9]](https://aistudio.google.com/docs/whats-new-gemini-4-hydrogen?generate-content=true) คำแนะนำนี้เขียนไว้สำหรับงาน long decode ส่วน fallback 3 s ของเรายกเลิก request เองอยู่แล้ว แต่ request ที่ถูกยกเลิกยังถูกคิดเงินหรือไม่ เอกสารไม่ได้บอก **[UNVERIFIED]**
- ตัวเลือกที่ไม่ได้อยู่ใน SRS: `gemini-3.5-flash-lite` ราคา $0.30/$2.50 ต่อ 1M tokens [[S2]](https://aistudio.google.com/docs/pricing) default thinking = `minimal` [[S5]](https://aistudio.google.com/docs/thinking?generate-content=true) และ Google อธิบายว่าเป็น "fastest, lowest-cost model ... sub-second latency targets" [[S9]](https://aistudio.google.com/docs/whats-new-gemini-4-hydrogen?generate-content=true) น่าทดลองเทียบกับ NFR-02

### 1.3 Rate limits และ free tier

- Gemini API ไม่ได้เผยแพร่ RPM/TPM/RPD ของ model เหล่านี้ไว้ในเอกสาร ต้องดูใน AI Studio ของแต่ละ project ("can be viewed in Google AI Studio") และ "Specified rate limits are not guaranteed" [[S3]](https://aistudio.google.com/docs/rate-limits) ตัวเลข free tier ที่ใช้จริงจึงยังไม่ทราบ **[UNVERIFIED]**
- limit นับต่อ project ไม่ใช่ต่อ API key และ RPD reset ตอนเที่ยงคืนเวลา Pacific [[S3]](https://aistudio.google.com/docs/rate-limits)
- preview/experimental model มี rate limit เข้มกว่า [[S3]](https://aistudio.google.com/docs/rate-limits) และ preview model จะถูก deprecate โดยแจ้งล่วงหน้าอย่างน้อย 2 สัปดาห์ [[S1]](https://aistudio.google.com/docs/models)
- Usage tiers: Free → Tier 1 (ผูก billing, billing cap $250) → Tier 2 (จ่ายครบ $100 + 3 วัน) → Tier 3 (จ่ายครบ $1,000 + 30 วัน) และมี spend-based limit ต่อ 10 นาที (Tier 1 = $10) ถ้าเกินจะได้ `429 RESOURCE_EXHAUSTED` [[S3]](https://aistudio.google.com/docs/rate-limits) ซึ่งค่าใช้จ่ายของเราต่ำกว่านั้นมาก
- free tier: prompt/response "Used to improve our products: Yes" ส่วน paid tier เป็น "No" [[S2]](https://aistudio.google.com/docs/pricing) [[S4]](https://aistudio.google.com/docs/billing)
- ปริมาณใช้งานของเราคือ 600 บรรทัด/ชม. ≈ 10 RPM เฉลี่ย (เป็น burst ได้ช่วงบทสนทนาถี่) จะพอกับ free tier หรือไม่ต้องดูตัวเลขจริงใน AI Studio **[UNVERIFIED]**

## 2. `gemini-3.5-live-translate-preview` (FR-AUD-04, FR-AUD-05)

### 2.1 ข้อเท็จจริงหลัก

| หัวข้อ | รายละเอียด |
| :--- | :--- |
| มีจริง / สถานะ | มี เป็น **Preview** และ latest update มิ.ย. 2026 [[S12]](https://aistudio.google.com/docs/models/gemini-3.5-live-translate-preview) เปิดตัวบน Google Blog วันที่ 2026-06-09 [[S20]](https://blog.google/innovation-and-ai/models-and-research/gemini-models/gemini-live-3-5-translate/) |
| ราคา (paid) | input audio $3.50/1M tokens ≈ **$0.0053/นาที**, output audio $21.00/1M tokens ≈ **$0.0315/นาที**, คิดที่ 25 tokens/วินาทีของเสียง รวมเป็น "approximately **$0.0368 per minute**" [[S2]](https://aistudio.google.com/docs/pricing#gemini-3.5-live-translate-preview) |
| Free tier | input/output "Free of charge" (แลกกับข้อมูลถูกนำไปใช้ปรับปรุงผลิตภัณฑ์) [[S2]](https://aistudio.google.com/docs/pricing#gemini-3.5-live-translate-preview) |
| Input / output | input = Audio (speech) เท่านั้น, output = Audio (translated speech) + Text (transcript) [[S12]](https://aistudio.google.com/docs/models/gemini-3.5-live-translate-preview) โดยไม่รองรับ text input [[S13]](https://aistudio.google.com/docs/live-api/live-translate) |
| Token limits | input 131,072, output 65,536 [[S12]](https://aistudio.google.com/docs/models/gemini-3.5-live-translate-preview) |
| ฟีเจอร์ที่ไม่รองรับ | Thinking, function calling, structured outputs, context caching, Batch/Flex/Priority [[S12]](https://aistudio.google.com/docs/models/gemini-3.5-live-translate-preview) |
| en→th | รองรับ: Thai = `th` อยู่ในรายการ 70+ ภาษา [[S13]](https://aistudio.google.com/docs/live-api/live-translate#supported-languages) และ model ตรวจภาษาต้นทางเองอัตโนมัติ [[S20]](https://blog.google/innovation-and-ai/models-and-research/gemini-models/gemini-live-3-5-translate/) |
| Latency ที่ประกาศ | ไม่มีตัวเลข ms มีแค่ว่าสร้างเสียงแปลต่อเนื่องโดยไม่รอผู้พูดพูดจบ และ "stays just a few seconds behind the speaker throughout the session" [[S20]](https://blog.google/innovation-and-ai/models-and-research/gemini-models/gemini-live-3-5-translate/) |
| Prompt / glossary | **รับได้** ผ่าน `systemInstruction` ใน `setup` message เพื่อกำหนด "Custom terminology and glossaries", "Formality and register" และบริบทผู้พูด แต่แก้กลาง session ไม่ได้ ต้องเปิด session ใหม่ และคำสั่งที่ขัดกับ `targetLanguageCode`, เปลี่ยนรูปแบบ output หรือสั่งงานที่ไม่ใช่การแปลอาจถูก ignore [[S13]](https://aistudio.google.com/docs/live-api/live-translate#system-instructions) |

### 2.2 ข้อจำกัดของ preview ที่ระบุในเอกสาร

- เสียงแปล (voice replication) ไม่คงที่: เสียงอาจเปลี่ยนหลังเงียบนาน หรือเลือกเพศผิด [[S13]](https://aistudio.google.com/docs/live-api/live-translate#limitations) [[S21]](https://deepmind.google/models/model-cards/gemini-3-5-audio/)
- ตรวจภาษาพลาดได้เมื่อสำเนียงหนัก ภาษาคล้ายกัน หรือสลับภาษาเร็ว แต่เอกสารบอกว่ากระทบเฉพาะ input transcript และคำแปลยังควรถูก [[S13]](https://aistudio.google.com/docs/live-api/live-translate#limitations)
- กรอง noise/เพลงได้แต่ไม่ทั้งหมด [[S13]](https://aistudio.google.com/docs/live-api/live-translate#limitations) ซึ่งเสียงเกมมีทั้งเพลงและเอฟเฟกต์ปน จึงควรทดสอบกับเกมจริง **[UNVERIFIED]**
- `echoTargetLanguage` มีค่า default เป็น `false` คือ model จะเงียบเมื่อได้ยินภาษาเป้าหมาย [[S13]](https://aistudio.google.com/docs/live-api/live-translate#setup-message-configuration) และถ้าเปิดไว้ background noise อาจทำให้เสียงมี artifact [[S21]](https://deepmind.google/models/model-cards/gemini-3-5-audio/)
- ความเสี่ยงทั่วไปของ preview: rate limit เข้มกว่า และอาจถูก deprecate โดยแจ้งล่วงหน้า ≥2 สัปดาห์ [[S1]](https://aistudio.google.com/docs/models) [[S3]](https://aistudio.google.com/docs/rate-limits)
- rate limit ของ Live API เช่นจำนวน concurrent sessions ไม่มีในเอกสารสาธารณะ **[UNVERIFIED]**

### 2.3 Session lifecycle (สำคัญสำหรับการเล่นเกมหลายชั่วโมง)

- ถ้าไม่เปิด compression, session แบบ audio-only อยู่ได้ **15 นาที** และ **connection มีอายุราว 10 นาที** ก่อน server ส่ง `GoAway` (มี `timeLeft`) แล้วตัดเป็น ABORTED [[S16]](https://aistudio.google.com/docs/live-api/session-management)
- ทางแก้ที่เอกสารแนะนำ: `contextWindowCompression` ทำให้ session ยาวได้ไม่จำกัด และ session resumption (เก็บ handle จาก `SessionResumptionUpdate` ซึ่งใช้ได้ 2 ชม. หลัง session จบ) [[S16]](https://aistudio.google.com/docs/live-api/session-management) [[S18]](https://aistudio.google.com/docs/live-api/best-practices#session-management)
- เอกสารไม่ได้ยืนยันว่า Live Translate model รองรับ compression/resumption เหมือน Live model ทั่วไป **[UNVERIFIED]** ต้องทดสอบใน spike
- context window ของ session: 128K tokens สำหรับ native audio models [[S17]](https://aistudio.google.com/docs/live-api/capabilities#context-window) ตรงกับ model card ที่ระบุ 128K [[S21]](https://deepmind.google/models/model-cards/gemini-3-5-audio/)

### 2.4 Protocol notes สำหรับ WebSocket client ใน Rust

- Protocol: stateful WebSocket (WSS) [[S19]](https://aistudio.google.com/docs/live-api#technical-specifications)
- Endpoint (API key): `wss://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent?key={API_KEY}` [[S13]](https://aistudio.google.com/docs/live-api/live-translate) [[S14]](https://aistudio.google.com/docs/live-api/get-started-websocket)
- Endpoint (ephemeral token): `...BidiGenerateContentConstrained?access_token={token}` บน `v1beta` [[S14]](https://aistudio.google.com/docs/live-api/get-started-websocket) โดยปกติ Live API เป็น server-to-server และถ้า client ต่อเองควรใช้ ephemeral token [[S17]](https://aistudio.google.com/docs/live-api/capabilities#client-authentication) แอปเราเป็น desktop app ที่ Rust core ต่อ socket เอง จึงต้องตัดสินใจว่า API key อยู่ที่ใคร (นอก scope ของ ticket นี้)
- message แรกต้องเป็น `setup` [[S14]](https://aistudio.google.com/docs/live-api/get-started-websocket) และ `translationConfig` ต้องอยู่ใน `generationConfig` [[S13]](https://aistudio.google.com/docs/live-api/live-translate#configuration) ตัวอย่างโครงสร้างที่ปรับให้เข้ากับเรา:

  ```json
  {
    "setup": {
      "model": "models/gemini-3.5-live-translate-preview",
      "systemInstruction": { "parts": [{ "text": "<tone + glossary>" }] },
      "inputAudioTranscription": {},
      "outputAudioTranscription": {},
      "generationConfig": {
        "responseModalities": ["AUDIO"],
        "translationConfig": { "targetLanguageCode": "th", "echoTargetLanguage": false }
      }
    }
  }
  ```

- ส่งเสียงด้วย `{"realtimeInput":{"audio":{"data":"<base64>","mimeType":"audio/pcm;rate=16000"}}}` ในรูปแบบ raw 16-bit PCM, 16 kHz, mono, little-endian และส่งเป็น chunk ละ 100 ms [[S13]](https://aistudio.google.com/docs/live-api/live-translate#sending-audio) ซึ่งคำนวณได้เป็น 1,600 samples = 3,200 bytes ต่อ chunk หรือ 10 messages/วินาที (คำนวณเอง)
- ข้อความขาเข้าเป็น JSON `BidiGenerateContentServerMessage` [[S14]](https://aistudio.google.com/docs/live-api/get-started-websocket#receiving-responses) ฟิลด์ที่ใช้คือ `serverContent.inputTranscription` / `serverContent.outputTranscription` (`text`, `languageCode`, `startOffset`, `endOffset`), `serverContent.modelTurn.parts[].inlineData` (เสียง 24 kHz PCM), `usageMetadata`, `goAway`, `sessionResumptionUpdate` [[S15]](https://aistudio.google.com/api-reference/live) [[S13]](https://aistudio.google.com/docs/live-api/live-translate)
- ตัวอย่างทุกตัวตั้ง `responseModalities: ["AUDIO"]` และเอกสาร capabilities ระบุว่า native audio models รองรับเฉพาะ `AUDIO` ถ้าต้องการ text ให้ใช้ output transcription [[S17]](https://aistudio.google.com/docs/live-api/capabilities#response-modalities) แปลว่าเราจะ **จ่ายค่า output audio แม้จะทิ้งเสียงไปแล้วใช้แค่ transcript** ส่วนการตั้งเป็น TEXT อย่างเดียวได้หรือไม่ **[UNVERIFIED]**
- หมายเหตุของเรา (ไม่ได้มาจากเอกสาร Google): Rust client ควรรับได้ทั้ง Text frame และ Binary frame ที่บรรจุ JSON และต้อง resample เสียงที่ capture ได้ (โดยมากเป็น 48 kHz float) ลงเป็น 16 kHz s16le mono ก่อนส่ง

## 3. ประมาณค่าใช้จ่ายต่อชั่วโมงที่เล่น

### 3.1 สาย subtitle

สมมติฐาน (ของเรา ไม่ใช่ตัวเลขจาก Google):

| รายการ | ค่า |
| :--- | :--- |
| จำนวนบรรทัด/ชม. | 600 |
| Input ต่อ request | system/tone prompt ~250 + บรรทัดปัจจุบัน 60 + context 3 บรรทัด 180 + glossary ~40 + extra prompt ~30 ≈ **560 tokens** |
| Output ที่มองเห็น ต่อ request | ~40 tokens |
| Thinking tokens ต่อ request **[UNVERIFIED]** | `low` ≈ 100, `medium` ≈ 400, `high` ≈ 1,000 (ต้องวัดจาก `usageMetadata` จริง) |
| ต่อชั่วโมง | input 336K tokens, visible output 24K tokens, thinking 60K / 240K / 600K tokens |

ผลลัพธ์ (USD/ชม., Standard tier, คำนวณจากราคาใน [[S2]](https://aistudio.google.com/docs/pricing)):

| Model (ราคาที่ใช้) | ไม่มี thinking (ขั้นต่ำทางทฤษฎี) | `low` | `medium` | `high` |
| :--- | ---: | ---: | ---: | ---: |
| `gemini-3.8-flash` ($0.75/$3.75 ถึง 31 ธ.ค. 2026) | 0.34 | **0.57** | 1.24 (default) | 2.59 |
| `gemini-3.8-flash` ($1.50/$7.50 ตั้งแต่ 1 ม.ค. 2027) | 0.68 | 1.13 | 2.48 (default) | 5.18 |
| `gemini-4-hydrogen` ($1.50/$7.50) | 0.68 | **1.13** | 2.48 | 5.18 (default) |
| `gemini-4-carbon` ($4/$20 ตั้งแต่ 1 ธ.ค. 2026) | 1.82 | 3.02 | 6.62 (default) | 13.82 |
| `gemini-3.5-flash-lite` ($0.30/$2.50) ใช้เทียบ | 0.16 (`minimal` default) | 0.31 | — | — |

ตีความ: ราคาต่อบรรทัดของ `gemini-3.8-flash` ที่ `low` อยู่ที่ราว $0.00095 ตัวที่ทำให้ค่าใช้จ่ายแกว่งมากที่สุดคือ thinking level ไม่ใช่ความยาว prompt ส่วน Carbon แพงกว่า Flash ราว 5 เท่าที่ level เดียวกัน และไม่มี free tier จึงไม่เหมาะเป็น default ของสายนี้

### 3.2 สายเสียง (Live Translate)

ใช้ราคาจาก [[S2]](https://aistudio.google.com/docs/pricing#gemini-3.5-live-translate-preview): input $0.00525/นาทีของเสียงที่ส่ง และ output $0.0315/นาทีของเสียงแปลที่ได้กลับมา สมมติฐานคือ Rust ส่ง PCM ต่อเนื่องตลอด 60 นาที (input = 60 นาที) ส่วน output ขึ้นกับสัดส่วนเวลาที่มีบทพูดในเกม **[UNVERIFIED]**

| สัดส่วนเวลาที่มีบทพูด | Input | Output | **รวม/ชม.** | ถ้ามี transcription surcharge **[UNVERIFIED]** |
| :--- | ---: | ---: | ---: | ---: |
| 100% (worst case เท่ากับราคาประกาศ $0.0368/นาที) | 0.32 | 1.89 | **2.21** | ~2.87 |
| 50% | 0.32 | 0.95 | **1.26** | ~1.59 |
| 25% | 0.32 | 0.47 | **0.79** | ~0.95 |

ความไม่แน่นอนที่ต้องวัดใน spike:

- **Transcription surcharge:** เอกสาร best practices ของ Live API บอกว่าเมื่อเปิด `inputAudioTranscription`/`outputAudioTranscription` จะคิดค่า text tokens ของ transcript ที่ "text token output rate" เพิ่มจากค่า audio [[S18]](https://aistudio.google.com/docs/live-api/best-practices#audio-tokens-and-transcriptions) แต่ตารางราคาของ Live Translate ไม่ได้แยก rate ของ text ไว้ คอลัมน์ขวาในตารางด้านบนจึงสมมติว่า transcript อังกฤษ ~175 tokens/นาทีของบทพูด (ใช้ตัวเลขที่ Google ใช้ประมาณราคา Transcribe Live [[S2]](https://aistudio.google.com/docs/pricing#gemini-3.5-transcribe-live)) บวก transcript ไทย ~350 tokens/นาที คิดที่ $21/1M
- **Compounding context:** Live API โดยทั่วไปคิดเงิน "per turn for all tokens present in the session context window" [[S18]](https://aistudio.google.com/docs/live-api/best-practices#session-context-window) แต่ footnote ราคาของ Live Translate บอกว่าคิดจาก "total input and output audio token consumption" [[S2]](https://aistudio.google.com/docs/pricing#gemini-3.5-live-translate-preview) ซึ่งฟังดูเป็นเชิงเส้น ถ้าคิดแบบ compounding จริงค่าใช้จ่ายจะสูงกว่าตารางมาก **[UNVERIFIED]** ต้องดู `usageMetadata` ระหว่าง session ยาว
- **ลด input cost:** ถ้าทำ client-side VAD แล้วส่งเฉพาะช่วงที่มีเสียงพูด input จะลดตามสัดส่วน (ที่ 50% ยอดรวมจะเหลือราว $1.10/ชม.) แต่เอกสารไม่ได้บอกว่าการหยุดส่งเสียงกระทบคุณภาพของ continuous translation หรือไม่ **[UNVERIFIED]**
- **ทางเลือกอื่นเพื่อเทียบ:** `gemini-3.5-transcribe-live` ราคา blended ~$0.009/นาที [[S2]](https://aistudio.google.com/docs/pricing#gemini-3.5-transcribe-live) (≈ $0.54/ชม. ถ้าส่งต่อเนื่อง) แล้วส่งข้อความต่อเข้าสาย subtitle ราว $0.3–0.6/ชม. วิธีนี้รับ context/glossary/tone ได้ครบเหมือนสาย subtitle แต่ latency รวมน่าจะสูงกว่า **[UNVERIFIED]**

## 4. สิ่งที่กระทบ SRS

1. **FR-TRN-12:** model ID ทั้งสามมีจริง แต่ต้องปรับ:
   - ตัวเลือก thinking level ต้องผูกกับ model (ห้าม `minimal` และ `xhigh` ใช้ได้เฉพาะ Hydrogen)
   - default thinking ของ Hydrogen คือ `high` ซึ่งแพงและช้า สำหรับสาย subtitle ควรตั้ง `low` เอง
   - ควรระบุว่า Carbon ไม่มี free tier และมีราคาสูงกว่าตัวอื่นมาก
2. **ราคาจะเปลี่ยนเร็ว ๆ นี้:** Carbon ขึ้นราคา 2 เท่าในวันที่ 1 ธ.ค. 2026 และ 3.8 Flash ขึ้น 2 เท่าในวันที่ 1 ม.ค. 2027 [[S2]](https://aistudio.google.com/docs/pricing) หลังจากนั้น 3.8 Flash จะไม่ถูกกว่า Hydrogen แล้ว
3. **NFR-02 (median ≤1.5 s):** ไม่มีเอกสารใดประกาศ latency เป็นตัวเลข ms สำหรับ model ทั้งสาม ต้องวัดเองใน spike และ fallback ที่ 3 s ต้องทดสอบกับ thinking level ที่จะใช้จริง
4. **FR-AUD-04 (Live Translate):**
   - ข้อที่ว่า "ไม่รับ prompt/glossary" **ไม่ถูกต้องแล้ว** เพราะรับ `systemInstruction` ตอน setup ได้ ทั้ง glossary และ tone แต่ถ้าแก้ glossary/tone ต้องเปิด session ใหม่ [[S13]](https://aistudio.google.com/docs/live-api/live-translate#system-instructions)
   - ต้องมี requirement เรื่อง reconnect: connection อยู่ได้ ~10 นาที และ session ไม่บีบอัดอยู่ได้ 15 นาที จึงต้องจัดการ `GoAway` + session resumption + `contextWindowCompression` [[S16]](https://aistudio.google.com/docs/live-api/session-management) ไม่อย่างนั้นเสียงจะขาดทุก ~10 นาที
   - latency ที่ประกาศคือ "ตามหลังผู้พูดไม่กี่วินาที" [[S20]](https://blog.google/innovation-and-ai/models-and-research/gemini-models/gemini-live-3-5-translate/) จึงใช้ NFR-02 และกฎ fallback 3 s กับสายเสียงไม่ได้ ควรมี NFR แยก
   - เราต้องจ่ายค่า output audio แม้จะใช้แค่ transcript [[S17]](https://aistudio.google.com/docs/live-api/capabilities#response-modalities)
   - model ยังเป็น Preview จึงเปลี่ยนหรือถูกถอดได้ด้วยการแจ้งล่วงหน้า ≥2 สัปดาห์ [[S1]](https://aistudio.google.com/docs/models)
5. **SRS หัวข้อ 10 (คำถามเรื่องราคา Live Translate):** คำตอบคือ $3.50/1M input + $21.00/1M output audio tokens ที่ 25 tokens/วินาที หรือ ≈ $0.0368/นาที (≈ $2.21/ชม. ถ้ามีเสียงพูดตลอด) ใน free tier ใช้ได้ฟรี [[S2]](https://aistudio.google.com/docs/pricing#gemini-3.5-live-translate-preview)

## 5. สิ่งที่ยังไม่ได้ตรวจ

- ราคาบน Gemini Enterprise Agent Platform (Vertex) ซึ่งเอกสารบอกว่า "Prices may differ" [[S2]](https://aistudio.google.com/docs/pricing#notes) ticket นี้ใช้เฉพาะราคา Gemini Developer API
- ตัวเลข RPM/TPM/RPD จริงของแต่ละ model ซึ่งดูได้เฉพาะใน AI Studio ของ project
- จำนวน thinking tokens จริงต่อบรรทัด, latency จริง, การคิดค่า transcript และ compounding ของ Live Translate ซึ่งทั้งหมดต้องวัดใน spike

## Sources

ทุกแหล่งอ่านเมื่อ **2026-11-18**

| ID | หน้า | URL |
| :--- | :--- | :--- |
| S1 | Gemini models (รวม model version patterns) | https://aistudio.google.com/docs/models (เดิม https://ai.google.dev/gemini-api/docs/models) |
| S2 | Gemini Developer API pricing | https://aistudio.google.com/docs/pricing |
| S3 | Rate limits | https://aistudio.google.com/docs/rate-limits |
| S4 | Billing | https://aistudio.google.com/docs/billing |
| S5 | Thinking (generateContent) | https://aistudio.google.com/docs/thinking?generate-content=true |
| S6 | Model page: Gemini 3.8 Flash | https://aistudio.google.com/docs/models/gemini-3.8-flash |
| S7 | What's new in Gemini 3.8 Flash | https://aistudio.google.com/docs/whats-new-gemini-3.8?generate-content=true |
| S8 | Model page: Gemini 4 Hydrogen | https://aistudio.google.com/docs/models/gemini-4-hydrogen |
| S9 | What's new in Gemini 4 Hydrogen | https://aistudio.google.com/docs/whats-new-gemini-4-hydrogen?generate-content=true |
| S10 | Model page: Gemini 4 Carbon | https://aistudio.google.com/docs/models/gemini-4-carbon |
| S11 | What's new in Gemini 4 Carbon | https://aistudio.google.com/docs/whats-new-gemini-4-carbon?generate-content=true |
| S12 | Model page: Gemini 3.5 Live Translate | https://aistudio.google.com/docs/models/gemini-3.5-live-translate-preview |
| S13 | Live translation with Gemini Live API | https://aistudio.google.com/docs/live-api/live-translate |
| S14 | Get started with Live API using WebSockets | https://aistudio.google.com/docs/live-api/get-started-websocket |
| S15 | Live API WebSockets API reference | https://aistudio.google.com/api-reference/live |
| S16 | Session management with Live API | https://aistudio.google.com/docs/live-api/session-management |
| S17 | Live API capabilities guide | https://aistudio.google.com/docs/live-api/capabilities |
| S18 | Live API best practices | https://aistudio.google.com/docs/live-api/best-practices |
| S19 | Gemini Live API overview | https://aistudio.google.com/docs/live-api |
| S20 | Google Blog: Gemini 3.5 Live Translate (published 2026-06-09) | https://blog.google/innovation-and-ai/models-and-research/gemini-models/gemini-live-3-5-translate/ |
| S21 | DeepMind model card: Gemini 3.5 Audio | https://deepmind.google/models/model-cards/gemini-3-5-audio/ |
