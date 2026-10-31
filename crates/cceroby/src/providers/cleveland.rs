//! Cleveland Museum of Art request construction and response parsing.

use std::collections::BTreeMap;

use reqwest::header::{HeaderValue, USER_AGENT};
use serde::Deserialize;
use url::Url;

use crate::artwork::ArtworkKey;
use crate::core::{Artwork, CommercialLicense, ImageUrls, SearchQuery, SourceKind};

use super::{
    ArtworkDropReason, DisplayImageRequest, DisplayImageSize, DisplayMediaType, HttpRequest,
    Provider, ProviderCandidate, ProviderEntry, ProviderError, ProviderSearchPage, RatePolicy,
    TokenBucketPolicy,
};

const OFFICIAL_ENDPOINT: &str = "https://openaccess-api.clevelandart.org/api/artworks/";
const SEARCH_FIELDS: &str = "id,accession_number,share_license_status,title,creation_date,creators,culture,url,images,creditline,has_conservation_images";
const PAGE_SIZE: u64 = 20;
const CLEVELAND_USER_AGENT: HeaderValue =
    HeaderValue::from_static("cceroby/0.0.0 (local public-domain artwork search)");

/// Cleveland Museum of Art provider configuration.
#[derive(Debug, Clone)]
pub struct ClevelandProvider {
    endpoint: Url,
}

impl ClevelandProvider {
    #[must_use]
    pub fn new(endpoint: Url) -> Self {
        Self { endpoint }
    }

    pub fn official_endpoint() -> Result<Url, url::ParseError> {
        Url::parse(OFFICIAL_ENDPOINT)
    }

    pub fn official() -> Result<Self, url::ParseError> {
        Self::official_endpoint().map(Self::new)
    }

    #[must_use]
    pub fn entry(&self) -> ProviderEntry<'_> {
        ProviderEntry::Available(self)
    }
}

impl Provider for ClevelandProvider {
    fn kind(&self) -> SourceKind {
        SourceKind::ClevelandMuseum
    }

    fn rate_policy(&self) -> RatePolicy {
        RatePolicy::TokenBucket(TokenBucketPolicy::new(
            std::num::NonZeroU32::MIN,
            std::time::Duration::from_secs(1),
        ))
    }

    fn search_request(&self, query: &SearchQuery, cursor: Option<&str>) -> HttpRequest {
        let skip = cursor
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0);
        let mut url = self.endpoint.clone();
        {
            let mut pairs = url.query_pairs_mut();
            pairs.append_pair("q", query.query.as_str());
            pairs.append_key_only("cc0");
            if let Some(culture) = &query.culture {
                pairs.append_pair("culture", culture.as_str());
            }
            pairs.append_pair("has_image", "1");
            pairs.append_pair("skip", &skip.to_string());
            pairs.append_pair("limit", &PAGE_SIZE.to_string());
            pairs.append_pair("fields", SEARCH_FIELDS);
        }
        cleveland_request(url)
    }

    fn parse_search(
        &self,
        bytes: &[u8],
        _cursor: Option<&str>,
    ) -> Result<ProviderSearchPage, ProviderError> {
        let response: ClevelandSearchResponse =
            serde_json::from_slice(bytes).map_err(|_| ProviderError::MalformedResponse)?;
        let returned =
            u64::try_from(response.data.len()).map_err(|_| ProviderError::MalformedResponse)?;
        let skip = response
            .info
            .parameters
            .get("skip")
            .and_then(json_u64)
            .unwrap_or(0);
        Ok(ProviderSearchPage {
            candidates: response
                .data
                .into_iter()
                .map(|raw| ProviderCandidate { raw, context: None })
                .collect(),
            next_cursor: next_cursor(skip, returned, response.info.total),
        })
    }

    fn parse_artwork(
        &self,
        candidate: &ProviderCandidate,
        _object_bytes: Option<&[u8]>,
    ) -> Result<Artwork, ArtworkDropReason> {
        let raw: ClevelandArtwork = serde_json::from_value(candidate.raw.clone())
            .map_err(|_| ArtworkDropReason::MissingSourceId)?;
        normalize_artwork(raw)
    }

    fn artwork_request(&self, key: &ArtworkKey) -> Result<HttpRequest, ProviderError> {
        let mut url = detail_endpoint(&self.endpoint, key.id().as_str());
        url.query_pairs_mut().append_pair("fields", SEARCH_FIELDS);
        Ok(cleveland_request(url))
    }

    fn parse_artwork_response(&self, bytes: &[u8]) -> Result<Artwork, ProviderError> {
        let response: ClevelandDetailResponse =
            serde_json::from_slice(bytes).map_err(|_| ProviderError::MalformedResponse)?;
        normalize_artwork(response.data).map_err(|reason| match reason {
            ArtworkDropReason::NotPublicDomain => ProviderError::ArtworkUnavailable,
            ArtworkDropReason::MissingSourceId
            | ArtworkDropReason::MissingTitle
            | ArtworkDropReason::MissingImage => ProviderError::MalformedResponse,
        })
    }

    fn display_image_request(
        &self,
        artwork: &Artwork,
        size: DisplayImageSize,
    ) -> Result<DisplayImageRequest, ProviderError> {
        let raw = match size {
            DisplayImageSize::Card => &artwork.image_urls.thumbnail,
            DisplayImageSize::Preview => &artwork.image_urls.display,
        };
        parse_remote_url(raw)
            .map(cleveland_request)
            .map(|request| DisplayImageRequest::new(request, DisplayMediaType::Jpeg))
            .ok_or(ProviderError::InvalidImageRequest)
    }

    fn best_image_request(&self, artwork: &Artwork) -> Result<HttpRequest, ProviderError> {
        artwork
            .image_urls
            .original
            .as_deref()
            .ok_or(ProviderError::MissingImageService)
            .and_then(|raw| parse_remote_url(raw).ok_or(ProviderError::InvalidImageRequest))
            .map(cleveland_request)
    }
}

