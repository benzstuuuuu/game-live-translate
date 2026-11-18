# สเปกจริงของการ์ด UGREEN กับ PS5 (issue #2)

ค้นเมื่อ 2026-11-18 วันที่การ์ดมาถึง อ้างอิง SRS หัวข้อ 2 (ข้อจำกัด Capture card), NFR-19, FR-CAP-09, FR-CAP-10

ระดับความเชื่อถือที่ใช้ในเอกสารนี้

- **ยืนยันแล้ว**: มาจากเอกสารของเจ้าของเรื่องโดยตรง (Sony, Apple, FFmpeg) และอ่านหน้าเว็บได้จริง
- **เอกสารผู้ผลิตผ่านตัวกลาง**: เนื้อหาคู่มือหรือหน้าขายของ UGREEN ที่ได้มาจากเว็บตัวกลาง (manuals.plus, Newegg, ร้านค้า) ไม่ใช่จาก ugreen.com
- **ยังไม่ยืนยัน**: ไม่มีแหล่งที่เชื่อได้ ต้องลองกับของจริง

> ข้อจำกัดของการค้นครั้งนี้: หน้า UGREEN ทางการของรุ่น 25773 (`eu.ugreen.com/*/products/25773-amz`) ตอบ 404 แล้ว ส่วน store ของ ugreen.com/us/uk/eu ค้นหา "25773" ไม่เจอสินค้า manuals.plus, Newegg, Amazon, ManualsLib และ Shopee บล็อกการ fetch อัตโนมัติ ข้อมูลจากหน้าเหล่านี้จึงมาจากข้อความที่ search index เก็บไว้ ไม่ได้เปิดอ่านทั้งหน้าเอง ตัวเลขของการ์ดทุกตัวจึงต้องตรวจซ้ำกับของจริงตาม checklist ท้ายเอกสาร

## สรุปคำตอบ

| คำถาม | คำตอบ | ความเชื่อถือ |
|---|---|---|
| รุ่นอะไร | น่าจะเป็น **UGREEN CM726 / P/N 25773** ("2K@30Hz HDMI Capture Card") ไม่ใช่ 25173 (CM700 ซึ่งจับภาพ 4K60) | เอกสารผู้ผลิตผ่านตัวกลาง ต้องดูฉลากบนตัวการ์ดหรือกล่อง |
| โหมดจับภาพทาง USB | สูงสุด **1080p60 หรือ 1440p(2K)30** ไม่จับภาพ 4K | เอกสารผู้ผลิตผ่านตัวกลาง |
| Codec | **MJPEG และ YUY2** ยังไม่รู้ว่าโหมดไหนใช้ format ไหน | เอกสารผู้ผลิตผ่านตัวกลาง |
| Input เมื่อ PS5 ส่ง 4K | 4K30 4:4:4 ได้ ส่วน 4K60 ได้เฉพาะ **4:2:0** (หน้าขายบางหน้าว่าได้แค่ 4K30) | ข้อมูลขัดกัน ต้องลองกับของจริง |
| UVC/UAC บน macOS | plug-and-play ไม่ต้องลง driver ใช้กับ QuickTime/OBS ได้ เสียง HDMI มาทาง USB (24-bit/48 kHz) | เอกสารผู้ผลิตผ่านตัวกลาง |
| Loop-out | 4K30 4:4:4 หรือ 4K60 4:2:0 ทำงานพร้อมกับการจับภาพทาง USB | เอกสารผู้ผลิตผ่านตัวกลาง ข้อมูลขัดกัน |
| HDR / VRR / 120 Hz | ไม่มีหน้าขายของ 25773 หน้าไหนอ้างว่ารองรับ ให้ถือว่า **ไม่รองรับ** | ยังไม่ยืนยัน |
| PS5 | ปิด HDCP ตั้ง Resolution 1080p ปิด HDR, VRR และ 120 Hz | ยืนยันแล้ว (Sony, Elgato) |
| ความหน่วง | ไม่มีผลวัดที่เชื่อถือได้ ร้านค้ารายหนึ่งเขียนว่า "ประมาณ 42 ms" | ยังไม่ยืนยัน ต้องวัดเอง |

## 1. ระบุรุ่น

