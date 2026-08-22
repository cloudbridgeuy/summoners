//! Pure download input, state, and operator notice types.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::artwork::{ArtworkKey, ArtworkKeyError};
use crate::core::Artwork;

const MAX_SLUG_CHARACTERS: usize = 60;

/// One file-name stem that cannot escape the selected output directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slug(String);

impl Slug {
    pub fn parse(raw: &str) -> Result<Self, SlugError> {
        if raw.contains(['/', '\\']) || raw.contains("..") {
            return Err(SlugError::UnsafePath);
        }
        normalized_slug(raw).map(Self).ok_or(SlugError::Empty)
    }

    /// Build a safe initial value from trusted artwork metadata.
    #[must_use]
    pub fn from_title(title: &str) -> Self {
        normalized_slug(title).map_or_else(|| Self("artwork".into()), Self)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A proposed slug cannot become a safe file-name stem.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum SlugError {
    #[error("the file name must contain letters or numbers")]
    Empty,
    #[error("the file name must not contain a path")]
    UnsafePath,
}

/// Ordered, non-empty, unique free-form subject tags.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tags(Vec<String>);

impl Tags {
    #[must_use]
    pub fn parse(raw: &str) -> Self {
        let mut seen = HashSet::new();
        let values = raw
            .split([',', '\n', '\r'])
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .filter(|value| seen.insert((*value).to_owned()))
            .map(str::to_owned)
            .collect();
        Self(values)
    }

    #[must_use]
    pub fn as_slice(&self) -> &[String] {
        &self.0
    }
}

/// The complete and only browser-controlled download form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadForm {
    pub source: String,
    pub id: String,
    pub slug: String,
    pub tags: String,
}

/// Parsed browser input. All other download data must come from providers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadRequest {
    pub key: ArtworkKey,
    pub slug: Slug,
    pub tags: Tags,
}

/// Trusted provider data and parsed local values for one asset operation.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DownloadJob<'a> {
    pub(crate) artwork: &'a Artwork,
    pub(crate) attribution: &'a str,
    pub(crate) slug: &'a Slug,
    pub(crate) tags: &'a Tags,
    pub(crate) output: &'a Path,
}

impl TryFrom<DownloadForm> for DownloadRequest {
    type Error = DownloadValidationError;

    fn try_from(form: DownloadForm) -> Result<Self, Self::Error> {
        Ok(Self {
            key: ArtworkKey::try_from_parts(&form.source, &form.id)?,
            slug: Slug::parse(&form.slug)?,
            tags: Tags::parse(&form.tags),
        })
    }
}

impl DownloadRequest {
    /// Strictly decode and validate the complete browser form before any I/O.
    pub fn parse_urlencoded(bytes: &[u8]) -> Result<Self, DownloadValidationError> {
        DownloadForm::parse_urlencoded(bytes)?.try_into()
    }
}

impl DownloadForm {
    fn parse_urlencoded(bytes: &[u8]) -> Result<Self, DownloadFormError> {
        let raw = std::str::from_utf8(bytes).map_err(|_| DownloadFormError::Malformed)?;
        if raw.is_empty() {
            return Err(DownloadFormError::Malformed);
        }
        let mut source = None;
        let mut id = None;
        let mut slug = None;
        let mut tags = None;
        for field in raw.split('&') {
            let (raw_name, raw_value) = field
                .split_once('=')
                .filter(|(name, _)| !name.is_empty())
                .ok_or(DownloadFormError::Malformed)?;
            let name = decode_component(raw_name)?;
            let value = decode_component(raw_value)?;
            let slot = match name.as_str() {
                "source" => &mut source,
                "id" => &mut id,
                "slug" => &mut slug,
                "tags" => &mut tags,
                _ => return Err(DownloadFormError::UnknownField),
            };
            if slot.replace(value).is_some() {
                return Err(DownloadFormError::DuplicateField);
            }
        }
        Ok(Self {
            source: source.ok_or(DownloadFormError::MissingField)?,
            id: id.ok_or(DownloadFormError::MissingField)?,
            slug: slug.ok_or(DownloadFormError::MissingField)?,
            tags: tags.ok_or(DownloadFormError::MissingField)?,
        })
    }
}

fn decode_component(raw: &str) -> Result<String, DownloadFormError> {
    let bytes = raw.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut position = 0;
    while position < bytes.len() {
        match bytes[position] {
            b'+' => {
                decoded.push(b' ');
                position += 1;
            }
            b'%' => {
                let high = bytes
                    .get(position + 1)
                    .copied()
                    .and_then(hex_value)
                    .ok_or(DownloadFormError::Malformed)?;
                let low = bytes
                    .get(position + 2)
                    .copied()
                    .and_then(hex_value)
                    .ok_or(DownloadFormError::Malformed)?;
                decoded.push((high << 4) | low);
                position += 3;
            }
            byte => {
                decoded.push(byte);
                position += 1;
            }
        }
    }
    String::from_utf8(decoded).map_err(|_| DownloadFormError::Malformed)
}

