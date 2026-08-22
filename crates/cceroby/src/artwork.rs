//! Trusted artwork identity, reconstruction errors, and attribution text.

use thiserror::Error;

use crate::core::{Artwork, SourceKind};

const MAX_OBJECT_ID_LENGTH: usize = 240;

/// One provider object identifier that is safe to use as local route data.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ArtworkId(String);

impl ArtworkId {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The only browser-provided identity accepted by artwork routes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ArtworkKey {
    source: SourceKind,
    id: ArtworkId,
}

impl ArtworkKey {
    pub fn try_from_parts(source: &str, id: &str) -> Result<Self, ArtworkKeyError> {
        let source = parse_source(source)?;
        let id = parse_artwork_id(source, id)?;
        Ok(Self { source, id })
    }

    #[must_use]
    pub const fn source(&self) -> SourceKind {
        self.source
    }

    #[must_use]
    pub fn id(&self) -> &ArtworkId {
        &self.id
    }
}

/// A browser-provided artwork identity is not safe or known.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ArtworkKeyError {
    #[error("the artwork source is not known")]
    UnknownSource,
    #[error("the artwork ID is malformed")]
    MalformedId,
}

fn parse_source(raw: &str) -> Result<SourceKind, ArtworkKeyError> {
    SourceKind::ALL
        .into_iter()
        .find(|source| source.key() == raw)
        .ok_or(ArtworkKeyError::UnknownSource)
}

fn parse_artwork_id(source: SourceKind, raw: &str) -> Result<ArtworkId, ArtworkKeyError> {
    let valid_length = !raw.is_empty() && raw.len() <= MAX_OBJECT_ID_LENGTH;
    let safe_characters = raw.chars().all(|character| {
        !character.is_control() && !matches!(character, '/' | '\\' | '?' | '#' | '&' | '=' | '%')
    });
    let source_format = match source {
        SourceKind::ArtInstituteChicago
        | SourceKind::ClevelandMuseum
        | SourceKind::MetropolitanMuseum => raw.parse::<u64>().is_ok_and(|id| id > 0),
        SourceKind::Smithsonian | SourceKind::WikimediaCommons => true,
    };

    (valid_length && safe_characters && source_format)
        .then(|| ArtworkId(raw.to_owned()))
        .ok_or(ArtworkKeyError::MalformedId)
}