- ชื่อสินค้าบน Shopee คือ "HDMI Input & Loop Out, USB-C Capture, 2K@30Hz MJPEG, loop out 4K@60Hz 4:2:0" ตรงกับคำอธิบายรุ่น **25773** บน Newegg ที่ว่ารับ input และ loop-out ได้ 4K@30Hz (4:4:4) และ "max 4K@60Hz(4:2:0) Input and 4K@60Hz Loop-out(4:2:0)" ([Newegg 25773](https://www.newegg.com/elgato-25773/p/2XW-010J-000G8)) และตรงกับหน้าร้านที่ระบุ "Model Number: CM726 (25773)", "Input Resolution: Up to dual 4K@30Hz or 4K@60Hz (4:2:0)" และมีพอร์ต HDMI 2 ช่อง (in + loop-out) ([Rapidtech CM726](https://rapidtech.co.ke/?p=56366))
- คู่มือของรุ่นนี้บน manuals.plus ใช้ชื่อ "UGREEN 2K@30Hz HDMI Capture Card" รุ่น CM726 / P/N 25773 ([manuals.plus B0DGXJS6BF](https://manuals.plus/asin/B0DGXJS6BF), [manuals.plus 25773](https://manuals.plus/m/1bab22ae47fc82d18034eed514ae5020ad749f6396827719253d21702c065726))
- อย่าสับสนกับ **25173 (CM700)** ซึ่งจับภาพ 4K60/2K144/1080p240 และไม่รองรับ HDR/VRR ทาง USB ([UGREEN UK 25173](https://uk.ugreen.com/products/25173)) และ **25772 (CM725)** ซึ่งรับ input ทาง USB-C ไม่ใช่ HDMI ([manuals.plus CM725](https://manuals.plus/ae/1005007037878396))
- **ยังไม่ยืนยัน**: หน้า UGREEN EU ของ 25773 ที่ search index เคยเก็บไว้เขียนว่า "Video Capture Resolution 4K@30Hz, 2K@30Hz" และ "HDMI 4K@30Hz Passthrough and Loop-out" (ไม่มี 4K60) แต่วันนี้หน้านั้นตอบ 404 ส่วน [Newegg.ca 25773](https://www.newegg.ca/grey-ugreen-25773/p/1B9-05RN-00049) ก็เขียนว่า loop-out แค่ 4K30 เป็นไปได้ว่ามีหลาย hardware revision ภายใต้ P/N เดียวกัน หรือหน้าเก่ายังไม่ได้แก้ข้อมูล ต้องดูฉลากบนการ์ดและทดสอบ 4K60 จริง

## 2. การจับภาพทาง USB

- **ความละเอียดและ fps**: คู่มือให้ HDMI IN และ HDMI OUT สูงสุด 4K@30Hz และ USB สูงสุด 2K@30Hz ([manuals.plus B0DGXJS6BF](https://manuals.plus/asin/B0DGXJS6BF)) หน้าร้านเขียนว่า "Capture Resolution: Up to 1080p@60Hz or 1440p@30Hz" ([Rapidtech CM726](https://rapidtech.co.ke/?p=56366)) ส่วน Newegg เขียนว่าไม่รองรับการจับภาพ 4K ([Newegg.ca 25773](https://www.newegg.ca/grey-ugreen-25773/p/1B9-05RN-00049))
- **Codec**: "Video Format: MJPEG, YUY2" ([Rapidtech CM726](https://rapidtech.co.ke/?p=56366)) แต่หน้าขายไม่ได้บอกว่าแต่ละความละเอียดใช้ format อะไร ผู้รีวิวบน desertcart บ่นว่าภาพที่จับที่ 1440p แตกเป็นบล็อก แต่ภาพ loop-out ปกติ ([desertcart listing](https://www.desertcart.co.th/products/685062205-video-capture-card-hdmi-to-usb-c-streaming-device-2k)) อาการแบบนี้เข้ากับการบีบอัด MJPEG ที่ 1440p30 (นี่เป็นการตีความ ยังไม่ได้ตรวจ)
- **ชิปข้างใน (ยังไม่ยืนยัน)**: สเปก "รับ 4K30, ส่งออก USB 1080p60, YUV/MJPEG, มี HDMI loop-out" ตรงกับชิปตระกูล MacroSilicon MS2130/MS2131 ซึ่งรับ HDMI 1.4 สูงสุด 4K30 และรองรับ USB 3.0 + UVC 1.5 ([หน้าผู้จัดจำหน่าย MS2130](https://macrosilicon-cf-pages.pages.dev/products/ms2130.html) ซึ่งไม่ใช่เว็บ MacroSilicon เอง) ตัว MS2130 ส่งออกได้สูงสุด 1080p60 แบบ YUV หรือ JPEG ส่วน MS2131 คือ MS2130 ที่เพิ่ม HDMI Tx สำหรับ loop-out ([CNX Software](https://www.cnx-software.com/2022/11/07/ms2130-based-4k-hdmi-to-usb-3-0-video-capture-dongle-sells-for-19/)) UGREEN ไม่ได้ระบุชิป ดู Vendor ID ได้จาก checklist (usb.ids บันทึก MacroSilicon เป็น `534d` ([linux-usb.org usb.ids](http://www.linux-usb.org/usb.ids)))
- **ผลต่อ Yokoten**: สำหรับ OCR ควรใช้ **1080p60 แบบ YUY2** ถ้าการ์ดมีโหมดนี้ เพราะไม่มี artifact จาก JPEG รอบตัวอักษร subtitle แบนด์วิดท์ 1080p60 YUY2 ประมาณ 1920×1080×2 B×60 ≈ 2 Gbit/s ซึ่งพอดีกับ USB 3.0 (5 Gbit/s) แต่จะเกินถ้าการ์ดตกไปเป็น USB 2.0 (480 Mbit/s) ในกรณีนั้นจะเหลือแค่ MJPEG

## 3. UVC/UAC บน macOS

- คู่มือ UGREEN เขียนว่า "Plug and Play with no driver installation or external power supply required" รองรับ OBS Studio, QuickTime และอื่น ๆ และใน OBS อุปกรณ์จะขึ้นชื่อว่า **"UGREEN 25773"** รองรับ iPadOS 17 ขึ้นไป แต่ไม่รองรับ iPhone ([manuals.plus B0DGXJS6BF](https://manuals.plus/asin/B0DGXJS6BF)) หน้าร้านระบุ OS ที่รองรับว่า "Windows 8.1/10/11, macOS, Linux, Android, iPadOS 17 or later" ([Rapidtech CM726](https://rapidtech.co.ke/?p=56366))
- ฝั่ง Apple ใน macOS 14 ขึ้นไป AVFoundation มี `AVCaptureDevice.DeviceType.external` สำหรับกล้องภายนอก และเอกสารอธิบายว่า external device บน iPad คือ "those that conform to the UVC (USB Video Class) specification" ([Apple: external](https://developer.apple.com/documentation/avfoundation/avcapturedevice/devicetype-swift.struct/external)) แต่ละ format ที่อุปกรณ์รองรับอ่านได้จาก `AVCaptureDevice.formats` และ `videoSupportedFrameRateRanges` ([Apple: formats](https://developer.apple.com/documentation/avfoundation/avcapturedevice/formats), [Apple: videoSupportedFrameRateRanges](https://developer.apple.com/documentation/avfoundation/avcapturedevice/format/videosupportedframerateranges)) ส่วนเสียงหาได้ด้วย device type `.microphone` (macOS 14 ขึ้นไป) ([Apple: microphone](https://developer.apple.com/documentation/avfoundation/avcapturedevice/devicetype-swift.struct/microphone)) Apple ไม่ได้เขียนตรง ๆ ว่า macOS มี UVC/UAC class driver ในตัว ข้อนี้รู้กันโดยทั่วไปแต่ต้องยืนยันด้วยการเสียบใช้จริง
- **เสียง**: การ์ดจับเสียง HDMI ที่ 24-bit/48 kHz ([manuals.plus B0DGXJS6BF](https://manuals.plus/asin/B0DGXJS6BF), [Rapidtech CM726](https://rapidtech.co.ke/?p=56366)) ข้อควรระวังฝั่ง PS5 (ยืนยันจากผู้ผลิตการ์ดรายอื่น): ถ้าเสียบหูฟังเข้า controller PS5 จะส่งเสียงทั้งหมดไปที่ controller และ capture card จะไม่ได้รับเสียงเลย ([Elgato 4K S + PS5](https://www.elgato.com/us/en/explorer/products/capture/how-to-set-up-game-capture-4k-s-with-playstation-5-and-playstation-5-pro/)) ข้อนี้กระทบการถอดเสียงจาก capture device ถ้าผู้ใช้เสียบหูฟังกับจอย Yokoten จะไม่ได้ยินเสียงเกม

## 4. Loop-out

- หน้าขายอ้างว่าได้ 4K30 4:4:4 หรือ 4K60 4:2:0 ([Newegg 25773](https://www.newegg.com/elgato-25773/p/2XW-010J-000G8)) แต่หน้าอื่นบอกแค่ 4K30 (ดูหัวข้อ 1) Newegg ยังเขียนไว้ด้วยว่าแหล่งสัญญาณ 4K60 อาจต้องตั้งเป็น 4K30 (4:4:4)
- **ใช้พร้อมการจับภาพ USB ได้**: คู่มือเขียนว่า loop-out มีไว้ "for real-time monitoring" ระหว่างจับภาพ ([manuals.plus B0DGXJS6BF](https://manuals.plus/asin/B0DGXJS6BF))
- **HDR / VRR / 120 Hz**: ไม่มีหน้าไหนของ 25773 อ้างว่ารองรับ (ยังไม่ยืนยัน) ขณะที่การ์ดรุ่นแพงกว่าอย่าง 25173 ยังเขียนไว้ชัดว่าไม่มี HDR/VRR ทาง USB ([UGREEN UK 25173](https://uk.ugreen.com/products/25173)) ให้ถือว่าไม่มีทั้งสามอย่าง
- **ข้อควรรู้**: PS5 เลือกความละเอียดจาก EDID ของอุปกรณ์ปลายทาง ([Sony: resolution guide](https://www.playstation.com/en-us/support/hardware/ps5-4k-resolution-guide/) "detects your TV's capabilities and automatically selects the best available resolution") เมื่อต่อผ่านการ์ด PS5 จะเห็น EDID ที่การ์ดส่งให้ ซึ่งอาจเป็น EDID ของการ์ดเองหรือของ TV ที่ต่อ loop-out อยู่ ยังไม่รู้ว่าการ์ดตัวนี้ทำแบบไหน (ยังไม่ยืนยัน) ถ้าส่ง EDID ของ TV ไป PS5 อาจส่ง 4K60 มาและการ์ดอาจจับภาพไม่ได้ ดูได้จากหน้า "Information for the Connected HDMI Device" ใน PS5

## 5. ตั้งค่า PS5

ยืนยันจากเอกสาร Sony และผู้ผลิตการ์ด:

1. **ปิด HDCP ระหว่างที่ PS5 ต่อเข้า TV ตรง** (ยังไม่ผ่านการ์ด) ด้วย Settings > System > HDMI แล้วปิด HDCP จากนั้นค่อยต่อการ์ด ([Elgato 4K S + PS5](https://www.elgato.com/us/en/explorer/products/capture/how-to-set-up-game-capture-4k-s-with-playstation-5-and-playstation-5-pro/)) PS5 เปิด HDCP ไว้เป็นค่าเริ่มต้น และถ้ายังเปิดอยู่ภาพที่จับจะดำ ([Elgato help: Neo + PS5](https://help.elgato.com/hc/en-us/articles/25653778851853-Elgato-Game-Capture-Neo-PlayStation-5-Setup)) คู่มือ UGREEN ก็บอกให้ผู้ใช้ PS4/PS5 ปิด HDCP ก่อน ([manuals.plus B0DGXJS6BF](https://manuals.plus/asin/B0DGXJS6BF)) ไม่พบหน้า Sony สำหรับ PS5 ที่อธิบายตัวเลือกนี้โดยตรง ให้ดูชื่อเมนูจริงบนเครื่อง ผลข้างเคียงที่แหล่งรองหลายแหล่งพูดถึงคือแอปสตรีมหนังอย่าง Netflix จะใช้ไม่ได้ระหว่างที่ปิด HDCP
2. ไปที่ **Settings > Screen and Video > Video Output** ([Sony: resolution guide](https://www.playstation.com/en-us/support/hardware/ps5-4k-resolution-guide/)) แล้วตั้งค่าตามนี้
   - **Resolution = 1080p** (แนะนำสำหรับ MVP) เพราะการ์ดจับภาพได้สูงสุด 1080p60 การส่ง 1080p60 จึงไม่ต้อง scale และได้ fps เต็ม
   - **120 Hz Output = Off, VRR = Off** (Sony: ปิด ALLM ให้หมดจริงต้องปิด VRR ด้วย)
   - **HDR = Off** (การ์ดไม่น่าจะรองรับ ยังไม่ยืนยัน)
   - ถ้าจะลอง 4K60 ผ่าน loop-out ให้ตั้ง **Video Transfer Rate = -2** ซึ่ง Sony ระบุว่า "When displaying 4K content at 50/60 Hz, YUV420 color format is used" และเป็น chroma เดียวที่การ์ดอ้างว่ารับได้ที่ 4K60 ([Sony: resolution guide](https://www.playstation.com/en-us/support/hardware/ps5-4k-resolution-guide/))
3. Sony แนะนำให้ถอด "game capture devices" ออกก่อนเมื่อแก้ปัญหาภาพ ([Sony: picture & sound issues](https://www.playstation.com/en-us/support/hardware/ps5-picture-sound-issues/)) ถ้าภาพไม่ขึ้นให้ลองต่อตรงก่อนเสมอ

## 6. ความหน่วง (NFR-19 ≤ 100 ms)

- **ไม่พบผลวัดที่เชื่อถือได้ของ 25773** UGREEN เขียนแค่ "low latency" หน้าร้านรายหนึ่งเขียนว่า "Latency: Approximately 42 ms" แต่ไม่บอกที่มา ([Rapidtech CM726](https://rapidtech.co.ke/?p=56366))
- ข้อมูลเทียบเคียง (ไม่ใช่การ์ดตัวนี้): ผู้ซื้อ dongle MS2130 รายงานว่าหน่วง "around 6 to 11 frames" ([CNX Software](https://www.cnx-software.com/2022/11/07/ms2130-based-4k-hdmi-to-usb-3-0-video-capture-dongle-sells-for-19/)) ซึ่งที่ 60 fps เท่ากับประมาณ 100–183 ms แบบ glass-to-glass รวมซอฟต์แวร์และจอ
- **Loop-out** เป็น passthrough จึงไม่น่ามีความหน่วงที่รู้สึกได้ (ยังไม่ยืนยัน) ทางถอยที่ต่อ loop-out เข้า TV ตรงจึงไม่น่ามีปัญหาเรื่อง NFR-19
- **Topology MVP** (การ์ด → USB → viewer บน Mac → USB-C-to-HDMI → TV) มีความหน่วงจากการจับ + การ decode MJPEG + viewer + compositor ของ macOS + การแสดงผลออกจอภายนอก และ TV อาจไม่เข้า Game Mode เมื่อสัญญาณมาจาก Mac **ความเสี่ยงที่จะเกิน 100 ms สูง** ต้องวัดใน M0 ก่อนผูกกับ topology นี้

## 7. ข้อเท็จจริงฝั่ง Mac ที่เกี่ยวกับ topology

- MacBook Air M2 มี Thunderbolt / USB 4 สองช่องกับ MagSafe 3 และต่อจอภายนอกได้ 1 จอ ([Apple: MacBook Air M2 tech specs](https://support.apple.com/en-us/111867)) ใน topology MVP ช่องหนึ่งใช้กับการ์ด อีกช่องใช้กับ adapter HDMI ไป TV ส่วนการชาร์จต้องใช้ MagSafe พอร์ตจึงพอดีโดยไม่ต้องมี hub ห้ามต่อการ์ดผ่าน hub ที่เป็น USB 2.0

## Checklist ตรวจกับของจริง

ทำตามลำดับและจดผลลงคอมเมนต์ของ issue หรือ ticket M0

1. **ฉลาก**: ดู P/N และรุ่นบนตัวการ์ดหรือกล่องว่าเป็น 25773 / CM726 ไหม และจด hardware revision ถ้ามี
2. **USB enumeration**: รัน `system_profiler SPUSBDataType` (ถ้าว่างบน macOS รุ่นใหม่ ให้ลอง `system_profiler SPUSBHostDataType`) หรือ `ioreg -p IOUSB -l -w0 | grep -i -B2 -A20 ugreen` แล้วจดชื่ออุปกรณ์, Vendor ID / Product ID (ถ้า Vendor ID เป็น `0x534d` แสดงว่าเป็นชิป MacroSilicon) และ **Speed ต้องเป็น 5 Gb/s (SuperSpeed)** ถ้าเห็น 480 Mb/s ให้เปลี่ยนสายหรือพอร์ต
3. **กล้องและเสียง**: รัน `system_profiler SPCameraDataType` ต้องเห็น "UGREEN 25773" แบบไม่ลง driver รัน `system_profiler SPAudioDataType` และเปิด Audio MIDI Setup ต้องเห็น input ของการ์ดที่ 48 kHz
4. **ffmpeg**: รัน `ffmpeg -f avfoundation -list_devices true -i ""` แล้วจด index และชื่อของ video และ audio device ([FFmpeg avfoundation](https://ffmpeg.org/ffmpeg-devices.html#avfoundation))
5. **Format และ fps ที่การ์ดประกาศ**: รัน `ffmpeg -f avfoundation -video_size 1x1 -framerate 1 -i "UGREEN 25773:none"` เมื่อขอโหมดที่ไม่มี ffmpeg จะพิมพ์รายการ "Supported modes" ออกมา และใช้ `-pixel_format` ที่ไม่มีอยู่เพื่อให้พิมพ์ pixel format ที่ใช้ได้ (ตามเอกสาร FFmpeg) วิธีที่ละเอียดกว่าคือเขียน Swift script สั้น ๆ วนอ่าน `AVCaptureDevice.formats` แล้วพิมพ์ FourCC ของ `formatDescription.mediaSubType` (`yuvs`/`2vuy` = YUY2/UYVY, `dmb1` = MJPEG), ขนาด และ `videoSupportedFrameRateRanges` ทั้งหมด
6. **fps ที่ได้จริง**: รัน `ffmpeg -f avfoundation -video_size 1920x1080 -framerate 60 -pixel_format yuyv422 -i "UGREEN 25773:none" -t 15 -f null -` แล้วดูค่า `fps=` ทำซ้ำที่ 2560x1440@30 และ 1920x1080@60 แบบ MJPEG ถ้ามี จดว่า 1080p60 ได้ 60 จริงไหม
7. **ตารางทดลองค่า PS5** (ปิด HDCP แล้ว): ลอง (a) 1080p, (b) 1440p, (c) 2160p + Transfer Rate Auto, (d) 2160p + Transfer Rate -2 และ (e) HDR On ในแต่ละแบบจดว่า (1) ภาพ capture ขึ้นไหม, (2) loop-out ขึ้นไหม, (3) ค่าใน PS5 > Screen and Video > Video Output > Current Video Output Signal / Information for the Connected HDMI Device ทำทั้งแบบมี TV ต่อ loop-out และไม่มี เพื่อดูว่า EDID มาจากไหน
8. **ลองเปิด HDCP**: ตั้ง HDCP = On แล้วยืนยันว่า capture ดำ (เพื่อให้ข้อความแจ้งเตือนใน Yokoten ถูกต้อง)
9. **เสียง**: ได้ยินเสียงเกมผ่านการ์ดใน QuickTime/OBS แล้วเสียบหูฟังเข้า DualSense เพื่อยืนยันว่าเสียงหายจาก capture
10. **ความหน่วง (NFR-19)**: ต่อ loop-out เข้า TV (ภาพอ้างอิงที่ไม่ผ่าน Mac) และเปิด viewer บนจอ Mac แล้วถ่ายทั้งสองจอพร้อมกันด้วยโหมด slow-mo 240 fps ของ iPhone ขณะมีภาพเคลื่อนไหวเร็ว (เช่นนาฬิกาจับเวลาหรือตัวนับเฟรมในเกม) นับเฟรมที่ต่างกันอย่างน้อย 10 ครั้งแล้วหาค่ามัธยฐาน จากนั้นวัดซ้ำแบบ topology MVP จริง (viewer เต็มจอบน TV ผ่าน adapter) เทียบกับ PS5 ต่อ TV ตรง โดยเปิด Game Mode ของ TV ทั้งสองแบบ ทำแยกตาม viewer ที่ issue #3 จะเลือก
11. **ความเสถียร**: เล่นต่อเนื่อง 30 นาที ดูว่าภาพหลุด, fps ตก หรือการ์ดร้อนผิดปกติไหม และดูว่าหลัง Mac sleep/wake การ์ดยังอยู่ไหม
