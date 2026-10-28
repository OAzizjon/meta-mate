//! The local clock: names of files the person sees use local time, not UTC.

use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
use windows::Win32::System::SystemInformation::GetSystemTime;
use windows::Win32::System::Time::{SystemTimeToFileTime, SystemTimeToTzSpecificLocalTime};

/// Seconds the local clock is ahead of UTC right now (daylight saving included); 0 when
/// Windows cannot say.
pub fn local_offset_seconds() -> i64 {
    // SAFETY: GetSystemTime only writes the returned struct.
    let utc = unsafe { GetSystemTime() };
    let mut local = SYSTEMTIME::default();
    // SAFETY: both pointers are to live stack values; None is the current time zone.
    if unsafe { SystemTimeToTzSpecificLocalTime(None, &utc, &mut local) }.is_err() {
        return 0;
    }
    match (ticks(&utc), ticks(&local)) {
        // FILETIME ticks are 100 ns.
        (Some(utc), Some(local)) => (local - utc) / 10_000_000,
        _ => 0,
    }
}

fn ticks(time: &SYSTEMTIME) -> Option<i64> {
    let mut file = FILETIME::default();
    // SAFETY: both pointers are to live values.
    unsafe { SystemTimeToFileTime(time, &mut file) }.ok()?;
    let ticks = (u64::from(file.dwHighDateTime) << 32) | u64::from(file.dwLowDateTime);
    i64::try_from(ticks).ok()
}

#[cfg(test)]
mod tests {
    use super::local_offset_seconds;

    #[test]
    fn the_offset_is_a_real_time_zone() {
        let offset = local_offset_seconds();
        // UTC-12 .. UTC+14, in whole quarter hours.
        assert!((-12 * 3600..=14 * 3600).contains(&offset), "{offset}");
        assert_eq!(offset % 900, 0, "{offset}");
    }
}
