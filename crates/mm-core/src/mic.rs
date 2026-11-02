//! Pure microphone downmixing, streaming resampling and Muse realtime framing.

pub const OUTPUT_RATE: u32 = 24_000;
pub const FRAME_SAMPLES: usize = 1_920;
pub const FRAME_BYTES: usize = FRAME_SAMPLES * 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MicError {
    InvalidRate,
    InvalidChannels,
}

impl std::fmt::Display for MicError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidRate => "microphone sample rate must be nonzero",
            Self::InvalidChannels => "microphone channel count must be nonzero",
        })
    }
}

impl std::error::Error for MicError {}

/// Accepts arbitrary chunks, including chunks splitting an interleaved channel group.
/// Linear interpolation uses the previous and current mono samples, with the first
/// sample extended backwards. Output times are at the ends of sample intervals;
/// integer phase preserves timing without accumulating floating-point drift.
#[derive(Debug)]
pub struct Framer {
    input_rate: u32,
    input_channels: u16,
    channel_sum: f64,
    channel_count: u16,
    previous: Option<f64>,
    phase: u64,
    frame: Vec<u8>,
    finished: bool,
}

impl Framer {
    pub fn new(input_rate: u32, input_channels: u16) -> Result<Self, MicError> {
        if input_rate == 0 {
            return Err(MicError::InvalidRate);
        }
        if input_channels == 0 {
            return Err(MicError::InvalidChannels);
        }
        Ok(Self {
            input_rate,
            input_channels,
            channel_sum: 0.0,
            channel_count: 0,
            previous: None,
            phase: 0,
            frame: Vec::with_capacity(FRAME_BYTES),
            finished: false,
        })
    }

    /// Returns every complete 80 ms PCM16LE frame. NaN input is silence;
    /// infinities are treated as signed full scale before downmixing.
    /// Calls after `finish` return no frames.
    pub fn push(&mut self, samples: &[f32]) -> Vec<Vec<u8>> {
        let mut frames = Vec::new();
        if self.finished {
            return frames;
        }
        for &sample in samples {
            self.channel_sum += if sample.is_nan() {
                0.0
            } else if sample.is_infinite() {
                f64::from(sample.signum())
            } else {
                f64::from(sample)
            };
            self.channel_count += 1;
            if self.channel_count != self.input_channels {
                continue;
            }
            let mono = self.channel_sum / f64::from(self.input_channels);
            self.channel_sum = 0.0;
            self.channel_count = 0;
            let previous = self.previous.unwrap_or(mono);
            self.phase += u64::from(OUTPUT_RATE);
            while self.phase >= u64::from(self.input_rate) {
                self.phase -= u64::from(self.input_rate);
                let fraction = 1.0 - self.phase as f64 / f64::from(OUTPUT_RATE);
                let value = (previous + (mono - previous) * fraction).clamp(-1.0, 1.0);
                let pcm = (value * f64::from(i16::MAX)).round() as i16;
                self.frame.extend_from_slice(&pcm.to_le_bytes());
                if self.frame.len() == FRAME_BYTES {
                    frames.push(std::mem::replace(
                        &mut self.frame,
                        Vec::with_capacity(FRAME_BYTES),
                    ));
                }
            }
            self.previous = Some(mono);
        }
        frames
    }

    /// Ends the stream and pads its last partial output frame with silence.
    /// An incomplete input channel group and sub-output-sample duration are
    /// discarded. Repeated calls return None; create a new framer to restart.
    pub fn finish(&mut self) -> Option<Vec<u8>> {
        self.finished = true;
        if self.frame.is_empty() {
            return None;
        }
        self.frame.resize(FRAME_BYTES, 0);
        Some(std::mem::take(&mut self.frame))
    }
}

/// RMS of signed PCM16LE bytes in 0..=1. Empty input is silence; an odd
/// trailing byte is ignored. Normalize by 32768 to include i16::MIN safely.
pub fn level(frame: &[u8]) -> f32 {
    let mut squares = 0.0_f64;
    let mut count = 0_usize;
    for bytes in frame.as_chunks::<2>().0 {
        let sample = f64::from(i16::from_le_bytes(*bytes)) / 32_768.0;
        squares += sample * sample;
        count += 1;
    }
    if count == 0 {
        0.0
    } else {
        (squares / count as f64).sqrt() as f32
    }
}

/// The most `StreamGain` lifts a quiet microphone: +20 dB.
pub const MAX_GAIN: f64 = 10.0;

/// The utterance gain while the audio streams (speech recognition gets each frame as it is
/// captured, so the words can show while the person speaks): the gain follows the loudest
/// speech frame so far, `-3 dBFS / peak`, between 1 (never quieter) and +20 dB. It only goes
/// down as louder speech arrives, so there is no pumping; frames below the noise floor never
/// raise it, so silence before the first word is not boosted.
#[derive(Clone, Debug, Default)]
pub struct StreamGain {
    peak: f64,
}

impl StreamGain {
    /// Scales one PCM16LE frame in place.
    pub fn apply(&mut self, pcm: &mut [u8]) {
        if level(pcm) >= 0.000_316_228 {
            let frame_peak = pcm
                .as_chunks::<2>()
                .0
                .iter()
                .map(|bytes| f64::from(i16::from_le_bytes(*bytes)).abs() / 32_768.0)
                .fold(0.0_f64, f64::max);
            self.peak = self.peak.max(frame_peak);
        }
        let gain = if self.peak > 0.0 {
            (0.707_945_784 / self.peak).clamp(1.0, MAX_GAIN)
        } else {
            1.0
        };
        if gain == 1.0 {
            return;
        }
        for bytes in pcm.as_chunks_mut::<2>().0 {
            let sample = f64::from(i16::from_le_bytes(*bytes)) / 32_768.0;
            let scaled = (sample * gain).clamp(-0.99, 0.99);
            *bytes = ((scaled * 32_768.0).round() as i16).to_le_bytes();
        }
    }
}

/// How long a recording may go without samples once they flowed: a Bluetooth headset
/// switching profile or a changed default device can leave the stream silent for good.
pub const STALL_MS: u64 = 1_000;

/// Whether the microphone stopped delivering samples. Timestamps are milliseconds since
/// capture began; the clock starts at the first samples, as a device may be slow to open.
#[derive(Clone, Copy, Debug, Default)]
pub struct StallWatch {
    fed_at: Option<u64>,
}

impl StallWatch {
    /// Samples arrived at `now_ms`.
    pub fn fed(&mut self, now_ms: u64) {
        self.fed_at = Some(now_ms);
    }

    /// Whether more than `STALL_MS` passed since samples last arrived.
    pub fn stalled(&self, now_ms: u64) -> bool {
        self.fed_at
            .is_some_and(|fed_at| now_ms.saturating_sub(fed_at) > STALL_MS)
    }
}