#[must_use]
fn cleveland_request(url: Url) -> HttpRequest {
    HttpRequest::get(url).with_header(USER_AGENT, CLEVELAND_USER_AGENT)
}

#[must_use]
fn detail_endpoint(search_endpoint: &Url, object_id: &str) -> Url {
    let mut detail = search_endpoint.clone();
    detail.set_path(&format!(
        "{}/{}",
        detail.path().trim_end_matches('/'),
        object_id
    ));
    detail.set_query(None);
    detail
}

#[must_use]
fn next_cursor(skip: u64, returned: u64, total: u64) -> Option<String> {
    skip.checked_add(returned)
        .filter(|next| returned > 0 && *next < total)
        .map(|next| next.to_string())
}

fn json_u64(value: &serde_json::Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|raw| raw.parse().ok()))
}

fn parse_remote_url(raw: &str) -> Option<Url> {
    Url::parse(raw.trim())
        .ok()
        .filter(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some())
}

fn normalize_artwork(raw: ClevelandArtwork) -> Result<Artwork, ArtworkDropReason> {
    let license = parse_license(raw.share_license_status.as_deref())?;
    let source_id = raw
        .id
        .map(|value| value.to_string())
        .ok_or(ArtworkDropReason::MissingSourceId)?;
    let title = nonempty(raw.title).ok_or(ArtworkDropReason::MissingTitle)?;
    let images = raw
        .images
        .as_ref()
        .and_then(select_images)
        .ok_or(ArtworkDropReason::MissingImage)?;
    let object_url = raw
        .url
        .as_deref()
        .and_then(parse_remote_url)
        .ok_or(ArtworkDropReason::MissingSourceId)?;

    Ok(Artwork {
        source: SourceKind::ClevelandMuseum,
        source_id,
        title,
        creator: raw.creators.as_deref().and_then(join_creators),
        date: nonempty(raw.creation_date),
        culture: raw.culture.as_deref().and_then(join_culture),
        license,
        image_urls: ImageUrls {
            thumbnail: images.thumbnail.to_string(),
            display: images.display.to_string(),
            original: Some(images.original.to_string()),
        },
        institution: SourceKind::ClevelandMuseum.label().into(),
        provider_credit: nonempty(raw.creditline),
        object_url: object_url.to_string(),
    })
}

fn parse_license(raw: Option<&str>) -> Result<CommercialLicense, ArtworkDropReason> {
    match raw {
        Some("CC0") => Ok(CommercialLicense::Cc0),
        Some(_) | None => Err(ArtworkDropReason::NotPublicDomain),
    }
}

