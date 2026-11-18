# Yokoten

Yokoten 横展 เฟสแรกคือ server บน Mac (คำสั่ง `yokoten`) ที่อ่าน subtitle หรือฟังเสียงภาษาอังกฤษจากเกมผ่าน OBS แล้วส่งคำแปลไทยขึ้นสตรีม Discord และหน้าอ่านบนมือถือแบบสด ไฟล์นี้กำหนดคำที่ใช้ในเอกสาร issue และโค้ด รายละเอียดเชิงสเปกอยู่ใน [Yokoten SRS](https://claude.ai/artifact/LJ4WDjTP8L5k6kdr8Ydr2m)

## เกมและแหล่งภาพ

**เครื่องเกม**:
Console ที่เล่นเกม เช่น PS5 และต่อเข้า Mac ผ่าน capture card
_Avoid_: คอนโซล, ตัวเครื่อง

**Capture card**:
อุปกรณ์ที่รับ HDMI จากเครื่องเกม แล้วส่งภาพและเสียงเข้า Mac ทาง USB
_Avoid_: การ์ดจับภาพ, grabber, dongle

**Loop out**:
ช่อง HDMI ของ capture card ที่ส่งภาพจากเครื่องเกมต่อไป TV ตรงโดยไม่ผ่าน Mac ผู้เล่นดูเกมทางนี้
_Avoid_: passthrough, HDMI out

**OBS**:
แอปบน Mac ที่รับภาพและเสียงจาก capture card และเป็นต้นทางของสตรีม Yokoten คุยกับ OBS ผ่าน obs-websocket
_Avoid_: viewer, encoder

**Source การ์ด**:
Input ใน OBS ที่เป็นภาพและเสียงจาก capture card หาจากชื่ออุปกรณ์ เพราะ device ID เปลี่ยนเมื่อถอดเสียบ
_Avoid_: device, capture source

**Scene yokoten-ocr**:
Scene ใน OBS ที่มีเฉพาะ source การ์ด และ crop ตามกรอบ subtitle เพื่อให้ Yokoten อ่านภาพ ไม่ใช่ Program จึงไม่ออกสตรีม
_Avoid_: OCR scene

## โปรไฟล์และกรอบ

**โปรไฟล์เกม**:
ชุดค่าที่จำไว้ต่อเกมหนึ่งเกม ได้แก่ แหล่งข้อความ, กรอบ subtitle, ตำแหน่งแถบสตรีม, glossary ของเกม และประวัติ ผู้ใช้เลือกเองทุกครั้งที่เปลี่ยนเกม
_Avoid_: preset, game settings

**กรอบ subtitle**:
พื้นที่สี่เหลี่ยมบนภาพเกม 16:9 ที่ Yokoten อ่านและ OCR หนึ่งโปรไฟล์เกมมีหนึ่งกรอบ
_Avoid_: ROI, พื้นที่จับภาพ

## ข้อความและการแปล

**แหล่งข้อความ**:
ที่มาของข้อความต้นฉบับของโปรไฟล์เกม คือ subtitle หรือเสียง โปรไฟล์หนึ่งใช้ได้ทีละแหล่ง
_Avoid_: input, โหมดแปล

**สาย subtitle**:
เส้นทางที่ได้ข้อความจากการอ่านภาพกรอบ subtitle ผ่าน OBS แล้ว OCR
_Avoid_: OCR mode, โหมดภาพ

**สายเสียง**:
เส้นทางที่ส่งเสียงจาก capture card ให้ Live Translate แล้วได้บรรทัดอังกฤษคู่ไทย
_Avoid_: audio mode, โหมดเสียง

**Live Translate**:
บริการของ Gemini ที่รับเสียงอังกฤษแล้วคืน transcript อังกฤษ คำแปลไทย และเสียงไทย ไม่ผ่านตัวแปล แต่รับ glossary กับโทนเป็น system instruction ตอนเริ่ม session
_Avoid_: speech translation, realtime mode

**บรรทัด**:
ข้อความต้นฉบับหนึ่งชุดจาก subtitle หรือเสียง พร้อมคำแปลไทย เป็นหน่วยของประวัติ
_Avoid_: ประโยค, entry, message

**ตัวแปล**:
เครื่องมือที่แปลข้อความอังกฤษเป็นไทยในสาย subtitle ในเฟสแรกคือ Apple Translation และ Gemini ทั้งแอปใช้ตัวแปลเดียวในเวลาหนึ่ง
_Avoid_: engine, provider, model

**ตัวแปลสำรอง**:
ตัวแปลที่ใช้แทนเมื่อตัวแปลที่เลือกใช้ไม่ได้ ในเฟสแรกคือ Apple Translation
_Avoid_: backup translator

**Fallback**:
การสลับไปใช้ตัวแปลสำรองเมื่อตัวแปลที่เลือกล้มเหลวหรือช้าเกินกำหนด แล้วกลับไปใช้ตัวเดิมเมื่อใช้ได้อีกครั้ง
_Avoid_: failover, สลับอัตโนมัติ

**Glossary**:
คู่คำต้นฉบับกับคำแปลที่ตัวแปลต้องใช้ทุกครั้งที่คำนั้นปรากฏ มีสองชั้นคือ glossary กลางและ glossary ของเกม
_Avoid_: dictionary, คำศัพท์, term list

**Glossary กลาง**:
Glossary ที่ใช้กับทุกเกม
_Avoid_: global glossary

**Glossary ของเกม**:
Glossary ในโปรไฟล์เกม เมื่อคำซ้ำกับ glossary กลางให้ใช้ของเกม
_Avoid_: local glossary

**บรรทัดบริบท**:
บรรทัดก่อนหน้าที่ส่งไปพร้อมข้อความที่กำลังแปล เพื่อให้คำแปลต่อเนื่องกัน
_Avoid_: history, memory

**โทน**:
ระดับภาษาไทยของคำแปลที่ผู้ใช้เลือก ค่าเริ่มต้นคือภาษาพูด
_Avoid_: style, register

## สิ่งที่ผู้ใช้เห็น

**หน้าอ่าน**:
หน้าเว็บ `/reader` บนมือถือใน LAN ที่แสดงบรรทัดล่าสุด (ไทยตัวใหญ่ อังกฤษตัวเล็ก) กับประวัติของ session และมีปุ่มเริ่มหรือหยุดแปล
_Avoid_: side window, หน้าต่างข้างเกม, web viewer

**แถบสตรีม**:
แถบคำแปลไทยบนภาพสตรีม มาจากหน้า `/overlay` ที่ OBS ใส่เป็น Browser Source คนดูใน Discord เห็น ผู้เล่นบน TV ไม่เห็น
_Avoid_: overlay, subtitle bar, HUD

**หน้าตั้งค่า**:
หน้าเว็บ `/setup` ที่เปิดได้จาก Mac เครื่องเดียวกันเท่านั้น ใช้ตั้ง key, โปรไฟล์, กรอบ subtitle, แถบสตรีม และ glossary
_Avoid_: settings window, admin

**Session**:
ช่วงตั้งแต่กดเริ่มแปลจนกดหยุด หน้าอ่านแสดงประวัติของ session ปัจจุบัน
_Avoid_: รอบเล่น, run

**ประวัติ**:
บรรทัดทั้งหมดของโปรไฟล์เกม เรียงตามเวลา
_Avoid_: log, transcript

## แผนงาน

**เฟส**:
ขอบเขตงานทั้งชุดที่ส่งมอบพร้อมกัน เฟสแรกคือสายสตรีม ช่วง M0 ถึง M4 ส่วนแอป Tauri และเกม Steam บน Mac อยู่ในเฟส 2
_Avoid_: release, version

**ช่วง**:
ขั้นของแผนพัฒนาเฟสแรก มีห้าช่วงตั้งแต่ M0 ถึง M4
_Avoid_: sprint, phase

**Gate**:
ชุดเกณฑ์ทดสอบรับงานท้ายแต่ละช่วง (G0–G4) ช่วงถัดไปเริ่มได้เมื่อ gate ก่อนหน้าผ่าน
_Avoid_: checkpoint, milestone review

**MVP**:
ช่วง M0 ถึง M2 ที่จบเมื่อผ่าน G2 คือเพื่อนใน Discord เห็นเกมพร้อมแถบสตรีม และผู้เล่นอ่านไทยกับอังกฤษบนหน้าอ่าน
_Avoid_: v1, beta
