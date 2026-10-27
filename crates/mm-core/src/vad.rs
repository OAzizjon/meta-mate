//! RMS-only end-of-speech detection. Timestamps are milliseconds since capture began.

const CALIBRATE_MS: u64 = 300;
const MIN_SPEECH_MS: u64 = 600;
const SILENCE_MS: u64 = 1_200;
const MAX_MS: u64 = 30_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SilenceEnd {
    Silence,
    TooLong,
}

/// One detector per recording. The first 300 ms estimate background noise; later
/// quiet frames slowly track it. Separate entry/exit thresholds prevent chatter.
#[derive(Debug, Default)]
pub struct SilenceDetector {
    first: Option<u64>,
    previous: Option<u64>,
    floor: f32,
    calibration_samples: u32,
    speaking: bool,
    speech_ms: u64,
    quiet_since: Option<u64>,
    ended: bool,
}

impl SilenceDetector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed a frame's RMS and capture timestamp, not its delivery time. Backward
    /// timestamps are clamped; invalid levels count as silence. Returns once only.
    pub fn input(&mut self, rms: f32, now_ms: u64) -> Option<SilenceEnd> {
        if self.ended {
            return None;
        }
        let now_ms = now_ms.max(self.previous.unwrap_or(now_ms));
        let first = *self.first.get_or_insert(now_ms);
        let elapsed = now_ms.saturating_sub(first);
        let delta = now_ms.saturating_sub(self.previous.replace(now_ms).unwrap_or(now_ms));
        if elapsed >= MAX_MS {
            self.ended = true;
            return Some(SilenceEnd::TooLong);
        }
        let rms = if rms.is_finite() {
            rms.clamp(0.0, 1.0)
        } else {
            0.0
        };
        if elapsed < CALIBRATE_MS {
            self.calibration_samples += 1;
            self.floor += (rms - self.floor) / self.calibration_samples as f32;
            // Calibration can include an immediate word; cap it so speech at the
            // start does not teach the detector that the speaker is background.
            self.floor = self.floor.min(0.03);
        }
        let enter = (self.floor * 3.0 + 0.008).max(0.018);
        let leave = (self.floor * 1.8 + 0.004).max(0.010);
        let was_speaking = self.speaking;
        self.speaking = rms >= if was_speaking { leave } else { enter };
        if was_speaking {
            self.speech_ms = self.speech_ms.saturating_add(delta);
        }
        if self.speaking {
            self.quiet_since = None;
        } else {
            if elapsed >= CALIBRATE_MS {
                self.floor += (rms.min(enter) - self.floor) * 0.02;
            }
            let quiet = *self.quiet_since.get_or_insert(now_ms);
            if self.speech_ms >= MIN_SPEECH_MS && now_ms.saturating_sub(quiet) >= SILENCE_MS {
                self.ended = true;
                return Some(SilenceEnd::Silence);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthetic_sequences() {
        // Each tuple is (duration in ms, RMS); feed at 20 ms capture intervals.
        type Case<'a> = (&'a str, &'a [(u64, f32)], Option<SilenceEnd>);
        let cases: &[Case] = &[
            (
                "speech then silence",
                &[(320, 0.005), (800, 0.15), (1_300, 0.005)],
                Some(SilenceEnd::Silence),
            ),
            (
                "short pause",
                &[(320, 0.005), (800, 0.15), (1_000, 0.005), (800, 0.15)],
                None,
            ),
            (
                "brief sound",
                &[(320, 0.005), (400, 0.15), (2_000, 0.005)],
                None,
            ),
            ("background", &[(5_000, 0.03)], None),
            (
                "never speech",
                &[(30_100, 0.005)],
                Some(SilenceEnd::TooLong),
            ),
            (
                "continuous speech",
                &[(320, 0.005), (30_000, 0.15)],
                Some(SilenceEnd::TooLong),
            ),
            (
                "noisy room speech",
                &[(320, 0.03), (800, 0.2), (1_300, 0.03)],
                Some(SilenceEnd::Silence),
            ),
            (
                "immediate speech",
                &[(800, 0.15), (1_300, 0.005)],
                Some(SilenceEnd::Silence),
            ),
            (
                "varying background",
                &[(320, 0.01), (800, 0.014), (800, 0.008), (800, 0.016)],
                None,
            ),
        ];
        for (name, sequence, expected) in cases {
            let mut detector = SilenceDetector::new();
            let mut now = 0;
            let mut results = Vec::new();
            for &(duration, rms) in *sequence {
                for _ in 0..duration / 20 {
                    if let Some(end) = detector.input(rms, now) {
                        results.push(end);
                    }
                    now += 20;
                }
            }
            assert_eq!(
                results,
                expected.iter().copied().collect::<Vec<_>>(),
                "{name}"
            );
            if expected.is_some() {
                assert_eq!(detector.input(0.0, now + MAX_MS), None, "{name}");
            }
        }
    }

    #[test]
    fn hysteresis_and_exact_silence_boundary() {
        let mut detector = SilenceDetector::new();
        for now in (0..320).step_by(20) {
            assert_eq!(detector.input(0.005, now), None);
        }
        assert_eq!(detector.input(0.1, 320), None);
        // Above the exit threshold, below entry: speech remains on.
        for now in (340..=920).step_by(20) {
            assert_eq!(detector.input(0.016, now), None);
        }
        assert_eq!(detector.input(0.0, 940), None);
        assert_eq!(detector.input(0.0, 2_139), None);
        assert_eq!(detector.input(0.0, 2_140), Some(SilenceEnd::Silence));
    }

    #[test]
    fn invalid_levels_and_backward_time_are_safe() {
        let mut detector = SilenceDetector::new();
        assert_eq!(detector.input(f32::NAN, 100), None);
        assert_eq!(detector.input(f32::INFINITY, 50), None);
        assert_eq!(detector.input(-1.0, 30_099), None);
        assert_eq!(detector.input(2.0, 30_100), Some(SilenceEnd::TooLong));
    }

    #[test]
    fn microphone_cadence_keeps_short_pauses_and_waits_for_enough_speech() {
        let mut detector = SilenceDetector::new();
        let mut now = 0;
        for (frames, rms) in [(4, 0.005), (4, 0.15), (20, 0.005), (4, 0.15)] {
            for _ in 0..frames {
                assert_eq!(detector.input(rms, now), None);
                now += 80;
            }
        }
        let quiet = now;
        for _ in 0..15 {
            assert_eq!(detector.input(0.005, now), None);
            now += 80;
        }
        assert_eq!(now - quiet, SILENCE_MS);
        assert_eq!(detector.input(0.005, now), Some(SilenceEnd::Silence));
    }
}
