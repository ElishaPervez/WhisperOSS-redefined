//! Pure audio math. Upload is always 16 kHz mono 16-bit WAV (spec §4):
//! devices capture at their native rate and we downsample locally so the
//! upload is ~3x smaller than v1's.

/// Linear-interpolation downsample to 16 kHz. Adequate for speech-to-text;
/// no external DSP crate needed.
pub fn resample_to_16k(input: &[i16], src_rate: u32) -> Vec<i16> {
    const DST: f64 = 16_000.0;
    if src_rate == 16_000 || input.is_empty() {
        return input.to_vec();
    }
    let ratio = src_rate as f64 / DST;
    let out_len = (input.len() as f64 / ratio).floor() as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let pos = i as f64 * ratio;
        let i0 = pos.floor() as usize;
        let frac = pos - i0 as f64;
        let s0 = input[i0] as f64;
        let s1 = input[(i0 + 1).min(input.len() - 1)] as f64;
        out.push((s0 + (s1 - s0) * frac).round() as i16);
    }
    out
}

/// Keeps the interpolation position between microphone callbacks so splitting
/// the same recording into different callback sizes does not change its speed.
pub struct StreamingResampler {
    source_rate: u32,
    buffered: Vec<i16>,
    next_position: f64,
}

impl StreamingResampler {
    pub fn new() -> Self {
        Self {
            source_rate: 0,
            buffered: Vec::new(),
            next_position: 0.0,
        }
    }

    pub fn push(&mut self, input: &[i16], source_rate: u32) -> Vec<i16> {
        if input.is_empty() {
            return Vec::new();
        }
        if source_rate == 16_000 {
            self.source_rate = source_rate;
            self.buffered.clear();
            self.next_position = 0.0;
            return input.to_vec();
        }
        if self.source_rate != source_rate {
            self.source_rate = source_rate;
            self.buffered.clear();
            self.next_position = 0.0;
        }

        self.buffered.extend_from_slice(input);
        let ratio = source_rate as f64 / 16_000.0;
        let mut output = Vec::with_capacity((input.len() as f64 / ratio).ceil() as usize);
        while self.next_position.floor() as usize + 1 < self.buffered.len() {
            let lower = self.next_position.floor() as usize;
            let fraction = self.next_position - lower as f64;
            let first = self.buffered[lower] as f64;
            let second = self.buffered[lower + 1] as f64;
            output.push((first + (second - first) * fraction).round() as i16);
            self.next_position += ratio;
        }

        let consumed = (self.next_position.floor() as usize).min(self.buffered.len());
        self.buffered.drain(..consumed);
        self.next_position -= consumed as f64;
        output
    }
}

/// Standard 44-byte RIFF/WAVE header + PCM data, built in memory.
/// No temp files anywhere in the pipeline (spec §4).
pub fn encode_wav_mono16(samples: &[i16], sample_rate: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut b = Vec::with_capacity(44 + data_len as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVE");
    b.extend_from_slice(b"fmt ");
    b.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    b.extend_from_slice(&1u16.to_le_bytes()); // PCM
    b.extend_from_slice(&1u16.to_le_bytes()); // mono
    b.extend_from_slice(&sample_rate.to_le_bytes());
    b.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // byte rate
    b.extend_from_slice(&2u16.to_le_bytes()); // block align
    b.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        b.extend_from_slice(&s.to_le_bytes());
    }
    b
}

/// True when the whole recording never rises above ~1% of full scale —
/// the user held the keys but said nothing. Dropped without upload (spec §5).
pub fn is_effectively_silent(samples: &[i16]) -> bool {
    const FLOOR: u16 = 330;
    !samples.iter().any(|s| s.unsigned_abs() > FLOOR)
}

/// v1's tuned visualizer curve: peak → 0.0..=1.0 with a soft knee.
pub fn normalize_level(peak: i16) -> f64 {
    (((peak as f64) / 7000.0).min(1.0).powf(0.72) * 1.18).min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_16k_is_passthrough() {
        let s = vec![1i16, 2, 3, 4];
        assert_eq!(resample_to_16k(&s, 16_000), s);
    }

    #[test]
    fn resample_48k_thirds_the_length_and_keeps_amplitude() {
        let s = vec![900i16; 4800]; // 100 ms of constant signal at 48 kHz
        let out = resample_to_16k(&s, 48_000);
        assert_eq!(out.len(), 1600);
        assert!(out.iter().all(|&v| v == 900));
    }

    #[test]
    fn resample_empty_is_empty() {
        assert!(resample_to_16k(&[], 48_000).is_empty());
    }

    #[test]
    fn streaming_resample_preserves_timing_across_callback_boundaries() {
        let input = vec![900i16; 4_800];
        let mut resampler = StreamingResampler::new();
        let mut output = resampler.push(&input[..1_701], 48_000);
        output.extend(resampler.push(&input[1_701..3_119], 48_000));
        output.extend(resampler.push(&input[3_119..], 48_000));

        assert_eq!(output.len(), 1_600);
        assert!(output.iter().all(|&sample| sample == 900));
    }

    #[test]
    fn wav_header_is_valid_for_16k_mono() {
        let wav = encode_wav_mono16(&[0i16, 1000, -1000], 16_000);
        assert_eq!(wav.len(), 44 + 6);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(u16::from_le_bytes([wav[22], wav[23]]), 1); // mono
        assert_eq!(u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]), 16_000);
        assert_eq!(u16::from_le_bytes([wav[34], wav[35]]), 16); // bits/sample
        assert_eq!(u32::from_le_bytes([wav[40], wav[41], wav[42], wav[43]]), 6); // data bytes
        assert_eq!(i16::from_le_bytes([wav[46], wav[47]]), 1000);
    }

    #[test]
    fn silence_detection() {
        assert!(is_effectively_silent(&[]));
        assert!(is_effectively_silent(&vec![50i16; 16_000]));
        let mut speech = vec![50i16; 16_000];
        speech[8000] = 5000;
        assert!(!is_effectively_silent(&speech));
    }

    #[test]
    fn level_normalization_bounds() {
        assert_eq!(normalize_level(0), 0.0);
        assert!(normalize_level(700) > 0.1);
        assert!((normalize_level(i16::MAX) - 1.0).abs() < 1e-9);
    }
}
