use mm_core::wav::{WavError, pcm16_samples, read, write_pcm16};
use proptest::prelude::*;

fn fmt(tag: u16, bits: u16, channels: u16, rate: u32) -> Vec<u8> {
    let align = channels * (bits / 8);
    let mut bytes = Vec::new();
    bytes.extend(tag.to_le_bytes());
    bytes.extend(channels.to_le_bytes());
    bytes.extend(rate.to_le_bytes());
    bytes.extend((rate * u32::from(align)).to_le_bytes());
    bytes.extend(align.to_le_bytes());
    bytes.extend(bits.to_le_bytes());
    bytes
}

fn extensible(tag: u16, bits: u16) -> Vec<u8> {
    let mut bytes = fmt(0xfffe, bits, 2, 24_000);
    bytes.extend(22_u16.to_le_bytes());
    bytes.extend(bits.to_le_bytes());
    bytes.extend(3_u32.to_le_bytes());
    bytes.extend(u32::from(tag).to_le_bytes());
    bytes.extend([0, 0, 16, 0, 128, 0, 0, 170, 0, 56, 155, 113]);
    bytes
}

fn chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut bytes = id.to_vec();
    bytes.extend((body.len() as u32).to_le_bytes());
    bytes.extend(body);
    if !body.len().is_multiple_of(2) {
        bytes.push(0);
    }
    bytes
}

fn riff(chunks: &[Vec<u8>]) -> Vec<u8> {
    let body = chunks.concat();
    let mut bytes = b"RIFF".to_vec();
    bytes.extend((4 + body.len() as u32).to_le_bytes());
    bytes.extend(b"WAVE");
    bytes.extend(body);
    bytes
}

fn wav(format: &[u8], data: &[u8]) -> Vec<u8> {
    riff(&[chunk(b"fmt ", format), chunk(b"data", data)])
}

#[test]
fn round_trip_and_canonical_header() {
    let samples = [-1.0, -0.7, -0.1, 0.0, 0.1, 0.7, 1.0, 0.4];
    let bytes = write_pcm16(48_000, 2, &samples);
    assert_eq!(bytes.len(), 44 + samples.len() * 2);
    assert_eq!(&bytes[..44], &wav(&fmt(1, 16, 2, 48_000), &[0; 16])[..44]);
    let decoded = read(&bytes).unwrap();
    assert_eq!((decoded.sample_rate, decoded.channels), (48_000, 2));
    for (actual, expected) in decoded.samples.iter().zip(samples) {
        assert!((actual - expected).abs() <= 1.0 / 32767.0);
    }
    assert_eq!(
        format!("{decoded:?}"),
        "Wav { sample_rate: 48000, channels: 2, sample_count: 8 }"
    );
    let (_, _, raw) = pcm16_samples(&bytes).unwrap();
    assert_eq!(raw.as_ptr(), bytes[44..].as_ptr());
    assert_eq!(raw, &bytes[44..]);
    let clipped = read(&write_pcm16(16_000, 1, &[-2.0, 2.0, f32::NAN])).unwrap();
    assert_eq!(clipped.samples, [-1.0, 1.0, 0.0]);
}

#[test]
fn manual_pcm_float_and_extensible_headers() {
    let pcm: Vec<_> = [i16::MIN, -16384, 0, i16::MAX]
        .into_iter()
        .flat_map(i16::to_le_bytes)
        .collect();
    for format in [fmt(1, 16, 2, 24_000), extensible(1, 16)] {
        let bytes = wav(&format, &pcm);
        let decoded = read(&bytes).unwrap();
        assert_eq!(decoded.samples, [-1.0, -16384.0 / 32767.0, 0.0, 1.0]);
        assert_eq!(pcm16_samples(&bytes).unwrap(), (24_000, 2, pcm.as_slice()));
    }
    let values = [-1.0_f32, 0.25, 1.5, -2.0, f32::NAN, f32::INFINITY];
    let floats: Vec<_> = values.into_iter().flat_map(f32::to_le_bytes).collect();
    for format in [fmt(3, 32, 2, 24_000), extensible(3, 32)] {
        let bytes = wav(&format, &floats);
        assert_eq!(
            read(&bytes).unwrap().samples,
            [-1.0, 0.25, 1.0, -1.0, 0.0, 1.0]
        );
        assert_eq!(pcm16_samples(&bytes), Err(WavError::UnsupportedFormat));
    }
}

#[test]
fn skips_unknown_chunks_padding_and_uses_first_chunks() {
    let bytes = riff(&[
        chunk(b"LIST", b"INFO"),
        chunk(b"JUNK", &[42]),
        chunk(b"fmt ", &fmt(1, 16, 1, 16_000)),
        chunk(b"fmt ", &fmt(7, 8, 1, 16_000)),
        chunk(b"data", &32767_i16.to_le_bytes()),
        chunk(b"data", &[0, 0]),
    ]);
    assert_eq!(read(&bytes).unwrap().samples, [1.0]);
    let data_first = riff(&[
        chunk(b"data", &[0, 0]),
        chunk(b"fmt ", &fmt(1, 16, 1, 16_000)),
    ]);
    assert_eq!(read(&data_first).unwrap().samples, [0.0]);
}

