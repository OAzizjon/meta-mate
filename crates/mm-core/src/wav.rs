//! In-memory RIFF/WAVE audio for recordings, proxy speech and canned phrases.

pub struct Wav {
    pub sample_rate: u32,
    pub channels: u16,
    pub samples: Vec<f32>,
}

impl std::fmt::Debug for Wav {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Wav")
            .field("sample_rate", &self.sample_rate)
            .field("channels", &self.channels)
            .field("sample_count", &self.samples.len())
            .finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WavError {
    InvalidHeader,
    TruncatedChunk,
    MissingFmt,
    MissingData,
    InvalidFmt,
    InvalidChannels,
    InvalidRate,
    UnsupportedFormat,
    InvalidBlockAlign,
}

impl std::fmt::Display for WavError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidHeader => "expected RIFF/WAVE header",
            Self::TruncatedChunk => "truncated WAVE chunk",
            Self::MissingFmt => "missing WAVE format chunk",
            Self::MissingData => "missing WAVE data chunk",
            Self::InvalidFmt => "invalid WAVE format chunk",
            Self::InvalidChannels => "WAVE channel count must be between 1 and 8",
            Self::InvalidRate => "WAVE sample rate must be between 1 and 384000",
            Self::UnsupportedFormat => "unsupported WAVE encoding or sample width",
            Self::InvalidBlockAlign => "invalid WAVE block alignment",
        })
    }
}

impl std::error::Error for WavError {}

fn array<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N], WavError> {
    bytes
        .get(offset..)
        .and_then(|b| b.first_chunk::<N>())
        .copied()
        .ok_or(WavError::InvalidFmt)
}

struct Format {
    rate: u32,
    channels: u16,
    align: usize,
    pcm: bool,
}

fn format(bytes: &[u8]) -> Result<Format, WavError> {
    let mut tag = u16::from_le_bytes(array(bytes, 0)?);
    let channels = u16::from_le_bytes(array(bytes, 2)?);
    let rate = u32::from_le_bytes(array(bytes, 4)?);
    let align = u16::from_le_bytes(array(bytes, 12)?);
    let bits = u16::from_le_bytes(array(bytes, 14)?);
    if channels == 0 || channels > 8 {
        return Err(WavError::InvalidChannels);
    }
    if rate == 0 || rate > 384_000 {
        return Err(WavError::InvalidRate);
    }
    if tag == 0xfffe {
        let extra = usize::from(u16::from_le_bytes(array(bytes, 16)?));
        if extra < 22 || bytes.len().saturating_sub(18) < extra {
            return Err(WavError::InvalidFmt);
        }
        let valid_bits = u16::from_le_bytes(array(bytes, 18)?);
        if valid_bits != bits {
            return Err(WavError::UnsupportedFormat);
        }
        let guid = array::<16>(bytes, 24)?;
        tag = match guid {
            [1, 0, 0, 0, 0, 0, 16, 0, 128, 0, 0, 170, 0, 56, 155, 113] => 1,
            [3, 0, 0, 0, 0, 0, 16, 0, 128, 0, 0, 170, 0, 56, 155, 113] => 3,
            _ => return Err(WavError::UnsupportedFormat),
        };
    }
    if !matches!((tag, bits), (1, 16) | (3, 32)) {
        return Err(WavError::UnsupportedFormat);
    }
    if align != channels * (bits / 8) {
        return Err(WavError::InvalidBlockAlign);
    }
    Ok(Format {
        rate,
        channels,
        align: usize::from(align),
        pcm: tag == 1,
    })
}

fn chunks(bytes: &[u8]) -> Result<(Format, &[u8]), WavError> {
    if bytes.get(..4) != Some(b"RIFF") || bytes.get(8..12) != Some(b"WAVE") {
        return Err(WavError::InvalidHeader);
    }
    // The outer RIFF length is advisory: streaming producers may not finalize it.
    let mut rest = bytes.get(12..).ok_or(WavError::InvalidHeader)?;
    let mut fmt = None;
    let mut data = None;
    while !rest.is_empty() {
        let header = rest.get(..8).ok_or(WavError::TruncatedChunk)?;
        let size = u32::from_le_bytes(array(header, 4)?) as usize;
        let body = rest.get(8..).ok_or(WavError::TruncatedChunk)?;
        let id = header.get(..4);
        if id == Some(b"data") && data.is_none() {
            data = body.get(..size.min(body.len()));
        } else if id == Some(b"fmt ") && fmt.is_none() {
            fmt = Some(format(body.get(..size).ok_or(WavError::TruncatedChunk)?)?);
        }
        if fmt.is_some() && data.is_some() {
            break;
        }
        if size > body.len() {
            if id == Some(b"data") {
                break;
            }
            return Err(WavError::TruncatedChunk);
        }
        rest = body.get(size..).ok_or(WavError::TruncatedChunk)?;
        if !size.is_multiple_of(2) {
            rest = rest.get(1..).ok_or(WavError::TruncatedChunk)?;
        }
    }
    let fmt = fmt.ok_or(WavError::MissingFmt)?;
    let data = data.ok_or(WavError::MissingData)?;
    let whole = data.len() - data.len() % fmt.align;
    Ok((fmt, data.get(..whole).ok_or(WavError::TruncatedChunk)?))
}

/// Decodes complete interleaved frames. NaN floats become silence; other floats
/// are clamped to full scale. PCM -32768 and -32767 both map to -1.
pub fn read(bytes: &[u8]) -> Result<Wav, WavError> {
    let (fmt, data) = chunks(bytes)?;
    let samples = if fmt.pcm {
        data.as_chunks::<2>()
            .0
            .iter()
            .map(|b| (f32::from(i16::from_le_bytes(*b)) / 32767.0).clamp(-1.0, 1.0))
            .collect()
    } else {
        data.as_chunks::<4>()
            .0
            .iter()
            .map(|b| {
                let value = f32::from_le_bytes(*b);
                if value.is_nan() {
                    0.0
                } else {
                    value.clamp(-1.0, 1.0)
                }
            })
            .collect()
    };
    Ok(Wav {
        sample_rate: fmt.rate,
        channels: fmt.channels,
        samples,
    })
}

/// Borrows complete PCM16 frames, including PCM16 in an extensible container.
pub fn pcm16_samples(bytes: &[u8]) -> Result<(u32, u16, &[u8]), WavError> {
    let (fmt, data) = chunks(bytes)?;
    if !fmt.pcm {
        return Err(WavError::UnsupportedFormat);
    }
    Ok((fmt.rate, fmt.channels, data))
}

/// Writes a canonical PCM16 header. Callers supply a valid rate/channel count
/// and complete interleaved frames. Audio exceeding RIFF's size limit is capped.
pub fn write_pcm16(sample_rate: u32, channels: u16, samples: &[f32]) -> Vec<u8> {
    let count = samples.len().min(((u32::MAX - 36) / 2) as usize);
    let size = (count as u32) * 2;
    let align = channels.saturating_mul(2);
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + size).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&channels.to_le_bytes());
    bytes.extend_from_slice(&sample_rate.to_le_bytes());
    bytes.extend_from_slice(&sample_rate.saturating_mul(u32::from(align)).to_le_bytes());
    bytes.extend_from_slice(&align.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&size.to_le_bytes());
    for &sample in samples.iter().take(count) {
        let pcm = (sample.clamp(-1.0, 1.0) * 32767.0).round() as i16;
        bytes.extend_from_slice(&pcm.to_le_bytes());
    }
    bytes
}
