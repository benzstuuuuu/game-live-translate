//! WAV files for the audio-from-file source (dev and tests): only the format
//! Live Translate takes as it is, 16 kHz 16-bit PCM mono. Pure.

use super::live::SAMPLE_RATE;

const MAKE_ONE: &str = "ทำไฟล์ด้วย say -o x.wav --file-format=WAVE --data-format=LEI16@16000 \"…\" \
     หรือ ffmpeg -i in -ar 16000 -ac 1 -c:a pcm_s16le x.wav";

/// The PCM samples of a 16 kHz 16-bit mono WAV file, or a Thai reason the
/// file is not one.
pub fn pcm(bytes: &[u8]) -> Result<&[u8], String> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(format!("ไม่ใช่ไฟล์ WAV {MAKE_ONE}"));
    }
    let mut format_ok = None;
    let mut at = 12;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let size = u32::from_le_bytes([bytes[at + 4], bytes[at + 5], bytes[at + 6], bytes[at + 7]])
            as usize;
        let body = at + 8;
        let end = body.saturating_add(size).min(bytes.len());
        match id {
            b"fmt " => format_ok = Some(format_is_ok(&bytes[body..end])),
            b"data" => {
                return match format_ok {
                    Some(true) => Ok(&bytes[body..end]),
                    Some(false) => Err(format!("ต้องเป็นเสียง 16 kHz 16-bit mono {MAKE_ONE}")),
                    None => Err(format!("ไฟล์ WAV ไม่มีส่วน fmt {MAKE_ONE}")),
                };
            }
            _ => {}
        }
        // Chunks are padded to an even size.
        at = body.saturating_add(size).saturating_add(size % 2);
    }
    Err(format!("ไฟล์ WAV ไม่มีเสียง {MAKE_ONE}"))
}

/// PCM (1) or extensible (0xFFFE), one channel, 16 kHz, 16 bits.
fn format_is_ok(fmt: &[u8]) -> bool {
    if fmt.len() < 16 {
        return false;
    }
    let u16_at = |i: usize| u16::from_le_bytes([fmt[i], fmt[i + 1]]);
    let rate = u32::from_le_bytes([fmt[4], fmt[5], fmt[6], fmt[7]]);
    matches!(u16_at(0), 1 | 0xFFFE) && u16_at(2) == 1 && rate == SAMPLE_RATE && u16_at(14) == 16
}

/// A 16 kHz 16-bit mono WAV file holding `samples` (tests and dev tools).
pub fn encode(samples: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(44 + samples.len());
    let len = samples.len() as u32;
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    out.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes()); // bytes per second
    out.extend_from_slice(&2u16.to_le_bytes()); // bytes per frame
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(samples);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_fmt(channels: u16, rate: u32, bits: u16) -> Vec<u8> {
        let mut f = encode(&[1, 2, 3, 4]);
        f[22..24].copy_from_slice(&channels.to_le_bytes());
        f[24..28].copy_from_slice(&rate.to_le_bytes());
        f[34..36].copy_from_slice(&bits.to_le_bytes());
        f
    }

    #[test]
    fn reads_back_what_encode_wrote() {
        let samples: Vec<u8> = (0..=255).collect();
        assert_eq!(pcm(&encode(&samples)), Ok(&samples[..]));
    }

    #[test]
    fn skips_other_chunks_with_padding() {
        let plain = encode(&[9, 9]);
        let mut f = plain[..36].to_vec();
        f.extend_from_slice(b"LIST");
        f.extend_from_slice(&3u32.to_le_bytes());
        f.extend_from_slice(&[0, 0, 0, 0]); // 3 bytes + 1 pad
        f.extend_from_slice(&plain[36..]);
        assert_eq!(pcm(&f), Ok(&[9u8, 9][..]));
    }

    #[test]
    fn short_data_chunk_reads_what_is_there() {
        let mut f = encode(&[1, 2, 3, 4]);
        f.truncate(f.len() - 2);
        assert_eq!(pcm(&f), Ok(&[1u8, 2][..]));
    }

    #[test]
    fn refuses_other_formats_saying_how_to_make_one() {
        for f in [
            with_fmt(2, 16_000, 16),
            with_fmt(1, 48_000, 16),
            with_fmt(1, 16_000, 24),
        ] {
            let e = pcm(&f).unwrap_err();
            assert!(e.contains("16 kHz 16-bit mono"), "{e}");
            assert!(e.contains("LEI16@16000"), "{e}");
        }
    }

    #[test]
    fn refuses_what_is_not_wav() {
        assert!(pcm(b"").unwrap_err().contains("ไม่ใช่ไฟล์ WAV"));
        assert!(pcm(b"RIFF\0\0\0\0AIFF")
            .unwrap_err()
            .contains("ไม่ใช่ไฟล์ WAV"));
        let no_data = &encode(&[])[..36];
        assert!(pcm(no_data).unwrap_err().contains("ไม่มีเสียง"));
        let mut no_fmt = b"RIFF\0\0\0\0WAVE".to_vec();
        no_fmt.extend_from_slice(&encode(&[1, 2])[36..]);
        assert!(pcm(&no_fmt).unwrap_err().contains("fmt"));
        let mut tiny_fmt = encode(&[1, 2]);
        tiny_fmt[16..20].copy_from_slice(&4u32.to_le_bytes());
        assert!(pcm(&tiny_fmt).is_err());
    }
}