#[test]
fn truncated_and_streamed_data_keep_whole_frames() {
    for declared in [12_u32, u32::MAX] {
        for channels in [1, 2] {
            let mut bytes = wav(&fmt(1, 16, channels, 16_000), &[]);
            bytes[40..44].copy_from_slice(&declared.to_le_bytes());
            bytes.extend([0xff, 0x7f, 0, 0, 0x42, 0, 0x99]);
            let expected = if channels == 1 { 6 } else { 4 };
            assert_eq!(pcm16_samples(&bytes).unwrap().2, &bytes[44..44 + expected]);
            assert_eq!(read(&bytes).unwrap().samples.len(), expected / 2);
        }
    }
    // The RIFF length must not hide available streaming data.
    let mut bytes = write_pcm16(16_000, 1, &[0.5]);
    bytes[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(read(&bytes).unwrap().samples.len(), 1);
}

fn refusal(bytes: &[u8], error: WavError) {
    assert_eq!(read(bytes).unwrap_err(), error);
    assert_eq!(pcm16_samples(bytes).unwrap_err(), error);
    assert!(!error.to_string().is_empty());
}

#[test]
fn refuses_invalid_headers_formats_and_metadata() {
    refusal(b"", WavError::InvalidHeader);
    let valid = wav(&fmt(1, 16, 1, 16_000), &[]);
    for (offset, id) in [(0, b"RIFX"), (8, b"AVI ")] {
        let mut bytes = valid.clone();
        bytes[offset..offset + 4].copy_from_slice(id);
        refusal(&bytes, WavError::InvalidHeader);
    }
    refusal(&riff(&[chunk(b"data", &[])]), WavError::MissingFmt);
    refusal(
        &riff(&[chunk(b"fmt ", &fmt(1, 16, 1, 16_000))]),
        WavError::MissingData,
    );
    refusal(&wav(&[0; 15], &[]), WavError::InvalidFmt);
    for channels in [0, 9] {
        refusal(
            &wav(&fmt(1, 16, channels, 16_000), &[]),
            WavError::InvalidChannels,
        );
    }
    for rate in [0, 384_001] {
        refusal(&wav(&fmt(1, 16, 1, rate), &[]), WavError::InvalidRate);
    }
    for (tag, bits) in [(2, 16), (1, 8), (1, 24), (3, 16), (3, 64)] {
        refusal(
            &wav(&fmt(tag, bits, 1, 16_000), &[]),
            WavError::UnsupportedFormat,
        );
    }
    let mut bad = fmt(1, 16, 1, 16_000);
    bad[12..14].copy_from_slice(&4_u16.to_le_bytes());
    refusal(&wav(&bad, &[]), WavError::InvalidBlockAlign);
    let mut bad = extensible(1, 16);
    bad[39] ^= 1;
    refusal(&wav(&bad, &[]), WavError::UnsupportedFormat);
    let mut bad = extensible(1, 16);
    bad[18..20].copy_from_slice(&12_u16.to_le_bytes());
    refusal(&wav(&bad, &[]), WavError::UnsupportedFormat);
    let mut bad = extensible(1, 16);
    bad[16..18].copy_from_slice(&21_u16.to_le_bytes());
    refusal(&wav(&bad, &[]), WavError::InvalidFmt);
    refusal(&wav(&extensible(1, 16)[..39], &[]), WavError::InvalidFmt);
    let mut truncated = riff(&[]);
    truncated.extend(b"fmt ");
    truncated.extend(16_u32.to_le_bytes());
    truncated.extend([0; 2]);
    refusal(&truncated, WavError::TruncatedChunk);
    let mut truncated = riff(&[]);
    truncated.extend(b"LIST");
    refusal(&truncated, WavError::TruncatedChunk);
    let mut truncated = riff(&[]);
    truncated.extend(b"JUNK\x03\0\0\0x");
    refusal(&truncated, WavError::TruncatedChunk);
}

#[test]
fn optional_asr_recordings() {
    let directory =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/replay/asr");
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries {
        let path = entry.unwrap().path();
        if path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("wav"))
        {
            let decoded = read(&std::fs::read(&path).unwrap()).unwrap();
            assert_eq!(decoded.samples.len() % usize::from(decoded.channels), 0);
        }
    }
}

proptest! {
    #[test]
    fn arbitrary_bytes_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..8192)) {
        let _ = read(&bytes);
        let _ = pcm16_samples(&bytes);
    }

    #[test]
    fn valid_header_arbitrary_tail_never_panics(
        tail in prop::collection::vec(any::<u8>(), 0..8192),
        channels in 1_u16..=8,
        streamed in any::<bool>(),
    ) {
        let mut bytes = wav(&fmt(1, 16, channels, 24_000), &[]);
        let size = if streamed { u32::MAX } else { tail.len() as u32 };
        bytes[40..44].copy_from_slice(&size.to_le_bytes());
        bytes.extend(tail);
        let decoded = read(&bytes).unwrap();
        prop_assert_eq!(decoded.samples.len() % usize::from(channels), 0);
        let _ = pcm16_samples(&bytes).unwrap();
    }
}