fn nonempty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.trim().is_empty())
}

fn join_creators(creators: &[ClevelandCreator]) -> Option<String> {
    join_nonempty(
        creators
            .iter()
            .filter_map(|creator| creator.description.as_deref()),
    )
}

fn join_culture(cultures: &[String]) -> Option<String> {
    join_nonempty(cultures.iter().map(String::as_str))
}

fn join_nonempty<'a>(values: impl Iterator<Item = &'a str>) -> Option<String> {
    let joined = values
        .filter(|value| !value.trim().is_empty())
        .collect::<Vec<_>>()
        .join("; ");
    (!joined.is_empty()).then_some(joined)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SelectedImages {
    thumbnail: Url,
    display: Url,
    original: Url,
}

fn select_images(images: &ClevelandImages) -> Option<SelectedImages> {
    let assets = [
        images.web.as_ref(),
        images.print.as_ref(),
        images.full.as_ref(),
    ];
    let valid_assets = assets
        .into_iter()
        .flatten()
        .filter_map(|asset| asset.usable_url().map(|url| (asset, url)))
        .collect::<Vec<_>>();
    let best_jpeg = valid_assets
        .iter()
        .filter(|(asset, _)| asset.media_type() == Some(RemoteMediaType::Jpeg))
        .max_by_key(|(asset, _)| asset.pixel_area())?;
    let thumbnail = images
        .web
        .as_ref()
        .filter(|asset| asset.media_type() == Some(RemoteMediaType::Jpeg))
        .and_then(ClevelandImage::usable_url)
        .unwrap_or_else(|| best_jpeg.1.clone());
    let original = valid_assets
        .iter()
        .filter(|(asset, _)| asset.media_type() == Some(RemoteMediaType::Tiff))
        .max_by_key(|(asset, _)| asset.pixel_area())
        .unwrap_or(best_jpeg)
        .clone();
    Some(SelectedImages {
        thumbnail,
        display: best_jpeg.1.clone(),
        original: original.1,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RemoteMediaType {
    Jpeg,
    Tiff,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClevelandSearchResponse {
    info: ClevelandInfo,
    data: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClevelandInfo {
    total: u64,
    parameters: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClevelandDetailResponse {
    data: ClevelandArtwork,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClevelandArtwork {
    id: Option<u64>,
    #[serde(rename = "accession_number")]
    _accession_number: Option<String>,
    share_license_status: Option<String>,
    title: Option<String>,
    creation_date: Option<String>,
    creators: Option<Vec<ClevelandCreator>>,
    culture: Option<Vec<String>>,
    url: Option<String>,
    images: Option<ClevelandImages>,
    creditline: Option<String>,
    #[serde(rename = "has_conservation_images")]
    _has_conservation_images: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct ClevelandCreator {
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClevelandImages {
    #[serde(rename = "annotation")]
    _annotation: Option<String>,
    web: Option<ClevelandImage>,
    print: Option<ClevelandImage>,
    full: Option<ClevelandImage>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClevelandImage {
    url: String,
    width: Option<String>,
    height: Option<String>,
    #[serde(rename = "filesize")]
    _filesize: Option<String>,
    filename: Option<String>,
}

impl ClevelandImage {
    fn usable_url(&self) -> Option<Url> {
        parse_remote_url(&self.url)
    }

    fn pixel_area(&self) -> u64 {
        self.width
            .as_deref()
            .and_then(|width| width.parse::<u64>().ok())
            .zip(
                self.height
                    .as_deref()
                    .and_then(|height| height.parse::<u64>().ok()),
            )
            .and_then(|(width, height)| width.checked_mul(height))
            .unwrap_or(0)
    }

    fn media_type(&self) -> Option<RemoteMediaType> {
        let filename = self
            .filename
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(&self.url)
            .to_ascii_lowercase();
        if filename.ends_with(".jpg") || filename.ends_with(".jpeg") {
            Some(RemoteMediaType::Jpeg)
        } else if filename.ends_with(".tif") || filename.ends_with(".tiff") {
            Some(RemoteMediaType::Tiff)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod test_fixtures;

#[cfg(test)]
mod tests;
