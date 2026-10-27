//! A screenshot of the target region: `BitBlt` from the screen into a 32-bit top-down DIB
//! section, copied out as BGRA. Raw and unredacted, so it never leaves the PC as is
//! (convention 4). Hide our own windows first (`overlay::hide_for_capture`): CAPTUREBLT
//! also copies layered windows such as the highlight. An all-black result is refused,
//! never sent (invariant 3). Every GDI object is released on every path by a guard.

use std::ffi::c_void;
use std::fmt;

use mm_core::geom::PhysRect;
use mm_platform::CapturedImage;
use windows::Win32::Graphics::Gdi::{
    BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleDC, CreateDIBSection,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, GdiFlush, GetDC, HBITMAP, HDC, HGDIOBJ, ROP_CODE,
    ReleaseDC, SRCCOPY, SelectObject,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};

/// Why a capture was refused or failed. `Display` is for logs only and never carries
/// pixels; users hear a localized line chosen from the variant (invariant 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureError {
    /// The region has no pixels.
    Empty,
    /// The region is not entirely inside the virtual screen.
    OffScreen,
    /// A GDI call failed; the name says which.
    Gdi(&'static str),
    /// Every pixel is black: protected content (DRM video, a secure window) or a copy that
    /// silently failed.
    Black,
}

impl fmt::Display for CaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("the capture region is empty"),
            Self::OffScreen => f.write_str("the capture region is outside the virtual screen"),
            Self::Gdi(call) => write!(f, "{call} failed"),
            Self::Black => f.write_str("the capture is all black (protected content)"),
        }
    }
}

impl std::error::Error for CaptureError {}

/// Captures `region` (physical virtual-screen pixels) as BGRA, rows top to bottom. The
/// alpha byte is 255: GDI leaves it undefined and the screen is opaque.
pub fn capture_region(region: PhysRect) -> Result<CapturedImage, CaptureError> {
    // Revalidate: the fields are public, so a struct literal can be empty or inverted.
    let region = PhysRect::new(region.left, region.top, region.right, region.bottom)
        .map_err(|_| CaptureError::Empty)?;
    let screen = virtual_screen().ok_or(CaptureError::OffScreen)?;
    if !within(region, screen) {
        return Err(CaptureError::OffScreen);
    }
    // Inside the virtual screen, so neither difference can overflow.
    let (width, height) = (region.right - region.left, region.bottom - region.top);
    let (Ok(columns), Ok(rows)) = (u32::try_from(width), u32::try_from(height)) else {
        return Err(CaptureError::Empty);
    };
    let len = (columns as usize)
        .checked_mul(rows as usize)
        .and_then(|count| count.checked_mul(CapturedImage::BYTES_PER_PIXEL))
        .ok_or(CaptureError::Empty)?;

    // SAFETY: a null window asks for the DC of the whole virtual screen.
    let dc = unsafe { GetDC(None) };
    if dc.0.is_null() {
        return Err(CaptureError::Gdi("GetDC"));
    }
    let screen_dc = ScreenDc(dc);
    // SAFETY: `screen_dc` holds a valid DC for the whole call.
    let dc = unsafe { CreateCompatibleDC(Some(screen_dc.0)) };
    if dc.0.is_null() {
        return Err(CaptureError::Gdi("CreateCompatibleDC"));
    }
    let memory_dc = MemoryDc(dc);

    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            // Negative: a top-down DIB, rows in the order `CapturedImage` keeps them.
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            // biCompression stays 0 (BI_RGB): 32-bit rows are 4-byte aligned, no padding.
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits: *mut c_void = std::ptr::null_mut();
    // SAFETY: `info` describes a 32-bit top-down DIB and outlives the call; `bits` is a
    // writable pointer on our stack that receives the bitmap's own pixel memory; no file
    // mapping.
    let bitmap =
        unsafe { CreateDIBSection(Some(memory_dc.0), &info, DIB_RGB_COLORS, &mut bits, None, 0) }
            .map_err(|_| CaptureError::Gdi("CreateDIBSection"))?;
    let dib = DibSection(bitmap);
    if bits.is_null() {
        return Err(CaptureError::Gdi("CreateDIBSection"));
    }
    // SAFETY: both handles are valid; `Selected` selects the previous bitmap back before
    // `dib` is deleted.
    let previous = unsafe { SelectObject(memory_dc.0, HGDIOBJ(dib.0.0)) };
    if previous.0.is_null() {
        return Err(CaptureError::Gdi("SelectObject"));
    }
    let _selected = Selected {
        dc: memory_dc.0,
        previous,
    };

    // SAFETY: valid DCs; the destination is the whole DIB and the source lies inside the
    // virtual screen (checked above). CAPTUREBLT includes layered windows.
    unsafe {
        BitBlt(
            memory_dc.0,
            0,
            0,
            width,
            height,
            Some(screen_dc.0),
            region.left,
            region.top,
            ROP_CODE(SRCCOPY.0 | CAPTUREBLT.0),
        )
    }
    .map_err(|_| CaptureError::Gdi("BitBlt"))?;
    // SAFETY: no arguments; finishes GDI's batched drawing before the bits are read.
    let _ = unsafe { GdiFlush() };
    // SAFETY: `bits` points at the DIB's `len` bytes (`columns * rows` pixels of 4 bytes,
    // no row padding), owned by `dib`, which lives until the end of this function; nothing
    // writes to them while they are copied.
    let mut pixels = unsafe { std::slice::from_raw_parts(bits.cast::<u8>(), len) }.to_vec();
    if is_all_black(&pixels) {
        return Err(CaptureError::Black);
    }
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel[3] = 0xFF;
    }
    // Only a zero size or a mismatched buffer fails here, and neither can happen.
    CapturedImage::new(columns, rows, pixels).map_err(|_| CaptureError::Empty)
}