/// Format the mandatory ready-to-print attribution from trusted provider data.
#[must_use]
pub fn format_attribution(artwork: &Artwork) -> String {
    let work = artwork
        .creator
        .as_deref()
        .filter(|creator| !creator.trim().is_empty())
        .map_or_else(
            || format!("“{}”", artwork.title),
            |creator| format!("“{}” — {creator}", artwork.title),
        );
    let mut parts = vec![work];
    if let Some(credit) = artwork
        .provider_credit
        .as_deref()
        .filter(|credit| !credit.trim().is_empty())
    {
        parts.push(credit.to_owned());
    }
    parts.push(format!(
        "{} ({})",
        artwork.license.label(),
        artwork.license.url()
    ));
    parts.push(format!("Source: {}", artwork.object_url));
    let mut attribution = String::new();
    for part in parts {
        if !attribution.is_empty() {
            attribution.push(' ');
        }
        attribution.push_str(&part);
        let terminal_candidate = part.trim_end().strip_suffix('”').unwrap_or(part.trim_end());
        if !matches!(
            terminal_candidate.chars().next_back(),
            Some('.' | '!' | '?')
        ) {
            attribution.push('.');
        }
    }
    attribution
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use crate::core::{CommercialLicense, ImageUrls};

    use super::*;

    fn complete_artwork() -> Artwork {
        Artwork {
            source: SourceKind::ArtInstituteChicago,
            source_id: "1001".into(),
            title: "Ceremonial Mask".into(),
            creator: Some("Maker unknown".into()),
            date: Some("1900–1920".into()),
            culture: Some("Côte d’Ivoire".into()),
            license: CommercialLicense::PublicDomain,
            image_urls: ImageUrls {
                thumbnail: "https://example.test/thumb.jpg".into(),
                display: "https://example.test/display.jpg".into(),
                original: Some("https://example.test/original.jpg".into()),
            },
            institution: "Art Institute of Chicago".into(),
            provider_credit: Some("Gift of A & B".into()),
            object_url: "https://example.test/artworks/1001".into(),
        }
    }

    #[test]
    fn artwork_key_parses_each_known_source_and_keeps_the_typed_parts() {
        let cases = [
            ("aic", "1001", SourceKind::ArtInstituteChicago),
            ("cleveland", "42", SourceKind::ClevelandMuseum),
            ("met", "77", SourceKind::MetropolitanMuseum),
            ("smithsonian", "edanmdm-NMAFA_1", SourceKind::Smithsonian),
            (
                "wikimedia",
                "File:Mask (1900).jpg",
                SourceKind::WikimediaCommons,
            ),
        ];

        for (source, id, expected_source) in cases {
            let key = ArtworkKey::try_from_parts(source, id).expect("key is valid");
            assert_eq!(key.source(), expected_source);
            assert_eq!(key.id().as_str(), id);
        }
    }

    #[test]
    fn source_parser_maps_every_known_key_and_rejects_unknown_keys() {
        for source in SourceKind::ALL {
            assert_eq!(parse_source(source.key()), Ok(source));
        }
        assert_eq!(parse_source("unknown"), Err(ArtworkKeyError::UnknownSource));
        assert_eq!(parse_source("AIC"), Err(ArtworkKeyError::UnknownSource));
    }

    #[test]
    fn artwork_id_parser_applies_provider_formats_and_safe_character_rules() {
        assert_eq!(
            parse_artwork_id(SourceKind::ArtInstituteChicago, "1001")
                .expect("numeric ID is valid")
                .as_str(),
            "1001"
        );
        assert_eq!(
            parse_artwork_id(SourceKind::WikimediaCommons, "File:Mask (1900).jpg")
                .expect("provider ID is valid")
                .as_str(),
            "File:Mask (1900).jpg"
        );
        for raw in ["", "0", "abc", "1/2", "1%2", "1&id=2"] {
            assert_eq!(
                parse_artwork_id(SourceKind::ArtInstituteChicago, raw),
                Err(ArtworkKeyError::MalformedId),
                "{raw}"
            );
        }
    }

    #[test]
    fn artwork_key_rejects_unknown_sources_malformed_ids_and_url_data() {
        assert_eq!(
            ArtworkKey::try_from_parts("unknown", "1"),
            Err(ArtworkKeyError::UnknownSource)
        );
        for id in [
            "",
            "0",
            "-1",
            "abc",
            "https://example.test/image.jpg",
            "1&url=x",
        ] {
            assert_eq!(
                ArtworkKey::try_from_parts("aic", id),
                Err(ArtworkKeyError::MalformedId)
            );
        }
        assert_eq!(
            ArtworkKey::try_from_parts("wikimedia", &"x".repeat(MAX_OBJECT_ID_LENGTH + 1)),
            Err(ArtworkKeyError::MalformedId)
        );
    }

    #[test]
    fn attribution_matches_the_exact_complete_metadata_format() {
        assert_eq!(
            format_attribution(&complete_artwork()),
            "“Ceremonial Mask” — Maker unknown. Gift of A & B. Public domain (https://creativecommons.org/publicdomain/mark/1.0/). Source: https://example.test/artworks/1001."
        );
    }

    #[test]
    fn attribution_omits_each_unavailable_optional_field_without_empty_punctuation() {
        enum MissingField {
            Creator,
            Date,
            Culture,
            ProviderCredit,
            OriginalImage,
        }
        let cases = [
            ("creator", MissingField::Creator),
            ("date", MissingField::Date),
            ("culture", MissingField::Culture),
            ("provider credit", MissingField::ProviderCredit),
            ("original image", MissingField::OriginalImage),
        ];

        for (name, missing) in cases {
            let mut artwork = complete_artwork();
            match missing {
                MissingField::Creator => artwork.creator = None,
                MissingField::Date => artwork.date = None,
                MissingField::Culture => artwork.culture = None,
                MissingField::ProviderCredit => artwork.provider_credit = None,
                MissingField::OriginalImage => artwork.image_urls.original = None,
            }
            let attribution = format_attribution(&artwork);
            assert!(!attribution.contains(".."), "{name}");
            assert!(!attribution.contains("— ."), "{name}");
            assert!(attribution.contains("Public domain (https://"), "{name}");
            assert!(attribution.ends_with("artworks/1001."), "{name}");
            if name == "creator" {
                assert!(attribution.starts_with("“Ceremonial Mask”. Gift"));
            }
            if name == "provider credit" {
                assert!(!attribution.contains("Gift of A & B"));
            }
        }
    }

    #[test]
    fn attribution_preserves_provider_credit_bytes_verbatim() {
        let mut artwork = complete_artwork();
        artwork.provider_credit = Some("Crédit — Donor; 100% verbatim".into());
        let attribution = format_attribution(&artwork);
        assert!(attribution.contains("Crédit — Donor; 100% verbatim"));
    }

    #[test]
    fn attribution_does_not_duplicate_terminal_creator_or_credit_punctuation() {
        let cases = [
            (
                "Maker unknown.",
                "Gift of A & B",
                "“Ceremonial Mask” — Maker unknown. Gift of A & B. Public domain",
            ),
            (
                "Maker unknown?",
                "Gift of A & B",
                "“Ceremonial Mask” — Maker unknown? Gift of A & B. Public domain",
            ),
            (
                "Maker unknown",
                "Gift of A & B.",
                "“Ceremonial Mask” — Maker unknown. Gift of A & B. Public domain",
            ),
            (
                "Maker unknown",
                "Gift of A & B!",
                "“Ceremonial Mask” — Maker unknown. Gift of A & B! Public domain",
            ),
        ];

        for (creator, credit, expected_start) in cases {
            let mut artwork = complete_artwork();
            artwork.creator = Some(creator.into());
            artwork.provider_credit = Some(credit.into());
            let attribution = format_attribution(&artwork);
            assert!(
                attribution.starts_with(expected_start),
                "{creator} / {credit}"
            );
            assert!(attribution.contains(credit), "{creator} / {credit}");
            assert!(!attribution.contains(".."), "{creator} / {credit}");
            assert!(!attribution.contains("?."), "{creator} / {credit}");
            assert!(!attribution.contains("!."), "{creator} / {credit}");
        }
    }

    #[test]
    fn attribution_omits_blank_optional_text() {
        let mut artwork = complete_artwork();
        artwork.creator = Some(" \t".into());
        artwork.provider_credit = Some("  ".into());
        assert_eq!(
            format_attribution(&artwork),
            "“Ceremonial Mask”. Public domain (https://creativecommons.org/publicdomain/mark/1.0/). Source: https://example.test/artworks/1001."
        );
    }
}
