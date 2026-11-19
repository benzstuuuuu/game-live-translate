//! Card audio for the audio path (FR-AUD-01, 08): which CoreAudio input is
//! the card, which of its formats to read, and turning what it gives (48 kHz,
//! f32 or i16, one or more channels) into the 16 kHz s16le mono that Live
//! Translate takes, in 100 ms chunks. No I/O; `crate::card_audio` reads the
//! card.

use super::live::{CHUNK_BYTES, SAMPLE_RATE};
use std::f32::consts::PI;

/// Microphone permission as macOS reports it (`AVAuthorizationStatus`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    /// Not asked yet: macOS asks when the card is first opened.
    NotAsked,
    Granted,
    /// Turned off in System Settings, or not allowed on this Mac.
    Denied,
}

impl Permission {
    /// From `AVCaptureDevice authorizationStatusForMediaType:`.
    pub fn from_av(status: isize) -> Self {
        match status {
            3 => Self::Granted,
            // 1 restricted (not allowed on this Mac), 2 denied.
            1 | 2 => Self::Denied,
            _ => Self::NotAsked,
        }
    }
}

/// The input that is the card: the same name ignoring case and spaces at
/// the ends (as OBS's devices are matched), else the only one whose name
/// holds it.
pub fn pick_device(names: &[String], device: &str) -> Option<usize> {
    let want = device.trim().to_lowercase();
    if want.is_empty() {
        return None;
    }
    let names: Vec<String> = names.iter().map(|n| n.trim().to_lowercase()).collect();
    if let Some(i) = names.iter().position(|n| *n == want) {
        return Some(i);
    }
    let mut holding = (0..names.len()).filter(|&i| names[i].contains(&want));
    match (holding.next(), holding.next()) {
        (Some(i), None) => Some(i),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sample {
    F32,
    I16,
    /// Anything else (i32, u8, …): not read.
    Other,
}

/// One range of formats the input offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Offer {
    pub sample: Sample,
    pub channels: u16,
    pub min_rate: u32,
    pub max_rate: u32,
}

/// Which offer to open and at what rate: 48 kHz when it can, f32 before i16,
/// and only rates that divide down to 16 kHz exactly.
pub fn pick_format(offers: &[Offer]) -> Option<(usize, u32)> {
    RATES.iter().find_map(|&rate| {
        [Sample::F32, Sample::I16].iter().find_map(|&sample| {
            offers
                .iter()
                .position(|o| {
                    o.sample == sample
                        && o.channels > 0
                        && (o.min_rate..=o.max_rate).contains(&rate)
                })
                .map(|i| (i, rate))
        })
    })
}

/// Rates that divide down to 16 kHz exactly, best first.
const RATES: [u32; 4] = [48_000, 16_000, 32_000, 96_000];
/// FIR length per input sample dropped; 49 taps at 48 kHz.
const TAPS_PER_STEP: usize = 16;
/// Where the low-pass starts to cut, under the 8 kHz that 16 kHz can hold.
const CUTOFF_HZ: f32 = 7_200.0;

/// 16 kHz mono out of `rate` (a multiple of 16 kHz) with `channels`
/// interleaved: channels averaged, a low-pass FIR under 8 kHz so nothing
/// folds back, then every `rate / 16 kHz`-th sample kept. It keeps its state
/// between calls, so how the card splits its buffers does not change the
/// result.
pub struct Resampler {
    channels: usize,
    /// Input samples per output sample.
    step: usize,
    taps: Vec<f32>,
    /// The last `taps.len()` mono samples, a ring written at `at`.
    recent: Vec<f32>,
    at: usize,
    /// Mono samples since the last one kept.
    phase: usize,
}

impl Resampler {
    /// None when `rate` is not a multiple of 16 kHz or there are no channels.
    pub fn new(rate: u32, channels: u16) -> Option<Self> {
        if channels == 0 || rate == 0 || !rate.is_multiple_of(SAMPLE_RATE) {
            return None;
        }
        let step = (rate / SAMPLE_RATE) as usize;
        let taps = low_pass(step, rate);
        Some(Self {
            channels: channels.into(),
            step,
            recent: vec![0.0; taps.len()],
            taps,
            at: 0,
            phase: 0,
        })
    }

    /// Interleaved f32 in -1..1 to s16le bytes.
    pub fn f32(&mut self, interleaved: &[f32]) -> Vec<u8> {
        self.run(interleaved, |s| s)
    }

    /// Interleaved i16 to s16le bytes.
    pub fn i16(&mut self, interleaved: &[i16]) -> Vec<u8> {
        self.run(interleaved, |s| f32::from(s) / FULL_SCALE)
    }

    /// Whole frames only; CoreAudio never splits one across buffers.
    fn run<T: Copy>(&mut self, interleaved: &[T], to_f32: impl Fn(T) -> f32) -> Vec<u8> {
        let frames = interleaved.len() / self.channels;
        let mut out = Vec::with_capacity((frames / self.step + 1) * 2);
        for frame in interleaved.chunks_exact(self.channels) {
            let sum: f32 = frame.iter().map(|&s| to_f32(s)).sum();
            self.recent[self.at] = sum / self.channels as f32;
            self.at = (self.at + 1) % self.recent.len();
            self.phase += 1;
            if self.phase == self.step {
                self.phase = 0;
                out.extend_from_slice(&to_i16(self.filtered()).to_le_bytes());
            }
        }
        out
    }

    /// The low-passed sample at the newest position.
    fn filtered(&self) -> f32 {
        let n = self.recent.len();
        let newest = self.at + n - 1;
        self.taps
            .iter()
            .enumerate()
            .map(|(k, t)| t * self.recent[(newest - k) % n])
            .sum()
    }
}

const FULL_SCALE: f32 = 32_768.0;

fn to_i16(x: f32) -> i16 {
    (x * FULL_SCALE)
        .round()
        .clamp(-FULL_SCALE, FULL_SCALE - 1.0) as i16
}

/// Windowed-sinc (Hamming) low-pass for `rate`, gain 1 at 0 Hz. At 16 kHz
/// there is nothing to cut, so the one tap passes samples as they are.
fn low_pass(step: usize, rate: u32) -> Vec<f32> {
    if step == 1 {
        return vec![1.0];
    }
    let n = TAPS_PER_STEP * step + 1;
    let fc = CUTOFF_HZ / rate as f32;
    let mid = (n - 1) as f32 / 2.0;
    let raw: Vec<f32> = (0..n)
        .map(|i| {
            let x = i as f32 - mid;
            let sinc = if x == 0.0 {
                2.0 * fc
            } else {
                (2.0 * PI * fc * x).sin() / (PI * x)
            };
            let window = 0.54 - 0.46 * (2.0 * PI * i as f32 / (n - 1) as f32).cos();
            sinc * window
        })
        .collect();
    let gain: f32 = raw.iter().sum();
    raw.iter().map(|t| t / gain).collect()
}

/// Collects bytes and gives them back in 100 ms chunks.
#[derive(Debug, Default)]
pub struct Chunks {
    pending: Vec<u8>,
}

impl Chunks {
    pub fn push(&mut self, bytes: &[u8]) {
        self.pending.extend_from_slice(bytes);
    }

    /// The oldest whole chunk, if there is one yet.
    pub fn pop(&mut self) -> Option<Vec<u8>> {
        (self.pending.len() >= CHUNK_BYTES).then(|| self.pending.drain(..CHUNK_BYTES).collect())
    }
}

/// 100 ms of silence: what goes to Live Translate while the card is away,
/// so the session stays open.
pub fn silence() -> Vec<u8> {
    vec![0; CHUNK_BYTES]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn samples(bytes: &[u8]) -> Vec<i16> {
        bytes
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect()
    }

    /// `secs` of a sine at `hz`, sampled at `rate`, amplitude `amp`.
    fn sine(hz: f32, rate: u32, amp: f32, secs: f32) -> Vec<f32> {
        let n = (rate as f32 * secs) as usize;
        (0..n)
            .map(|i| amp * (2.0 * PI * hz * i as f32 / rate as f32).sin())
            .collect()
    }

    /// Root mean square after the filter has filled, as a fraction of full scale.
    fn rms(out: &[i16]) -> f32 {
        let settled = &out[64..];
        let sum: f32 = settled
            .iter()
            .map(|&s| (f32::from(s) / 32768.0).powi(2))
            .sum();
        (sum / settled.len() as f32).sqrt()
    }

    fn stereo(mono: &[f32]) -> Vec<f32> {
        mono.iter().flat_map(|&s| [s, s]).collect()
    }

    #[test]
    fn permission_follows_av_authorization_status() {
        assert_eq!(Permission::from_av(0), Permission::NotAsked);
        assert_eq!(Permission::from_av(1), Permission::Denied, "restricted");
        assert_eq!(Permission::from_av(2), Permission::Denied);
        assert_eq!(Permission::from_av(3), Permission::Granted);
        assert_eq!(Permission::from_av(-1), Permission::NotAsked);
    }

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|n| (*n).to_owned()).collect()
    }

    #[test]
    fn card_is_found_by_its_name_ignoring_case_and_spaces() {
        let list = names(&["MacBook Air Microphone", " ugreen 15390 ", "BlackHole 2ch"]);
        assert_eq!(pick_device(&list, "UGREEN 15390"), Some(1));
        assert_eq!(pick_device(&list, "  UGREEN 15390"), Some(1));
    }

    #[test]
    fn exact_name_wins_over_a_longer_one() {
        let list = names(&["UGREEN 15390 Audio", "UGREEN 15390"]);
        assert_eq!(pick_device(&list, "UGREEN 15390"), Some(1));
    }

    #[test]
    fn a_single_name_holding_the_card_name_is_the_card() {
        let list = names(&["MacBook Air Microphone", "USB Audio (UGREEN 15390)"]);
        assert_eq!(pick_device(&list, "UGREEN 15390"), Some(1));
    }

    #[test]
    fn no_card_when_missing_blank_or_unclear() {
        let list = names(&["MacBook Air Microphone", "BlackHole 2ch"]);
        assert_eq!(pick_device(&list, "UGREEN 15390"), None);
        assert_eq!(pick_device(&[], "UGREEN 15390"), None);
        assert_eq!(pick_device(&list, "  "), None, "blank name matches nothing");
        let two = names(&["UGREEN 15390 A", "UGREEN 15390 B"]);
        assert_eq!(pick_device(&two, "UGREEN 15390"), None, "which one?");
    }

    fn offer(sample: Sample, channels: u16, min_rate: u32, max_rate: u32) -> Offer {
        Offer {
            sample,
            channels,
            min_rate,
            max_rate,
        }
    }

    #[test]
    fn format_prefers_48_khz_and_f32() {
        let offers = [
            offer(Sample::I16, 2, 48_000, 48_000),
            offer(Sample::F32, 2, 44_100, 44_100),
            offer(Sample::F32, 2, 8_000, 96_000),
        ];
        assert_eq!(pick_format(&offers), Some((2, 48_000)));
        assert_eq!(pick_format(&offers[..2]), Some((0, 48_000)));
    }

    #[test]
    fn format_takes_another_multiple_of_16_khz_when_48_is_missing() {
        let offers = [
            offer(Sample::F32, 1, 44_100, 44_100),
            offer(Sample::I16, 1, 16_000, 16_000),
        ];
        assert_eq!(pick_format(&offers), Some((1, 16_000)));
        assert_eq!(
            pick_format(&[offer(Sample::F32, 2, 96_000, 96_000)]),
            Some((0, 96_000))
        );
    }

    #[test]
    fn format_none_without_a_readable_rate_or_sample() {
        assert_eq!(pick_format(&[]), None);
        assert_eq!(pick_format(&[offer(Sample::F32, 2, 44_100, 44_100)]), None);
        assert_eq!(
            pick_format(&[offer(Sample::Other, 2, 48_000, 48_000)]),
            None
        );
        assert_eq!(pick_format(&[offer(Sample::F32, 0, 48_000, 48_000)]), None);
    }

    #[test]
    fn resampler_needs_a_multiple_of_16_khz_and_a_channel() {
        assert!(Resampler::new(44_100, 2).is_none());
        assert!(Resampler::new(48_000, 0).is_none());
        assert!(Resampler::new(0, 1).is_none());
        assert!(Resampler::new(48_000, 2).is_some());
        assert!(Resampler::new(16_000, 1).is_some());
    }

    #[test]
    fn one_second_of_48_khz_stereo_f32_is_one_second_of_16_khz_mono() {
        let mut r = Resampler::new(48_000, 2).unwrap();
        let out = r.f32(&stereo(&sine(1_000.0, 48_000, 0.5, 1.0)));
        assert_eq!(out.len(), 16_000 * 2, "16 000 samples of 2 bytes");
    }

    #[test]
    fn speech_band_passes_at_the_same_loudness() {
        // A 1 kHz tone at half scale keeps its level: RMS of a sine = amp/√2.
        let mut r = Resampler::new(48_000, 2).unwrap();
        let out = samples(&r.f32(&stereo(&sine(1_000.0, 48_000, 0.5, 0.5))));
        let level = rms(&out);
        assert!((level - 0.5 / 2f32.sqrt()).abs() < 0.02, "{level}");
    }

    #[test]
    fn sound_above_8_khz_does_not_fold_back() {
        // 12 kHz cannot be held at 16 kHz; without the filter it would come
        // back as a 4 kHz tone at full level.
        let mut r = Resampler::new(48_000, 1).unwrap();
        let out = samples(&r.f32(&sine(12_000.0, 48_000, 0.5, 0.5)));
        assert!(rms(&out) < 0.005, "{}", rms(&out));
    }

    #[test]
    fn channels_are_averaged() {
        // Left and right opposite: nothing left in mono.
        let mut r = Resampler::new(48_000, 2).unwrap();
        let tone = sine(1_000.0, 48_000, 0.5, 0.2);
        let opposite: Vec<f32> = tone.iter().flat_map(|&s| [s, -s]).collect();
        assert!(samples(&r.f32(&opposite)).iter().all(|&s| s == 0));

        // Six channels, one with sound: a sixth of the level.
        let mut r = Resampler::new(48_000, 6).unwrap();
        let six: Vec<f32> = tone.iter().flat_map(|&s| [s, 0., 0., 0., 0., 0.]).collect();
        let level = rms(&samples(&r.f32(&six)));
        assert!((level - 0.5 / 2f32.sqrt() / 6.0).abs() < 0.01, "{level}");
    }

    #[test]
    fn i16_input_gives_the_same_as_f32() {
        let tone = sine(440.0, 48_000, 0.25, 0.1);
        let ints: Vec<i16> = tone.iter().map(|&s| (s * 32768.0).round() as i16).collect();
        let a = samples(&Resampler::new(48_000, 1).unwrap().f32(&tone));
        let b = samples(&Resampler::new(48_000, 1).unwrap().i16(&ints));
        assert_eq!(a.len(), b.len());
        let worst = a
            .iter()
            .zip(&b)
            .map(|(x, y)| (i32::from(*x) - i32::from(*y)).abs())
            .max();
        assert!(worst <= Some(1), "{worst:?}");
    }

    #[test]
    fn steady_level_comes_out_as_s16le() {
        // Full-scale DC through the filter: 32767 once settled, little endian.
        let mut r = Resampler::new(48_000, 2).unwrap();
        let out = r.i16(&[i16::MAX; 2 * 48_00]);
        let last = &out[out.len() - 2..];
        assert_eq!(last, &[0xFF, 0x7F]);
        let mut r = Resampler::new(48_000, 1).unwrap();
        let out = samples(&r.f32(&[-0.25; 4_800]));
        assert_eq!(*out.last().unwrap(), -8192);
    }

    #[test]
    fn too_loud_is_clipped_not_wrapped() {
        let mut r = Resampler::new(16_000, 1).unwrap();
        assert_eq!(samples(&r.f32(&[1.5, -1.5, 1.0])), [32767, -32768, 32767]);
    }

    #[test]
    fn at_16_khz_mono_samples_pass_unchanged() {
        let mut r = Resampler::new(16_000, 1).unwrap();
        let input = [0i16, 1, -1, 12_345, i16::MIN, i16::MAX];
        assert_eq!(samples(&r.i16(&input)), input);
    }

    #[test]
    fn how_the_card_splits_buffers_does_not_change_the_result() {
        let tone = stereo(&sine(700.0, 48_000, 0.3, 0.1));
        let whole = Resampler::new(48_000, 2).unwrap().f32(&tone);
        let mut r = Resampler::new(48_000, 2).unwrap();
        // Buffers of odd sizes that cut the 3-sample steps anywhere.
        let mut pieces = Vec::new();
        let mut at = 0;
        for size in [2usize, 10, 514, 6, 1_000].iter().cycle() {
            if at >= tone.len() {
                break;
            }
            let end = (at + size).min(tone.len());
            pieces.extend(r.f32(&tone[at..end]));
            at = end;
        }
        assert_eq!(pieces, whole);
    }

    #[test]
    fn chunks_come_out_100_ms_at_a_time_in_order() {
        let mut c = Chunks::default();
        assert_eq!(c.pop(), None);
        c.push(&vec![1; CHUNK_BYTES - 1]);
        assert_eq!(c.pop(), None, "not a whole chunk yet");
        c.push(&[2; CHUNK_BYTES + 5]);
        let first = c.pop().unwrap();
        assert_eq!(first.len(), CHUNK_BYTES);
        assert_eq!(first[CHUNK_BYTES - 2..], [1, 2]);
        let second = c.pop().unwrap();
        assert!(second.iter().all(|&b| b == 2));
        assert_eq!(c.pop(), None, "4 bytes wait for more");
    }

    #[test]
    fn silence_is_one_chunk_of_zeros() {
        let s = silence();
        assert_eq!(s.len(), CHUNK_BYTES);
        assert!(s.iter().all(|&b| b == 0));
    }
}