/// Whether every pixel of packed BGRA is black, alpha ignored (GDI leaves it undefined).
/// Protected content captures as pure black. An empty buffer counts as black: nothing to
/// show is never a screenshot.
pub fn is_all_black(bgra: &[u8]) -> bool {
    bgra.as_chunks::<4>()
        .0
        .iter()
        .all(|&[b, g, r, _]| b == 0 && g == 0 && r == 0)
}

/// The bounding rectangle of every monitor; physical pixels in a Per-Monitor v2 process.
fn virtual_screen() -> Option<PhysRect> {
    // SAFETY: plain metric indices, no pointers.
    let (left, top, width, height) = unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    };
    PhysRect::new(
        left,
        top,
        left.checked_add(width)?,
        top.checked_add(height)?,
    )
    .ok()
}

/// Whether `inner` lies entirely inside `outer`.
fn within(inner: PhysRect, outer: PhysRect) -> bool {
    outer.left <= inner.left
        && outer.top <= inner.top
        && inner.right <= outer.right
        && inner.bottom <= outer.bottom
}

/// The screen DC from `GetDC(None)`, released on every path.
struct ScreenDc(HDC);

impl Drop for ScreenDc {
    fn drop(&mut self) {
        // SAFETY: the DC came from GetDC(None) and is released exactly once, here.
        let _ = unsafe { ReleaseDC(None, self.0) };
    }
}

/// The memory DC from `CreateCompatibleDC`, deleted on every path.
struct MemoryDc(HDC);

impl Drop for MemoryDc {
    fn drop(&mut self) {
        // SAFETY: the DC came from CreateCompatibleDC and is deleted exactly once, here,
        // after `Selected` gave it its original bitmap back.
        let _ = unsafe { DeleteDC(self.0) };
    }
}

/// The DIB section, deleted on every path once it is no longer selected.
struct DibSection(HBITMAP);

impl Drop for DibSection {
    fn drop(&mut self) {
        // SAFETY: the bitmap came from CreateDIBSection, is no longer selected into a DC
        // (`Selected` is declared later, so it drops first) and is deleted exactly once.
        let _ = unsafe { DeleteObject(HGDIOBJ(self.0.0)) };
    }
}

/// The DIB section selected into the memory DC; dropping it selects the previous bitmap
/// back, so both can be deleted.
struct Selected {
    dc: HDC,
    previous: HGDIOBJ,
}

impl Drop for Selected {
    fn drop(&mut self) {
        // SAFETY: `previous` is what SelectObject returned for this DC, which is still
        // alive (`MemoryDc` is declared earlier, so it drops later).
        let _ = unsafe { SelectObject(self.dc, self.previous) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> PhysRect {
        PhysRect {
            left,
            top,
            right,
            bottom,
        }
    }

    #[test]
    fn black_is_black_whatever_the_alpha() {
        assert!(is_all_black(&[0, 0, 0, 0, 0, 0, 0, 0xFF]));
        assert!(is_all_black(&[0; 64]));
    }

    #[test]
    fn one_lit_channel_is_not_black() {
        for channel in 0..3 {
            let mut pixels = vec![0u8; 16];
            pixels[8 + channel] = 1;
            assert!(!is_all_black(&pixels), "channel {channel}");
        }
    }

    #[test]
    fn an_empty_buffer_counts_as_black() {
        assert!(is_all_black(&[]));
    }

    #[test]
    fn within_checks_every_edge() {
        let screen = rect(-1920, -200, 1920, 1080);
        assert!(within(rect(-1920, -200, 1920, 1080), screen));
        assert!(within(rect(-100, 0, 100, 50), screen));
        assert!(!within(rect(-1921, 0, 0, 50), screen));
        assert!(!within(rect(0, -201, 50, 50), screen));
        assert!(!within(rect(0, 0, 1921, 50), screen));
        assert!(!within(rect(0, 0, 50, 1081), screen));
    }

    #[test]
    fn empty_and_off_screen_regions_are_refused() {
        assert_eq!(
            capture_region(rect(10, 10, 10, 20)).err(),
            Some(CaptureError::Empty)
        );
        assert_eq!(
            capture_region(rect(20, 20, 10, 30)).err(),
            Some(CaptureError::Empty)
        );
        let far = rect(i32::MIN, i32::MIN, i32::MIN + 10, i32::MIN + 10);
        assert_eq!(capture_region(far).err(), Some(CaptureError::OffScreen));
    }

    #[test]
    fn display_names_the_reason_only() {
        assert_eq!(CaptureError::Gdi("BitBlt").to_string(), "BitBlt failed");
        assert_eq!(
            CaptureError::Black.to_string(),
            "the capture is all black (protected content)"
        );
    }
}
