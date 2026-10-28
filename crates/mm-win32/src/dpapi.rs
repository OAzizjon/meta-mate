//! Secrets at rest for this Windows user only (DPAPI): the sign-in session (D54). Another
//! user of the computer, or a copy of the file on another computer, cannot read them.

use windows::Win32::Foundation::{HLOCAL, LocalFree};
use windows::Win32::Security::Cryptography::{
    CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
};
use windows::core::PCWSTR;

/// `data` sealed for the current user; None when Windows refused.
pub fn protect(data: &[u8]) -> Option<Vec<u8>> {
    let input = blob(data)?;
    let mut output = CRYPT_INTEGER_BLOB::default();
    // SAFETY: `input` points into `data`, alive for the call; no description, entropy or
    // prompt; Windows allocates `output`, which `take` copies and frees.
    unsafe {
        CryptProtectData(
            &input,
            PCWSTR::null(),
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    }
    .ok()?;
    // SAFETY: `output` was filled by a successful CryptProtectData.
    Some(unsafe { take(output) })
}

/// What `protect` sealed for this user; None for anything else (another user's file,
/// tampered bytes).
pub fn unprotect(sealed: &[u8]) -> Option<Vec<u8>> {
    let input = blob(sealed)?;
    let mut output = CRYPT_INTEGER_BLOB::default();
    // SAFETY: as in `protect`.
    unsafe {
        CryptUnprotectData(
            &input,
            None,
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    }
    .ok()?;
    // SAFETY: `output` was filled by a successful CryptUnprotectData.
    Some(unsafe { take(output) })
}

fn blob(data: &[u8]) -> Option<CRYPT_INTEGER_BLOB> {
    Some(CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(data.len()).ok()?,
        // DPAPI only reads the input.
        pbData: data.as_ptr().cast_mut(),
    })
}

/// Copies a blob Windows allocated, then frees it.
///
/// # Safety
/// `blob` must come from a successful DPAPI call and not be used afterwards.
unsafe fn take(blob: CRYPT_INTEGER_BLOB) -> Vec<u8> {
    let bytes = if blob.pbData.is_null() || blob.cbData == 0 {
        Vec::new()
    } else {
        // SAFETY: DPAPI returned `cbData` readable bytes at `pbData`.
        unsafe { std::slice::from_raw_parts(blob.pbData, blob.cbData as usize) }.to_vec()
    };
    if !blob.pbData.is_null() {
        // SAFETY: DPAPI allocated `pbData` with LocalAlloc; it is freed once, here.
        unsafe { LocalFree(Some(HLOCAL(blob.pbData.cast()))) };
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::{protect, unprotect};

    #[test]
    fn sealed_bytes_open_only_untouched() {
        let secret = b"mms1.body.mac";
        let sealed = protect(secret).expect("DPAPI works for the test user");
        assert!(!sealed.windows(secret.len()).any(|window| window == secret));
        assert_eq!(unprotect(&sealed).as_deref(), Some(&secret[..]));
        let mut tampered = sealed.clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 0x55;
        assert_eq!(unprotect(&tampered), None);
        assert_eq!(unprotect(b"not sealed"), None);
    }
}
