use mm_core::mic::{FRAME_BYTES, FRAME_SAMPLES, Framer, MicError, OUTPUT_RATE, level};
use proptest::prelude::*;

fn sine(rate: u32, channels: u16, seconds: usize) -> Vec<f32> {
    (0..rate as usize * seconds)
        .flat_map(|i| {
            let value = (std::f64::consts::TAU * 1_000.0 * i as f64 / f64::from(rate)).sin();
            std::iter::repeat_n(value as f32 * 0.8, usize::from(channels))
        })
        .collect()
}

fn decode(bytes: &[u8]) -> Vec<i16> {
    bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| i16::from_le_bytes(*b))
        .collect()
}

fn complete(framer: &mut Framer, mut frames: Vec<Vec<u8>>) -> Vec<Vec<u8>> {
    if let Some(last) = framer.finish() {
        frames.push(last);
    }
    frames
}

#[test]
fn sine_frequency_and_exact_duration_at_device_rates() {
    for (rate, channels) in [(48_000, 2), (44_100, 1), (16_000, 1)] {
        // Two seconds is exactly 25 frames, so finish must not invent a tail.
        let input = sine(rate, channels, 2);
        let mut framer = Framer::new(rate, channels).unwrap();
        let frames = framer.push(&input);
        assert_eq!(frames.len(), 25, "rate {rate}");
        assert!(frames.iter().all(|f| f.len() == FRAME_BYTES));
        assert_eq!(framer.finish(), None);
        let pcm = decode(&frames.concat());
        let crossings = pcm.windows(2).filter(|w| w[0] <= 0 && w[1] > 0).count();
        let frequency = crossings as f64 / 2.0;
        assert!((frequency - 1_000.0).abs() < 20.0, "{rate}: {frequency}");
        assert!((level(&frames[5]) - 0.8 / 2.0_f32.sqrt()).abs() < 0.03);
    }
}

#[test]
fn whole_second_frame_counts_and_padding() {
    for rate in [48_000, 44_100, 16_000] {
        for seconds in 1..=3 {
            let mut framer = Framer::new(rate, 1).unwrap();
            let frames = framer.push(&vec![0.5; rate as usize * seconds]);
            let count = OUTPUT_RATE as usize * seconds;
            assert_eq!(frames.len(), count / FRAME_SAMPLES);
            let tail = framer.finish();
            if count.is_multiple_of(FRAME_SAMPLES) {
                assert_eq!(tail, None);
            } else {
                let tail = tail.unwrap();
                let used = count % FRAME_SAMPLES * 2;
                assert_eq!(tail.len(), FRAME_BYTES);
                assert!(
                    tail[..used]
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .all(|b| *b == [0, 64])
                );
                assert!(tail[used..].iter().all(|&b| b == 0));
            }
        }
    }
}

#[test]
fn finish_pads_once_and_ignores_incomplete_channel_group() {
    let mut framer = Framer::new(24_000, 2).unwrap();
    assert!(framer.push(&[0.5, 0.5, 0.9]).is_empty());
    let frame = framer.finish().unwrap();
    assert_eq!(&frame[..2], &[0, 64]);
    assert!(frame[2..].iter().all(|&b| b == 0));
    assert_eq!(framer.finish(), None);
    assert!(framer.push(&[1.0; 4_000]).is_empty());
    assert_eq!(Framer::new(24_000, 1).unwrap().finish(), None);
}

#[test]
fn clipping_rounding_and_little_endian() {
    let mut framer = Framer::new(24_000, 1).unwrap();
    framer.push(&[-2.0, -1.0, -0.5, 0.0, 0.5, 1.0, 2.0]);
    let frame = framer.finish().unwrap();
    assert_eq!(
        decode(&frame[..14]),
        [-32767, -32767, -16384, 0, 16384, 32767, 32767]
    );
    assert_eq!(&frame[8..10], &[0, 64]);
}

#[test]
fn upsampling_interpolates_across_pushes() {
    let mut framer = Framer::new(16_000, 1).unwrap();
    framer.push(&[0.0]);
    framer.push(&[0.75]);
    framer.push(&[0.0]);
    let frame = framer.finish().unwrap();
    assert_eq!(decode(&frame[..8]), [0, 8192, 24575, 8192]);
}

