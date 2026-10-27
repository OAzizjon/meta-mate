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
