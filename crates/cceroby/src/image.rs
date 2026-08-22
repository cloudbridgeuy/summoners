//! Typed image classification and the one allowed TIFF conversion.

use std::io::Cursor;

use ::image::ImageFormat;
use ::image::codecs::jpeg::JpegEncoder;
use thiserror::Error;

/// Downloaded bytes classified from their file signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadedImage {
    Jpeg(Vec<u8>),
    Tiff(Vec<u8>),
}

impl DownloadedImage {
    pub fn try_from_magic(bytes: Vec<u8>) -> Result<Self, ImageError> {
        match bytes.as_slice() {
            [0xff, 0xd8, 0xff, ..] => Ok(Self::Jpeg(bytes)),
            [b'I', b'I', 0x2a, 0x00, ..] | [b'M', b'M', 0x00, 0x2a, ..] => Ok(Self::Tiff(bytes)),
            _ => Err(ImageError::UnsupportedFormat),
        }
    }

    /// Return native JPEG bytes unchanged, or convert TIFF pixels once.
    pub fn into_jpeg_quality_100(self) -> Result<Vec<u8>, ImageError> {
        match self {
            Self::Jpeg(bytes) => Ok(bytes),
            Self::Tiff(bytes) => {
                let decoded = ::image::load_from_memory_with_format(&bytes, ImageFormat::Tiff)
                    .map_err(|_| ImageError::ConversionFailed)?;
                let mut jpeg = Cursor::new(Vec::new());
                JpegEncoder::new_with_quality(&mut jpeg, 100)
                    .encode_image(&decoded)
                    .map_err(|_| ImageError::ConversionFailed)?;
                Ok(jpeg.into_inner())
            }
        }
    }
}

/// A downloaded image cannot enter the standard JPEG asset path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ImageError {
    #[error("the image format is not supported")]
    UnsupportedFormat,
    #[error("the image could not be converted")]
    ConversionFailed,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    fn fixture(raw: &str) -> Vec<u8> {
        raw.trim()
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let text = std::str::from_utf8(pair).expect("fixture is ASCII");
                u8::from_str_radix(text, 16).expect("fixture contains hexadecimal bytes")
            })
            .collect()
    }

    fn native_jpeg() -> Vec<u8> {
        fixture(include_str!("../tests/fixtures/images/native-jpeg.hex"))
    }

    fn source_tiff() -> Vec<u8> {
        fixture(include_str!("../tests/fixtures/images/source-tiff.hex"))
    }

    #[test]
    fn magic_classifier_accepts_little_and_big_endian_tiff_and_jpeg() {
        assert!(matches!(
            DownloadedImage::try_from_magic(native_jpeg()),
            Ok(DownloadedImage::Jpeg(_))
        ));
        assert!(matches!(
            DownloadedImage::try_from_magic(source_tiff()),
            Ok(DownloadedImage::Tiff(_))
        ));
        assert!(matches!(
            DownloadedImage::try_from_magic(vec![b'M', b'M', 0, 0x2a, 1]),
            Ok(DownloadedImage::Tiff(_))
        ));
    }

    #[test]
    fn magic_classifier_rejects_other_and_truncated_data() {
        for bytes in [vec![], vec![0xff], vec![0xff, 0xd8], b"PNG".to_vec()] {
            assert_eq!(
                DownloadedImage::try_from_magic(bytes),
                Err(ImageError::UnsupportedFormat)
            );
        }
    }

    #[test]
    fn native_jpeg_bytes_are_returned_byte_for_byte() {
        let bytes = native_jpeg();
        let output = DownloadedImage::try_from_magic(bytes.clone())
            .expect("JPEG is recognized")
            .into_jpeg_quality_100()
            .expect("JPEG passes through");
        assert_eq!(output, bytes);
    }

    #[test]
    fn tiff_is_converted_once_to_a_decodable_quality_100_jpeg() {
        let output = DownloadedImage::try_from_magic(source_tiff())
            .expect("TIFF is recognized")
            .into_jpeg_quality_100()
            .expect("TIFF converts");
        assert!(matches!(output.as_slice(), [0xff, 0xd8, 0xff, ..]));
        let decoded = ::image::load_from_memory_with_format(&output, ImageFormat::Jpeg)
            .expect("output JPEG decodes");
        assert_eq!((decoded.width(), decoded.height()), (2, 2));
    }

    #[test]
    fn malformed_tiff_returns_a_typed_conversion_error() {
        assert_eq!(
            DownloadedImage::try_from_magic(vec![b'I', b'I', 0x2a, 0, 1])
                .expect("magic is TIFF")
                .into_jpeg_quality_100(),
            Err(ImageError::ConversionFailed)
        );
    }
}