#[test]
fn stereo_opposites_cancel_and_downmix_precedes_clipping() {
    let mut framer = Framer::new(24_000, 2).unwrap();
    framer.push(&[0.75, -0.75, 2.0, -1.0]);
    let frame = framer.finish().unwrap();
    assert_eq!(decode(&frame[..4]), [0, 16384]);
}

#[test]
fn rms_handles_silence_full_scale_and_odd_bytes() {
    assert_eq!(level(&[]), 0.0);
    assert_eq!(level(&[0; FRAME_BYTES]), 0.0);
    assert_eq!(level(&i16::MIN.to_le_bytes()), 1.0);
    assert_eq!(level(&[0, 64, 255]), 0.5);
    let mut framer = Framer::new(24_000, 1).unwrap();
    framer.push(&[f32::NAN, f32::INFINITY, f32::NEG_INFINITY]);
    let frame = framer.finish().unwrap();
    assert_eq!(decode(&frame[..6]), [0, 32767, -32767]);
    assert!((0.0..=1.0).contains(&level(&frame)));
}

#[test]
fn rejects_zero_rate_or_channels() {
    assert_eq!(Framer::new(0, 1).unwrap_err(), MicError::InvalidRate);
    assert_eq!(
        Framer::new(24_000, 0).unwrap_err(),
        MicError::InvalidChannels
    );
}

proptest! {
    #[test]
    fn chunk_sizes_preserve_every_byte(
        rate in prop_oneof![Just(48_000_u32), Just(44_100), Just(16_000), Just(24_000)],
        channels in 1_u16..=3,
        input in prop::collection::vec(-2.0_f32..2.0, 0..15_000),
        sizes in prop::collection::vec(1_usize..513, 1..40),
    ) {
        let mut whole = Framer::new(rate, channels).unwrap();
        let frames = whole.push(&input);
        let expected = complete(&mut whole, frames);
        let mut chunked = Framer::new(rate, channels).unwrap();
        let mut actual = Vec::new();
        let mut offset = 0;
        for size in sizes.iter().cycle() {
            if offset == input.len() {
                break;
            }
            let end = (offset + size).min(input.len());
            actual.extend(chunked.push(&input[offset..end]));
            prop_assert!(chunked.push(&[]).is_empty());
            offset = end;
        }
        let actual = complete(&mut chunked, actual);
        prop_assert_eq!(actual, expected);
    }
}

#[test]
fn stream_gain_lifts_quiet_speech_frame_by_frame_and_never_pumps() {
    use mm_core::mic::StreamGain;
    let frame = |amplitude: f64| -> Vec<u8> {
        (0..1920)
            .flat_map(|i| {
                let v = (amplitude * (i as f64 * 0.05).sin() * 32_767.0) as i16;
                v.to_le_bytes()
            })
            .collect()
    };
    let peak = |pcm: &[u8]| {
        pcm.chunks(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]).unsigned_abs())
            .max()
            .unwrap_or(0)
    };
    let mut gain = StreamGain::default();
    // Silence first: untouched.
    let mut silent = vec![0u8; 3840];
    gain.apply(&mut silent);
    assert!(silent.iter().all(|&b| b == 0));
    // Quiet speech (-30 dBFS) is lifted by the full +20 dB.
    let mut quiet = frame(0.03);
    let before = peak(&quiet);
    gain.apply(&mut quiet);
    assert!(peak(&quiet) >= before * 9, "{} -> {}", before, peak(&quiet));
    // Louder speech lowers the gain, never clips.
    let mut loud = frame(0.5);
    gain.apply(&mut loud);
    assert!(peak(&loud) <= (0.99 * 32_768.0) as u16);
    // After a loud frame, a quiet one is not boosted back up (no pumping).
    let mut quiet_again = frame(0.03);
    let before = peak(&quiet_again);
    gain.apply(&mut quiet_again);
    assert!(peak(&quiet_again) < before * 3);
}
