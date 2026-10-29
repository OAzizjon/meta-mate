use std::io::Cursor;

use mm_core::shot::{
    Bgra, REDACTION_PAD_PX, ShotError, ShotRectPx, downscale_to_rgb, encode_png_rgb, paint_black,
    png_data_url,
};

fn rect(left: i64, top: i64, right: i64, bottom: i64) -> ShotRectPx {
    ShotRectPx {
        left,
        top,
        right,
        bottom,
    }
}

#[test]
fn constructor_validates_dimensions_length_and_overflow() {
    assert!(Bgra::new(1, 1, &[1, 2, 3, 4]).is_ok());
    for (width, height) in [(0, 1), (1, 0), (0, 0)] {
        assert_eq!(
            Bgra::new(width, height, &[]).unwrap_err(),
            ShotError::InvalidImageSize
        );
    }
    for pixels in [&[0u8; 3][..], &[0u8; 5][..]] {
        assert!(matches!(
            Bgra::new(1, 1, pixels),
            Err(ShotError::InvalidBufferLength { .. })
        ));
    }
    assert_eq!(
        Bgra::new(u32::MAX, u32::MAX, &[]).unwrap_err(),
        ShotError::SizeOverflow
    );
}

#[test]
fn bgra_debug_shows_sizes_only() {
    let image = Bgra::new(1, 1, &[123, 124, 125, 126]).unwrap();
    assert_eq!(
        format!("{image:?}"),
        "Bgra { width: 1, height: 1, bytes: 4 }"
    );
}

#[test]
fn edge_is_clipped_and_outside_rect_is_not_counted() {
    let mut pixels = vec![37; 10 * 10 * 4];
    let count = paint_black(
        &mut pixels,
        10,
        10,
        &[rect(-2, -2, 1, 1), rect(30, 30, 40, 40)],
    )
    .unwrap();
    assert_eq!(count, 1);
    for y in 0..10 {
        for x in 0..10 {
            let i = (y * 10 + x) * 4;
            let expected = if x < 5 && y < 5 {
                [0, 0, 0, 255]
            } else {
                [37; 4]
            };
            assert_eq!(&pixels[i..i + 4], &expected);
        }
    }
}

#[test]
fn padding_covers_all_four_sides() {
    assert_eq!(REDACTION_PAD_PX, 4);
    let mut pixels = vec![99; 20 * 20 * 4];
    assert_eq!(
        paint_black(&mut pixels, 20, 20, &[rect(8, 8, 10, 10)]).unwrap(),
        1
    );
    for y in 0..20 {
        for x in 0..20 {
            let i = (y * 20 + x) * 4;
            let expected = if (4..14).contains(&x) && (4..14).contains(&y) {
                [0, 0, 0, 255]
            } else {
                [99; 4]
            };
            assert_eq!(&pixels[i..i + 4], &expected);
        }
    }
}

#[test]
fn padding_precedes_clipping_and_saturates_extreme_coordinates() {
    let mut pixels = vec![99; 4 * 4 * 4];
    assert_eq!(
        paint_black(&mut pixels, 4, 4, &[rect(5, 0, 6, 1)]).unwrap(),
        1
    );
    assert_eq!(&pixels[4..8], &[0, 0, 0, 255]);
    let rects = [
        rect(i64::MIN, i64::MIN, i64::MAX, i64::MAX),
        rect(i64::MIN, 0, i64::MIN + 1, 1),
        rect(i64::MAX - 1, 0, i64::MAX, 1),
        rect(2, 2, 1, 1),
        rect(1, 1, 1, 1),
    ];
    assert_eq!(paint_black(&mut pixels, 4, 4, &rects).unwrap(), 1);
    assert!(
        pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| *p == [0, 0, 0, 255])
    );
}

#[test]
fn half_size_averages_a_two_by_two_block_exactly() {
    let mut pixels = vec![0; 2560 * 1600 * 4];
    for (x, y, bgra) in [
        (0, 0, [4, 8, 12, 0]),
        (1, 0, [8, 16, 24, 1]),
        (0, 1, [12, 24, 36, 128]),
        (1, 1, [16, 32, 48, 255]),
    ] {
        let i = (y * 2560 + x) * 4;
        pixels[i..i + 4].copy_from_slice(&bgra);
    }
    let (rgb, width, height) =
        downscale_to_rgb(Bgra::new(2560, 1600, &pixels).unwrap(), 1280).unwrap();
    assert_eq!((width, height), (1280, 800));
    assert_eq!(rgb.len(), 1280 * 800 * 3);
    assert_eq!(&rgb[..3], &[30, 20, 10]);
    assert!(rgb[3..].iter().all(|v| *v == 0));
}