#[must_use]
const fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// The URL-encoded form envelope is incomplete or not canonical.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum DownloadFormError {
    #[error("the download form is malformed")]
    Malformed,
    #[error("the download form contains a duplicate field")]
    DuplicateField,
    #[error("the download form contains an unknown field")]
    UnknownField,
    #[error("the download form is missing a field")]
    MissingField,
}

/// A browser-provided download value is invalid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum DownloadValidationError {
    #[error(transparent)]
    Form(#[from] DownloadFormError),
    #[error("the artwork identity is invalid")]
    Artwork(#[from] ArtworkKeyError),
    #[error(transparent)]
    Slug(#[from] SlugError),
}

/// Whether an atomic write created or replaced the target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteDisposition {
    Created,
    Replaced,
}

/// One successful local asset write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedAsset {
    pub path: PathBuf,
    pub disposition: WriteDisposition,
}

/// A short download failure that cannot expose remote or storage details.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum DownloadError {
    #[error("the download form is invalid")]
    Validation,
    #[error("the artwork is not available from its source")]
    Provider,
    #[error("the source image is not a supported JPEG or TIFF")]
    UnsupportedFormat,
    #[error("the source image could not be converted")]
    Image,
    #[error("the image metadata could not be built")]
    Xmp,
    #[error("the JPEG could not be written")]
    Write,
}

impl From<DownloadValidationError> for DownloadError {
    fn from(_: DownloadValidationError) -> Self {
        Self::Validation
    }
}

/// A typed result rendered on the trusted detail page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadNotice {
    Created { path: PathBuf },
    Replaced { path: PathBuf },
    Error(DownloadError),
}

impl DownloadNotice {
    #[must_use]
    pub fn from_result(result: Result<SavedAsset, DownloadError>) -> Self {
        match result {
            Ok(SavedAsset {
                path,
                disposition: WriteDisposition::Created,
            }) => Self::Created { path },
            Ok(SavedAsset {
                path,
                disposition: WriteDisposition::Replaced,
            }) => Self::Replaced { path },
            Err(error) => Self::Error(error),
        }
    }

    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::Created { path } => format!("Created {}.", path.display()),
            Self::Replaced { path } => format!("Replaced {}.", path.display()),
            Self::Error(error) => error.to_string(),
        }
    }

    #[must_use]
    pub const fn is_error(&self) -> bool {
        matches!(self, Self::Error(_))
    }
}

