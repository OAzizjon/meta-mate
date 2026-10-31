//! Shared PNG preparation for synthetic benchmarks (never live capture).
use std::{error::Error, fs::File, io::BufReader};

use mm_core::geom::PhysRect;
use mm_core::redaction::{Redacted, RedactionInput, redact};
use mm_core::shot::Bgra;

pub fn image(path: &str, max_edge: u32) -> Result<Redacted, Box<dyn Error>> {
    let mut decoder = png::Decoder::new(BufReader::new(File::open(path)?));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info()?;
    let mut bytes = vec![0; reader.output_buffer_size().ok_or("PNG too large")?];
    let info = reader.next_frame(&mut bytes)?;
    let channels = match info.color_type {
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        _ => return Err("unsupported PNG color type".into()),
    };
    let mut bgra = Vec::new();
    for pixel in bytes[..info.buffer_size()].chunks_exact(channels) {
        if channels < 3 {
            bgra.extend_from_slice(&[pixel[0], pixel[0], pixel[0], 255]);
        } else {
            bgra.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 255]);
        }
    }
    let region = PhysRect::new(0, 0, info.width.try_into()?, info.height.try_into()?)?;
    Ok(redact(&RedactionInput {
        capture: Bgra::new(info.width, info.height, &bgra)?,
        region,
        snapshot_region: region,
        fields: &[],
        snapshot_truncated: false,
        unclassified_sensitive_field: false,
        max_edge,
    })?)
}
