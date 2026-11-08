// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use std::io::{Read, Write};
pub(super) const EXPECTED_PACKAGE_BYTES: u64 = 1_500_000_000;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    Download,
    Verify,
    Extract,
    Install,
    Complete,
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct PackageProgress {
    pub phase: Phase,
    pub completed: u64,
    pub total: u64,
}
impl PackageProgress {
    pub fn fraction(self) -> f32 {
        if self.phase == Phase::Complete {
            return 1.0;
        }
        if self.phase != Phase::Download {
            return 0.99;
        }
        (self.completed as f64 / self.total.max(1) as f64).clamp(0.0, 0.99) as f32
    }
}
pub(super) fn copy_measured(
    reader: &mut impl Read,
    writer: &mut impl Write,
    mut notify: impl FnMut(u64),
) -> std::io::Result<u64> {
    let mut buffer = [0u8; 128 * 1024];
    let mut completed = 0;
    let mut last = std::time::Instant::now();
    notify(0);
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        writer.write_all(&buffer[..count])?;
        completed += count as u64;
        if last.elapsed() >= std::time::Duration::from_millis(75) {
            notify(completed);
            last = std::time::Instant::now();
        }
    }
    writer.flush()?;
    notify(completed);
    Ok(completed)
}
#[cfg(test)]
mod package_progress_tests {
    use super::*;
    #[test]
    fn package_progress_counts_bytes_and_propagates_failed_writes() {
        assert_eq!(
            PackageProgress {
                phase: Phase::Download,
                completed: 750_000_000,
                total: EXPECTED_PACKAGE_BYTES
            }
            .fraction(),
            0.5
        );
        struct FailedWriter;
        impl Write for FailedWriter {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("disk write failed"))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut observed = Vec::new();
        let error = copy_measured(
            &mut std::io::Cursor::new(vec![1u8; 32]),
            &mut FailedWriter,
            |bytes| observed.push(bytes),
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "disk write failed");
        assert_eq!(observed, vec![0]);
    }
    #[test]
    fn package_progress_estimate_never_completes_before_installation() {
        let mut reader = std::io::Cursor::new(vec![42u8; 300_000]);
        let mut output = Vec::new();
        let mut observed = Vec::new();
        assert_eq!(
            copy_measured(&mut reader, &mut output, |bytes| observed.push(bytes)).unwrap(),
            300_000
        );
        assert_eq!(output.len(), 300_000);
        assert_eq!(observed.first(), Some(&0));
        assert_eq!(observed.last(), Some(&300_000));
        assert!(observed.windows(2).all(|v| v[0] <= v[1]));
        for phase in [
            Phase::Download,
            Phase::Verify,
            Phase::Extract,
            Phase::Install,
        ] {
            assert!(
                PackageProgress {
                    phase,
                    completed: EXPECTED_PACKAGE_BYTES * 2,
                    total: EXPECTED_PACKAGE_BYTES
                }
                .fraction()
                    < 1.0
            );
        }
        assert_eq!(
            PackageProgress {
                phase: Phase::Complete,
                completed: 0,
                total: 0
            }
            .fraction(),
            1.0
        );
    }
}