#[test]
fn odd_dimensions_use_fractional_area_weights() {
    let mut pixels = vec![0; 1001 * 3 * 4];
    // The first output pixel covers 2.002 source columns and all three rows.
    // Each row has red 100 and 200 in columns 0 and 1, and zero elsewhere.
    for y in 0..3 {
        pixels[(y * 1001) * 4 + 2] = 100;
        pixels[(y * 1001 + 1) * 4 + 2] = 200;
    }
    let (rgb, width, height) = downscale_to_rgb(Bgra::new(1001, 3, &pixels).unwrap(), 500).unwrap();
    assert_eq!((width, height), (500, 1));
    assert_eq!(&rgb[..3], &[149, 0, 0]);
    assert!(rgb[3..].iter().all(|v| *v == 0));

    // Nonintegral boundaries split the middle source pixel equally.
    let pixels = [0, 0, 0, 0, 0, 0, 90, 0, 0, 0, 240, 0];
    let (rgb, width, height) = downscale_to_rgb(Bgra::new(3, 1, &pixels).unwrap(), 2).unwrap();
    assert_eq!((width, height), (2, 1));
    assert_eq!(rgb, [30, 0, 0, 190, 0, 0]);
}

#[test]
fn no_upscaling_and_zero_keeps_size_and_rgb_order() {
    let pixels = [1, 2, 3, 0, 4, 5, 6, 255];
    for max_edge in [0, 2, 1280] {
        let (rgb, width, height) =
            downscale_to_rgb(Bgra::new(1, 2, &pixels).unwrap(), max_edge).unwrap();
        assert_eq!((width, height), (1, 2));
        assert_eq!(rgb, [3, 2, 1, 6, 5, 4]);
    }
}

#[test]
fn pixel_operations_reject_invalid_buffers() {
    let mut pixels = [9; 3];
    assert!(paint_black(&mut pixels, 1, 1, &[]).is_err());
    assert_eq!(pixels, [9; 3]);
    let invalid = Bgra {
        width: 1,
        height: 1,
        pixels: &pixels,
    };
    assert!(downscale_to_rgb(invalid, 1).is_err());
    assert!(encode_png_rgb(&pixels, 0, 1).is_err());
    assert!(encode_png_rgb(&pixels, 2, 1).is_err());
}

#[test]
fn png_round_trip_preserves_rgb_pixels() {
    let rgb = [255, 0, 0, 0, 255, 0, 0, 0, 255, 17, 42, 99];
    let bytes = encode_png_rgb(&rgb, 2, 2).unwrap();
    let decoder = png::Decoder::new(Cursor::new(bytes));
    let mut reader = decoder.read_info().unwrap();
    let mut decoded = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut decoded).unwrap();
    assert_eq!((info.width, info.height), (2, 2));
    assert_eq!(info.color_type, png::ColorType::Rgb);
    assert_eq!(info.bit_depth, png::BitDepth::Eight);
    assert_eq!(&decoded[..info.buffer_size()], &rgb);
}

#[test]
fn data_url_uses_png_prefix_and_standard_base64() {
    assert_eq!(png_data_url(&[251, 255]), "data:image/png;base64,+/8=");
    assert_eq!(png_data_url(&[]), "data:image/png;base64,");
}

