//! Core image transformations for painless-ghicon.
//!
//! Rounds the corners of the colored block pattern in GitHub identicon-like
//! two-color images. The square opaque canvas is left untouched; only the
//! shape of the foreground blocks changes.

mod github;
mod pattern;

use std::io::Cursor;

pub use github::resolve_avatar_url;
pub use pattern::round_pattern;

/// Default corner radius as a fraction of the detected cell size.
pub const DEFAULT_RADIUS_RATIO: f32 = 0.4;

/// Errors returned by this crate.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The input bytes could not be decoded as an image.
    #[error("failed to decode image: {0}")]
    Decode(#[source] image::ImageError),
    /// The result could not be encoded as PNG.
    #[error("failed to encode image: {0}")]
    Encode(#[source] image::ImageError),
    /// The radius ratio is outside the accepted range.
    #[error("radius ratio must be within (0.0, 0.5], got {0}")]
    InvalidRatio(f32),
    /// The avatar source is not a GitHub user ID or an allowed GitHub URL.
    #[error("cannot resolve avatar source {input:?}: {reason}")]
    InvalidSource {
        /// The offending input.
        input: String,
        /// Why it was rejected.
        reason: String,
    },
}

/// Result of converting an image binary with [`round_image_bytes`].
pub struct RoundedImage {
    /// PNG-encoded output image.
    pub png: Vec<u8>,
    /// Whether a two-color block pattern was detected and rounded. When
    /// `false`, `png` is the decoded input re-encoded unchanged.
    pub pattern_detected: bool,
}

/// Decodes an image binary (PNG or JPEG), rounds the corners of its block
/// pattern by `radius_ratio` of the detected cell size, and re-encodes the
/// result as PNG. Images without a detectable two-color pattern are
/// re-encoded unchanged, reported via [`RoundedImage::pattern_detected`].
///
/// # Errors
///
/// Returns [`Error::InvalidRatio`] when `radius_ratio` is outside (0.0, 0.5],
/// [`Error::Decode`] when `bytes` is not a decodable image, and
/// [`Error::Encode`] when the result cannot be encoded as PNG.
pub fn round_image_bytes(bytes: &[u8], radius_ratio: f32) -> Result<RoundedImage, Error> {
    if !(radius_ratio > 0.0 && radius_ratio <= 0.5) {
        return Err(Error::InvalidRatio(radius_ratio));
    }
    let rgba = image::load_from_memory(bytes)
        .map_err(Error::Decode)?
        .to_rgba8();
    let (result, pattern_detected) = match round_pattern(&rgba, radius_ratio) {
        Some(rounded) => (rounded, true),
        None => (rgba, false),
    };
    let mut png = Vec::new();
    result
        .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(Error::Encode)?;
    Ok(RoundedImage {
        png,
        pattern_detected,
    })
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_RADIUS_RATIO, Error, round_image_bytes};
    use image::{Rgba, RgbaImage};
    use std::io::Cursor;

    fn png_bytes(img: &RgbaImage) -> Vec<u8> {
        let mut bytes = Vec::new();
        img.write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
            .map_err(|e| format!("encoding test image failed: {e}"))
            .unwrap_or_default();
        assert!(!bytes.is_empty(), "test image must encode");
        bytes
    }

    fn block_image() -> RgbaImage {
        let mut img = RgbaImage::from_pixel(210, 210, Rgba([240, 240, 240, 255]));
        for y in 70..140 {
            for x in 70..140 {
                img.put_pixel(x, y, Rgba([50, 100, 200, 255]));
            }
        }
        img
    }

    #[test]
    fn bytes_roundtrip_rounds_the_pattern() {
        let bytes = png_bytes(&block_image());
        let Ok(rounded) = round_image_bytes(&bytes, DEFAULT_RADIUS_RATIO) else {
            panic!("conversion should succeed");
        };
        assert!(rounded.pattern_detected);
        let Ok(out) = image::load_from_memory(&rounded.png) else {
            panic!("output should be a decodable PNG");
        };
        let out = out.to_rgba8();
        assert_eq!(out.dimensions(), (210, 210));
        assert_eq!(out.get_pixel(70, 70).0, [240, 240, 240, 255]);
        assert_eq!(out.get_pixel(105, 105).0, [50, 100, 200, 255]);
    }

    #[test]
    fn patternless_image_is_reencoded_unchanged() {
        let img = RgbaImage::from_pixel(64, 64, Rgba([240, 240, 240, 255]));
        let Ok(rounded) = round_image_bytes(&png_bytes(&img), DEFAULT_RADIUS_RATIO) else {
            panic!("conversion should succeed");
        };
        assert!(!rounded.pattern_detected);
        let Ok(out) = image::load_from_memory(&rounded.png) else {
            panic!("output should be a decodable PNG");
        };
        assert_eq!(out.to_rgba8().get_pixel(32, 32).0, [240, 240, 240, 255]);
    }

    #[test]
    fn invalid_ratio_and_bytes_are_rejected() {
        let bytes = png_bytes(&block_image());
        assert!(matches!(
            round_image_bytes(&bytes, 0.0),
            Err(Error::InvalidRatio(_))
        ));
        assert!(matches!(
            round_image_bytes(&bytes, 0.51),
            Err(Error::InvalidRatio(_))
        ));
        assert!(matches!(
            round_image_bytes(b"not an image", DEFAULT_RADIUS_RATIO),
            Err(Error::Decode(_))
        ));
    }
}
