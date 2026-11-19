//! ID3 tags of the finished episode (ID3v2.3 for podcast client compatibility).

use crate::error::{CoreError, ErrorKind, Result};
use id3::frame::{Picture, PictureType};
use id3::{Tag, TagLike, Version};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const DEFAULT_ARTIST: &str = "Społeczność Chrześcijańska Tomy";
pub const DEFAULT_ALBUM: &str = "SCh Tomy podcast";

/// Cover images larger than this are rejected (podcast covers are < 1 MB).
const MAX_COVER_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EpisodeTags {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub year: Option<i32>,
    pub cover: Option<PathBuf>,
}

/// Reads and validates a JPEG cover.
pub fn read_cover(path: &Path) -> Result<Vec<u8>> {
    let meta = std::fs::metadata(path).map_err(|e| {
        CoreError::new(
            ErrorKind::CoverUnreadable,
            format!("{}: {e}", path.display()),
        )
    })?;
    if meta.len() > MAX_COVER_BYTES {
        return Err(CoreError::new(
            ErrorKind::CoverUnreadable,
            "cover larger than 10 MB",
        ));
    }
    let data = std::fs::read(path).map_err(|e| {
        CoreError::new(
            ErrorKind::CoverUnreadable,
            format!("{}: {e}", path.display()),
        )
    })?;
    if data.len() < 4 || data[0..3] != [0xFF, 0xD8, 0xFF] {
        return Err(CoreError::new(
            ErrorKind::CoverUnreadable,
            "cover is not a JPEG file",
        ));
    }
    Ok(data)
}

pub fn write_tags(file: &Path, tags: &EpisodeTags) -> Result<()> {
    let mut tag = Tag::new();
    tag.set_title(tags.title.trim());
    tag.set_artist(tags.artist.trim());
    tag.set_album(tags.album.trim());
    if let Some(y) = tags.year {
        tag.set_year(y);
    }
    tag.set_genre("Podcast");
    if let Some(cover) = &tags.cover {
        let data = read_cover(cover)?;
        tag.add_frame(Picture {
            mime_type: "image/jpeg".to_string(),
            picture_type: PictureType::CoverFront,
            description: "Cover".to_string(),
            data,
        });
    }
    tag.write_to_path(file, Version::Id3v23).map_err(|e| {
        CoreError::new(
            ErrorKind::OutputNotWritable,
            format!("writing ID3 tags: {e}"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_jpeg_cover() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("okładka.png");
        std::fs::write(&p, b"\x89PNG\r\n\x1a\n....").unwrap();
        assert_eq!(read_cover(&p).unwrap_err().kind, ErrorKind::CoverUnreadable);
        assert_eq!(
            read_cover(&dir.path().join("brak.jpg")).unwrap_err().kind,
            ErrorKind::CoverUnreadable
        );
    }
}