fn normalized_slug(raw: &str) -> Option<String> {
    let mut slug = String::new();
    let mut separator_pending = false;
    let mut character_count = 0;
    for character in raw.trim().chars().flat_map(char::to_lowercase) {
        if character.is_alphanumeric() {
            if separator_pending && !slug.is_empty() {
                if character_count == MAX_SLUG_CHARACTERS {
                    break;
                }
                slug.push('-');
                character_count += 1;
            }
            if character_count == MAX_SLUG_CHARACTERS {
                break;
            }
            slug.push(character);
            character_count += 1;
            separator_pending = false;
        } else {
            separator_pending = true;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    (!slug.is_empty()).then_some(slug)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn slug_parser_normalizes_unicode_whitespace_and_hyphens() {
        let slug = Slug::parse("  Máscara -- ritual\n azul  ").expect("slug is valid");
        assert_eq!(slug.as_str(), "máscara-ritual-azul");
    }

    #[test]
    fn slug_parser_rejects_traversal_separators_and_empty_results() {
        for raw in ["../mask", "mask/one", "mask\\one", "mask..one"] {
            assert_eq!(Slug::parse(raw), Err(SlugError::UnsafePath), "{raw}");
        }
        for raw in ["", "   ", "---", "..."] {
            assert!(Slug::parse(raw).is_err(), "{raw}");
        }
    }

    #[test]
    fn slug_parser_caps_output_at_sixty_characters_without_a_trailing_hyphen() {
        let slug = Slug::parse(&format!("{} next", "é".repeat(60))).expect("slug is valid");
        assert_eq!(slug.as_str().chars().count(), MAX_SLUG_CHARACTERS);
        assert!(!slug.as_str().ends_with('-'));
        let separated = Slug::parse(&format!("{} next", "x".repeat(59))).expect("slug is valid");
        assert!(separated.as_str().chars().count() <= MAX_SLUG_CHARACTERS);
        assert!(!separated.as_str().ends_with('-'));
    }

    #[test]
    fn title_slug_is_safe_and_has_a_nonempty_fallback() {
        assert_eq!(Slug::from_title("Mask / Figure").as_str(), "mask-figure");
        assert_eq!(Slug::from_title("...").as_str(), "artwork");
    }

    #[test]
    fn tag_parser_trims_drops_empty_values_preserves_order_and_deduplicates() {
        let tags = Tags::parse(" mask, ritual\nmask\r\n blue ,, ritual ");
        assert_eq!(tags.as_slice(), ["mask", "ritual", "blue"]);
    }

    #[test]
    fn form_conversion_keeps_only_typed_identity_slug_and_tags() {
        let request = DownloadRequest::parse_urlencoded(
            b"source=aic&id=1001&slug=+Ritual+Mask+&tags=mask%2C+ritual",
        )
        .expect("form is valid");
        assert_eq!(request.key.source().key(), "aic");
        assert_eq!(request.key.id().as_str(), "1001");
        assert_eq!(request.slug.as_str(), "ritual-mask");
        assert_eq!(request.tags.as_slice(), ["mask", "ritual"]);
    }

    #[test]
    fn form_parser_decodes_exactly_the_four_browser_fields() {
        assert_eq!(
            DownloadForm::parse_urlencoded(
                b"source=aic&id=1001&slug=Ritual+Mask&tags=mask%2C+ritual",
            ),
            Ok(DownloadForm {
                source: "aic".into(),
                id: "1001".into(),
                slug: "Ritual Mask".into(),
                tags: "mask, ritual".into(),
            })
        );
    }

    #[test]
    fn form_component_decoder_is_strict_about_percent_and_utf8_bytes() {
        assert_eq!(
            decode_component("C%C3%B4te+d%27Ivoire"),
            Ok("Côte d'Ivoire".into())
        );
        for malformed in ["%", "%0", "%GG", "%ff"] {
            assert_eq!(
                decode_component(malformed),
                Err(DownloadFormError::Malformed)
            );
        }
    }

    #[test]
    fn hexadecimal_decoder_accepts_both_cases_and_rejects_other_bytes() {
        assert_eq!(hex_value(b'0'), Some(0));
        assert_eq!(hex_value(b'9'), Some(9));
        assert_eq!(hex_value(b'a'), Some(10));
        assert_eq!(hex_value(b'F'), Some(15));
        assert_eq!(hex_value(b'g'), None);
    }

    #[test]
    fn strict_form_parser_rejects_unknown_duplicate_missing_and_malformed_fields() {
        for raw in [
            "source=aic&id=1&slug=mask&tags=&url=https%3A%2F%2Fevil.test",
            "source=aic&id=1&slug=mask&slug=other&tags=",
            "source=aic&id=1&slug=mask&tags=&license=CC0",
            "source=aic&id=1&slug=mask&tags=&attribution=evil",
            "source=aic&id=1&slug=mask&tags=&output_path=%2Ftmp%2Fevil",
            "source=aic&id=1&slug=mask&tags=&remote_request=evil",
            "source=aic&id=1&slug=mask",
            "source=aic&id=1&slug=mask%&tags=",
            "source=aic&id=1&slug=mask%GG&tags=",
            "source=aic&id=1&slug=mask%ff&tags=",
        ] {
            assert!(
                DownloadRequest::parse_urlencoded(raw.as_bytes()).is_err(),
                "{raw}"
            );
        }
        assert!(DownloadRequest::parse_urlencoded(b"source=aic&id=1&slug=mask&tags=\xff").is_err());
    }

    #[test]
    fn notice_decision_keeps_created_replaced_and_each_error_distinct() {
        let path = PathBuf::from("out/mask.jpg");
        assert_eq!(
            DownloadNotice::from_result(Ok(SavedAsset {
                path: path.clone(),
                disposition: WriteDisposition::Created,
            }))
            .message(),
            "Created out/mask.jpg."
        );
        assert_eq!(
            DownloadNotice::from_result(Ok(SavedAsset {
                path: path.clone(),
                disposition: WriteDisposition::Replaced,
            }))
            .message(),
            "Replaced out/mask.jpg."
        );
        for error in [
            DownloadError::Validation,
            DownloadError::Provider,
            DownloadError::UnsupportedFormat,
            DownloadError::Image,
            DownloadError::Xmp,
            DownloadError::Write,
        ] {
            let notice = DownloadNotice::from_result(Err(error));
            assert!(notice.is_error());
            assert_eq!(notice.message(), error.to_string());
        }
    }
}