// The pre-WP113 exact box filter, retained as an independent accuracy and
// timing reference. Production must not do u128 arithmetic per source pixel.
fn exact_box(src: Bgra<'_>, width: u32, height: u32) -> Vec<u8> {
    let (sw, sh) = (u64::from(src.width), u64::from(src.height));
    let (dw, dh) = (u64::from(width), u64::from(height));
    let area = u128::from(sw) * u128::from(sh);
    let mut rgb = vec![0; width as usize * height as usize * 3];
    for y in 0..dh {
        let (top, bottom) = (y * sh, (y + 1) * sh);
        for x in 0..dw {
            let (left, right) = (x * sw, (x + 1) * sw);
            let mut sums = [0u128; 3];
            for sy in top / dh..bottom.div_ceil(dh) {
                let wy = bottom.min((sy + 1) * dh) - top.max(sy * dh);
                for sx in left / dw..right.div_ceil(dw) {
                    let wx = right.min((sx + 1) * dw) - left.max(sx * dw);
                    let weight = u128::from(wx) * u128::from(wy);
                    let i = (sy as usize * src.width as usize + sx as usize) * 4;
                    for (channel, offset) in [2, 1, 0].into_iter().enumerate() {
                        sums[channel] += u128::from(src.pixels[i + offset]) * weight;
                    }
                }
            }
            let out = (y as usize * width as usize + x as usize) * 3;
            for channel in 0..3 {
                rgb[out + channel] = (sums[channel] / area) as u8;
            }
        }
    }
    rgb
}

fn synthetic_bgra(width: u32, height: u32) -> Vec<u8> {
    let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height {
        for x in 0..width {
            pixels.extend_from_slice(&[
                (x / 8 + y / 16) as u8,
                (y / 4) as u8,
                (x.wrapping_mul(13) ^ y.wrapping_mul(7)) as u8,
                (x ^ y) as u8,
            ]);
        }
    }
    pixels
}

#[test]
fn fixed_point_box_matches_exact_fixtures_within_one_channel_level() {
    for (sw, sh) in [(1, 31), (31, 1), (3, 2), (17, 29), (101, 63)] {
        for pixels in [
            synthetic_bgra(sw, sh),
            vec![255; sw as usize * sh as usize * 4],
            (0..sw as usize * sh as usize * 4)
                .map(|i| if i % 3 == 0 { 255 } else { 0 })
                .collect(),
        ] {
            let src = Bgra::new(sw, sh, &pixels).unwrap();
            for edge in [0, 1, 2, 7, 19, 100] {
                let (actual, width, height) = downscale_to_rgb(src, edge).unwrap();
                let expected = exact_box(src, width, height);
                for (actual, expected) in actual.iter().zip(expected) {
                    assert!(actual.abs_diff(expected) <= 1, "{sw}x{sh}, edge {edge}");
                }
            }
        }
    }
}

fn default_png(rgb: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(rgb).unwrap();
        writer.finish().unwrap();
    }
    bytes
}

/// Manual micro-benchmark: elapsed times include output allocation, exclude
/// fixture construction, and make no timing assertions. Compare in one build.
/// Local WP113 sample (test opt-level=3, dependencies=2): box 106.72 -> 92.57 ms;
/// PNG 119.01 -> 53.41 ms, 62,602 -> 3,417,046 bytes. This high-frequency
/// synthetic frame exposes Fast's size tradeoff. Unoptimized box timings were
/// 1089.01 -> 541.63 ms; PNG was slower unoptimized (119.54 -> 327.06 ms).
#[test]
#[ignore = "manual WP113 before/after timings and PNG byte sizes"]
fn screenshot_before_after_timings() {
    use std::hint::black_box;
    use std::time::Instant;

    let pixels = synthetic_bgra(1920, 1080);
    let src = Bgra::new(1920, 1080, &pixels).unwrap();
    let start = Instant::now();
    let before = black_box(exact_box(black_box(src), 1280, 720));
    let old_box = start.elapsed();
    let start = Instant::now();
    let (after, _, _) = black_box(downscale_to_rgb(black_box(src), 1280).unwrap());
    let new_box = start.elapsed();
    assert!(before.iter().zip(&after).all(|(a, b)| a.abs_diff(*b) <= 1));

    // Measure PNG on the full 1920x1080 synthetic frame, not the scaled image.
    let (rgb, _, _) = downscale_to_rgb(src, 0).unwrap();
    let start = Instant::now();
    let before = black_box(default_png(black_box(&rgb), 1920, 1080));
    let old_png = start.elapsed();
    let start = Instant::now();
    let after = black_box(encode_png_rgb(black_box(&rgb), 1920, 1080).unwrap());
    let new_png = start.elapsed();
    println!("box 1920x1080 -> 1280x720: before {old_box:?}, after {new_box:?}");
    println!(
        "PNG 1920x1080: before {old_png:?} / {} bytes, after {new_png:?} / {} bytes",
        before.len(),
        after.len()
    );
}
