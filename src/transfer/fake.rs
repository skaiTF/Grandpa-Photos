//! A pretend phone, so the UI and worker can be built and tested without real hardware.

use std::io::Read;
use std::thread;
use std::time::{Duration, SystemTime};

use super::TransferError;
use super::source::{DeviceStatus, MediaId, MediaItem, MediaKind, MediaSource};

const THUMB_SIZE: u32 = 96;

pub struct FakeSource {
    items: Vec<MediaItem>,
    /// Number of `status` calls before the phone appears "plugged in".
    polls_until_plugged: u32,
    /// Number of further `status` calls before "File transfer" mode is chosen.
    polls_until_ready: u32,
    polls: u32,
    /// Pause per chunk read, to imitate USB speed so progress is visible.
    read_delay: Duration,
}

impl FakeSource {
    /// Demo phone: appears after a few seconds, asks for File transfer mode, then
    /// offers 24 items that take a moment each to copy.
    pub fn demo() -> Self {
        Self {
            items: sample_items(24),
            polls_until_plugged: 2,
            polls_until_ready: 2,
            polls: 0,
            read_delay: Duration::from_millis(2),
        }
    }

    /// Already connected and instant. For tests.
    #[cfg(test)]
    pub fn ready(count: usize) -> Self {
        Self {
            items: sample_items(count),
            polls_until_plugged: 0,
            polls_until_ready: 0,
            polls: 0,
            read_delay: Duration::ZERO,
        }
    }
}

impl MediaSource for FakeSource {
    fn status(&mut self) -> DeviceStatus {
        self.polls = self.polls.saturating_add(1);
        let name = "Demo phone".to_string();
        if self.polls <= self.polls_until_plugged {
            DeviceStatus::NotConnected
        } else if self.polls <= self.polls_until_plugged + self.polls_until_ready {
            DeviceStatus::NeedsFileTransferMode { name }
        } else {
            DeviceStatus::Ready { name }
        }
    }

    fn list_media(&mut self) -> Result<Vec<MediaItem>, TransferError> {
        Ok(self.items.clone())
    }

    fn thumbnail(&mut self, id: &MediaId) -> Result<Vec<u8>, TransferError> {
        let index = self.index_of(id)?;
        Ok(solid_bmp(
            THUMB_SIZE,
            THUMB_SIZE,
            PALETTE[index % PALETTE.len()],
        ))
    }

    fn open(&mut self, id: &MediaId) -> Result<Box<dyn Read + '_>, TransferError> {
        let index = self.index_of(id)?;
        Ok(Box::new(FakeReader {
            remaining: self.items[index].size,
            delay: self.read_delay,
        }))
    }
}

impl FakeSource {
    fn index_of(&self, id: &MediaId) -> Result<usize, TransferError> {
        self.items
            .iter()
            .position(|item| &item.id == id)
            .ok_or_else(|| TransferError::NotFound(id.0.clone()))
    }
}

fn sample_items(count: usize) -> Vec<MediaItem> {
    let now = SystemTime::now();
    (0..count)
        .map(|i| {
            let is_video = i % 6 == 5;
            let (kind, ext) = if is_video {
                (MediaKind::Video, "mp4")
            } else {
                (MediaKind::Photo, "jpg")
            };
            MediaItem {
                id: MediaId(format!("fake-{i}")),
                file_name: format!("IMG_{:04}.{ext}", 1000 + i),
                size: 1_000_000 + (i as u64 * 37_000),
                // Spread over the last few days so sorting is visible.
                created: now.checked_sub(Duration::from_secs(i as u64 * 3600 * 5)),
                kind,
            }
        })
        .collect()
}

/// Produces `remaining` bytes of filler data, slowly.
struct FakeReader {
    remaining: u64,
    delay: Duration,
}

impl Read for FakeReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = buf
            .len()
            .min(usize::try_from(self.remaining).unwrap_or(usize::MAX));
        buf[..n].fill(0xAB);
        self.remaining -= n as u64;
        if !self.delay.is_zero() && n > 0 {
            thread::sleep(self.delay);
        }
        Ok(n)
    }
}

const PALETTE: [[u8; 3]; 6] = [
    [231, 111, 81],
    [244, 162, 97],
    [233, 196, 106],
    [42, 157, 143],
    [38, 70, 83],
    [131, 56, 236],
];

/// Encodes a single-colour 24-bit BMP. BMP is the simplest image format to write
/// by hand, and iced can decode it like the JPEG thumbnails a real phone sends.
fn solid_bmp(width: u32, height: u32, [r, g, b]: [u8; 3]) -> Vec<u8> {
    const HEADER_LEN: u32 = 14 + 40;
    let row_len = (width * 3).div_ceil(4) * 4; // rows are padded to 4 bytes
    let data_len = row_len * height;

    let mut out = Vec::with_capacity((HEADER_LEN + data_len) as usize);
    // File header
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(HEADER_LEN + data_len).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // reserved
    out.extend_from_slice(&HEADER_LEN.to_le_bytes()); // offset of pixel data
    // Info header
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // colour planes
    out.extend_from_slice(&24u16.to_le_bytes()); // bits per pixel
    out.extend_from_slice(&[0; 24]); // no compression, default resolution, no palette
    // Pixels, stored as BGR
    for _ in 0..height {
        for _ in 0..width {
            out.extend_from_slice(&[b, g, r]);
        }
        out.resize(out.len() + (row_len - width * 3) as usize, 0);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bmp_has_expected_length() {
        let bmp = solid_bmp(3, 2, [1, 2, 3]);
        // 3 px * 3 bytes = 9, padded to 12 per row; 2 rows; 54 header bytes.
        assert_eq!(bmp.len(), 54 + 24);
        assert_eq!(&bmp[..2], b"BM");
    }

    #[test]
    fn reader_yields_exact_size() {
        let mut source = FakeSource::ready(2);
        let item = source.list_media().unwrap().remove(1);
        let mut reader = source.open(&item.id).unwrap();
        let copied = std::io::copy(&mut reader, &mut std::io::sink()).unwrap();
        assert_eq!(copied, item.size);
    }

    #[test]
    fn demo_walks_through_connection_states() {
        let mut source = FakeSource::demo();
        assert_eq!(source.status(), DeviceStatus::NotConnected);
        assert_eq!(source.status(), DeviceStatus::NotConnected);
        assert!(matches!(
            source.status(),
            DeviceStatus::NeedsFileTransferMode { .. }
        ));
        assert!(matches!(
            source.status(),
            DeviceStatus::NeedsFileTransferMode { .. }
        ));
        assert!(matches!(source.status(), DeviceStatus::Ready { .. }));
    }
}
