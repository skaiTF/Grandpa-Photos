//! The `MediaSource` trait: the one interface the rest of the app uses to read a phone.
//!
//! `FakeSource` implements it for development and tests; the real Windows Portable
//! Devices implementation (`WpdSource`) arrives in milestone 3.

use std::io::Read;
use std::time::SystemTime;

use super::TransferError;

/// Opaque identifier of a file on the phone (an MTP object id for the real device).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MediaId(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Photo,
    Video,
}

/// One photo or video on the phone.
#[derive(Debug, Clone)]
pub struct MediaItem {
    pub id: MediaId,
    pub file_name: String,
    /// Size in bytes as reported by the phone. Used to verify the copy.
    pub size: u64,
    pub created: Option<SystemTime>,
    pub kind: MediaKind,
}

/// What the app can currently see of the phone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceStatus {
    NotConnected,
    /// The phone is plugged in but its files are not visible: it is locked or in
    /// "charging only" USB mode.
    NeedsFileTransferMode {
        name: String,
    },
    Ready {
        name: String,
    },
}

/// A phone (or a stand-in for one) that photos can be read from.
///
/// All methods block; they are only ever called from the worker thread.
pub trait MediaSource {
    fn status(&mut self) -> DeviceStatus;
    fn list_media(&mut self) -> Result<Vec<MediaItem>, TransferError>;
    /// Small preview image, encoded (JPEG/PNG/BMP bytes).
    fn thumbnail(&mut self, id: &MediaId) -> Result<Vec<u8>, TransferError>;
    /// Opens the full file for streaming. Never read it all into memory.
    fn open(&mut self, id: &MediaId) -> Result<Box<dyn Read + '_>, TransferError>;
}
