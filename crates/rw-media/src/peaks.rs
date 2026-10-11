//! Waveform peaks for drawing audio on the timeline.

use crate::MediaError;
use crate::decode::{AudioReader, CHANNELS, SAMPLE_RATE};
use rw_model::Time;
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

/// Peak values per second of audio.
pub const PEAKS_PER_SECOND: u32 = 100;
const MAGIC: &[u8; 4] = b"RWPK";

/// Loudest absolute sample in each 1/[`PEAKS_PER_SECOND`] s window, mixed
/// to mono, scaled to `0..=255`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Peaks {
    pub per_second: u32,
    pub values: Vec<u8>,
}

impl Peaks {
    pub fn compute(path: &Path, cancel: &AtomicBool) -> Result<Peaks, MediaError> {
        let mut reader = AudioReader::open(path)?;
        let window = (SAMPLE_RATE / PEAKS_PER_SECOND) as usize * CHANNELS;
        let mut buf = vec![0.0f32; window * 256];
        let mut values = Vec::new();
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(MediaError::Cancelled);
            }
            let n = reader.read(&mut buf)?;
            for chunk in buf[..n].chunks(window) {
                let peak = chunk
                    .chunks(CHANNELS)
                    .map(|f| (f.iter().sum::<f32>() / CHANNELS as f32).abs())
                    .fold(0.0f32, f32::max);
                values.push((peak.clamp(0.0, 1.0) * 255.0).round() as u8);
            }
            if n < buf.len() {
                break;
            }
        }
        Ok(Peaks {
            per_second: PEAKS_PER_SECOND,
            values,
        })
    }

    /// Largest peak in `[from, to)`, as `0.0..=1.0`.
    pub fn max_in(&self, from: Time, to: Time) -> f32 {
        let idx = |t: Time| {
            (t.max(Time::ZERO)
                .to_rational(1, self.per_second as i32)
                .max(0) as usize)
                .min(self.values.len())
        };
        let (a, b) = (idx(from), idx(to).max(idx(from) + 1).min(self.values.len()));
        self.values[a.min(b)..b]
            .iter()
            .copied()
            .max()
            .map_or(0.0, |v| f32::from(v) / 255.0)
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("tmp");
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(MAGIC)?;
        f.write_all(&self.per_second.to_le_bytes())?;
        f.write_all(&self.values)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(tmp, path)
    }

    pub fn load(path: &Path) -> io::Result<Peaks> {
        let mut f = std::fs::File::open(path)?;
        let mut head = [0u8; 8];
        f.read_exact(&mut head)?;
        if &head[..4] != MAGIC {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "not a peaks file",
            ));
        }
        let per_second = u32::from_le_bytes(head[4..8].try_into().expect("4 bytes"));
        let mut values = Vec::new();
        f.read_to_end(&mut values)?;
        Ok(Peaks { per_second, values })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_load_and_query() {
        let p = Peaks {
            per_second: 100,
            values: (0..=255).collect(),
        };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x/p.pk");
        p.save(&path).unwrap();
        assert_eq!(Peaks::load(&path).unwrap(), p);
        assert_eq!(p.max_in(Time::ZERO, Time::from_millis(100)), 9.0 / 255.0);
        assert_eq!(p.max_in(Time::from_secs(100), Time::from_secs(200)), 0.0);
        std::fs::write(&path, b"nope1234").unwrap();
        assert!(Peaks::load(&path).is_err());
    }
}
