//! The memory of the process as `/proc/self/status` says it. Pure text work, so the tests run on
//! every OS; only the backend reads the file.

use crate::platform::MemoryUse;

/// Reads `VmRSS`, `VmHWM` and `RssAnon` (all in kB) from the text of `/proc/self/status`.
/// `footprint` is the anonymous part of the resident memory (the heap and the decoded images:
/// what the program holds), without the file-backed pages of the libraries; a kernel that does
/// not list it gets the whole resident size. None when `VmRSS` is missing.
pub fn parse_status(text: &str) -> Option<MemoryUse> {
    let bytes = |key: &str| -> Option<u64> {
        text.lines().find_map(|line| {
            let rest = line.strip_prefix(key)?.trim().strip_suffix("kB")?.trim();
            rest.parse::<u64>().ok().map(|kb| kb * 1024)
        })
    };
    let resident = bytes("VmRSS:")?;
    Some(MemoryUse {
        resident,
        peak: bytes("VmHWM:").unwrap_or(resident),
        footprint: bytes("RssAnon:").unwrap_or(resident),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATUS: &str = "Name:\tcursorpie\nVmPeak:\t  900000 kB\nVmHWM:\t  200000 kB\nVmRSS:\t  150000 kB\nRssAnon:\t   90000 kB\nRssFile:\t   60000 kB\nThreads:\t5\n";

    #[test]
    fn the_three_numbers_are_read_in_bytes() {
        let m = parse_status(STATUS).unwrap();
        assert_eq!(m.resident, 150_000 * 1024);
        assert_eq!(m.peak, 200_000 * 1024);
        assert_eq!(m.footprint, 90_000 * 1024);
    }

    #[test]
    fn a_kernel_without_rssanon_gets_the_resident_size() {
        let m = parse_status("VmRSS:\t 1000 kB\n").unwrap();
        assert_eq!(
            (m.resident, m.peak, m.footprint),
            (1_024_000, 1_024_000, 1_024_000)
        );
    }

    #[test]
    fn text_without_vmrss_is_none() {
        assert!(parse_status("Name:\tx\n").is_none());
        assert!(parse_status("").is_none());
    }
}
