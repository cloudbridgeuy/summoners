//! Smithsonian Open Access request construction and response parsing.

use std::time::Duration;

use reqwest::header::{HeaderName, HeaderValue, USER_AGENT};
use serde::Deserialize;
use url::{Host, Url};

use crate::artwork::ArtworkKey;
use crate::core::{Artwork, CommercialLicense, ImageUrls, SearchQuery, SourceKind};

use super::{
    ArtworkDropReason, DisplayImageRequest, DisplayImageSize, DisplayMediaType, HttpRequest,
    Provider, ProviderCandidate, ProviderEntry, ProviderError, ProviderSearchPage, RatePolicy,
    TokenBucketPolicy,
};

pub const API_KEY_ENV: &str = "SMITHSONIAN_API_KEY";
const OFFICIAL_ENDPOINT: &str = "https://api.si.edu/openaccess/api/v1.0/search";
const PAGE_SIZE: usize = 20;
const X_API_KEY: HeaderName = HeaderName::from_static("x-api-key");
const SMITHSONIAN_USER_AGENT: HeaderValue =
    HeaderValue::from_static("cceroby/0.0.0 (local public-domain artwork search)");

#[derive(Clone)]
struct ApiKey(HeaderValue);

impl std::fmt::Debug for ApiKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ApiKey([REDACTED])")
    }
}

/// Smithsonian provider configuration.
#[derive(Clone)]
pub struct SmithsonianProvider {
    endpoint: Url,
    api_key: Option<ApiKey>,
}

impl std::fmt::Debug for SmithsonianProvider {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SmithsonianProvider")
            .field("scheme", &self.endpoint.scheme())
            .field("host", &self.endpoint.host_str())
            .field("path", &self.endpoint.path())
            .field("configured", &self.api_key.is_some())
            .finish()
    }
}

impl SmithsonianProvider {
    #[must_use]
    pub fn new(endpoint: Url, api_key: Option<String>) -> Self {
        Self {
            endpoint,
            api_key: api_key.and_then(parse_api_key),
        }
    }

    #[must_use]
    pub fn from_env(endpoint: Url) -> Self {
        Self::new(endpoint, std::env::var(API_KEY_ENV).ok())
    }

    pub fn official_endpoint() -> Result<Url, url::ParseError> {
        Url::parse(OFFICIAL_ENDPOINT)
    }

    pub fn official() -> Result<Self, url::ParseError> {
        Self::official_endpoint().map(Self::from_env)
    }

    #[must_use]
    pub const fn kind(&self) -> SourceKind {
        SourceKind::Smithsonian
    }

    #[must_use]
    pub fn entry(&self) -> ProviderEntry<'_> {
        self.api_key.as_ref().map_or(
            ProviderEntry::Unavailable {
                source: SourceKind::Smithsonian,
            },
            |_| ProviderEntry::Available(self),
        )
    }
}

impl Provider for SmithsonianProvider {
    fn kind(&self) -> SourceKind {
        self.kind()
    }

    fn rate_policy(&self) -> RatePolicy {
        RatePolicy::TokenBucket(TokenBucketPolicy::new(
            std::num::NonZeroU32::MIN,
            Duration::from_secs(1),
        ))
    }

    fn validate_search_cursor(&self, cursor: Option<&str>) -> Result<(), ProviderError> {
        parse_offset(cursor).map(|_| ())
    }

    fn search_request(&self, query: &SearchQuery, cursor: Option<&str>) -> HttpRequest {
        let start = parse_offset(cursor).unwrap_or(0);
        let mut url = self.endpoint.clone();
        {
            let mut pairs = url.query_pairs_mut();
            pairs.append_pair("q", query.query.as_str());
            pairs.append_pair("start", &start.to_string());
            pairs.append_pair("rows", &PAGE_SIZE.to_string());
            pairs.append_pair("type", "edanmdm");
            pairs.append_pair("row_group", "objects");
            pairs.append_pair("fqs", &filter_queries(query));
        }
        smithsonian_api_request(url, self.api_key.as_ref())
    }

    fn parse_search(
        &self,
        bytes: &[u8],
        cursor: Option<&str>,
    ) -> Result<ProviderSearchPage, ProviderError> {
        let envelope: SearchEnvelope =
            serde_json::from_slice(bytes).map_err(|_| ProviderError::MalformedResponse)?;
        let start = parse_offset(cursor)?;
        let row_count = envelope.response.row_count;
        let rows = envelope.response.rows;
        let next_start = start.saturating_add(rows.len());
        let next_cursor =
            (!rows.is_empty() && next_start < row_count).then(|| next_start.to_string());
        Ok(ProviderSearchPage {
            candidates: rows
                .into_iter()
                .map(|raw| ProviderCandidate { raw, context: None })
                .collect(),
            next_cursor,
        })
    }

    fn parse_artwork(
        &self,
        candidate: &ProviderCandidate,
        _object_bytes: Option<&[u8]>,
    ) -> Result<Artwork, ArtworkDropReason> {
        serde_json::from_value(candidate.raw.clone())
            .map_err(|_| ArtworkDropReason::MissingSourceId)
            .and_then(normalize_record)
    }

    fn artwork_request(&self, key: &ArtworkKey) -> Result<HttpRequest, ProviderError> {
        Ok(smithsonian_api_request(
            content_endpoint(&self.endpoint, key.id().as_str()),
            self.api_key.as_ref(),
        ))
    }

    fn parse_artwork_response(&self, bytes: &[u8]) -> Result<Artwork, ProviderError> {
        let envelope: DetailEnvelope =
            serde_json::from_slice(bytes).map_err(|_| ProviderError::MalformedResponse)?;
        normalize_record(envelope.response).map_err(drop_reason_to_provider_error)
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
        public_image_request(raw)
            .map(|request| DisplayImageRequest::new(request, DisplayMediaType::Jpeg))
    }

    fn best_image_request(&self, artwork: &Artwork) -> Result<HttpRequest, ProviderError> {
        artwork
            .image_urls
            .original
            .as_deref()
            .ok_or(ProviderError::MissingImageService)
            .and_then(public_image_request)
    }
}

#[derive(Debug, Deserialize)]
struct SearchEnvelope {
    response: SearchResponse,
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    #[serde(rename = "rowCount")]
    row_count: usize,
    rows: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct DetailEnvelope {
    response: SmithsonianRecord,
}

#[derive(Debug, Deserialize)]
struct SmithsonianRecord {
    url: Option<String>,
    title: Option<String>,
    content: SmithsonianContent,
}

#[derive(Debug, Deserialize)]
struct SmithsonianContent {
    #[serde(rename = "descriptiveNonRepeating")]
    descriptive: Descriptive,
    #[serde(default)]
    freetext: Freetext,
    #[serde(rename = "indexedStructured", default)]
    indexed: Indexed,
}

#[derive(Debug, Deserialize)]
struct Descriptive {
    #[serde(rename = "record_ID")]
    record_id: Option<String>,
    title: Option<LabeledText>,
    #[serde(rename = "data_source")]
    data_source: Option<String>,
    #[serde(rename = "record_link")]
    record_link: Option<String>,
    #[serde(rename = "online_media")]
    online_media: Option<OnlineMedia>,
}

#[derive(Debug, Deserialize)]
struct LabeledText {
    content: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct Freetext {
    #[serde(default)]
    name: Vec<LabeledText>,
    #[serde(default)]
    date: Vec<LabeledText>,
    #[serde(default)]
    culture: Vec<LabeledText>,
    #[serde(default)]
    place: Vec<LabeledText>,
    #[serde(rename = "creditLine", default)]
    credit_line: Vec<LabeledText>,
}

#[derive(Debug, Default, Deserialize)]
struct Indexed {
    #[serde(default)]
    culture: Vec<String>,
    #[serde(default)]
    place: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct OnlineMedia {
    #[serde(default)]
    media: Vec<Media>,
}

#[derive(Debug, Deserialize)]
struct Media {
    #[serde(rename = "type")]
    media_type: Option<String>,
    content: Option<String>,
    thumbnail: Option<String>,
    usage: Option<MediaUsage>,
}

#[derive(Debug, Deserialize)]
struct MediaUsage {
    access: Option<String>,
}

struct SelectedImage {
    thumbnail: String,
    display: String,
    original: String,
}

fn parse_api_key(raw: String) -> Option<ApiKey> {
    (!raw.trim().is_empty())
        .then_some(raw)
        .and_then(|value| HeaderValue::from_str(&value).ok())
        .map(|mut value| {
            value.set_sensitive(true);
            ApiKey(value)
        })
}

fn parse_offset(cursor: Option<&str>) -> Result<usize, ProviderError> {
    match cursor {
        None => Ok(0),
        Some(raw) if !raw.is_empty() && raw.bytes().all(|byte| byte.is_ascii_digit()) => raw
            .parse::<usize>()
            .map_err(|_| ProviderError::MalformedResponse),
        Some(_) => Err(ProviderError::MalformedResponse),
    }
}

fn regional_unit_codes(culture: Option<&str>) -> &'static [&'static str] {
    let Some(culture) = culture else {
        return &[];
    };
    match culture.trim().to_ascii_lowercase().as_str() {
        "africa" | "african" => &["NMAfA"],
        "asia" | "asian" => &["FSG", "FSA"],
        "pre-columbian" | "precolumbian" => &["NMAI"],
        _ => &[],
    }
}

fn filter_queries(query: &SearchQuery) -> String {
    let mut filters = vec!["online_media_type:Images".to_owned()];
    let units = regional_unit_codes(query.culture.as_ref().map(crate::core::Culture::as_str));
    if !units.is_empty() {
        filters.push(
            units
                .iter()
                .map(|unit| format!("unit_code:{unit}"))
                .collect::<Vec<_>>()
                .join(" OR "),
        );
    } else if let Some(culture) = &query.culture {
        filters.push(format!("culture:\"{}\"", escape_query(culture.as_str())));
    }
    serde_json::Value::Array(filters.into_iter().map(serde_json::Value::String).collect())
        .to_string()
}

fn escape_query(raw: &str) -> String {
    raw.replace('\\', "\\\\").replace('"', "\\\"")
}

fn normalize_record(raw: SmithsonianRecord) -> Result<Artwork, ArtworkDropReason> {
    let source_id = nonempty(raw.url)
        .or_else(|| {
            nonempty(raw.content.descriptive.record_id.clone())
                .map(|record_id| format!("edanmdm:{record_id}"))
        })
        .and_then(|value| {
            ArtworkKey::try_from_parts(SourceKind::Smithsonian.key(), &value)
                .ok()
                .map(|key| key.id().as_str().to_owned())
        })
        .ok_or(ArtworkDropReason::MissingSourceId)?;
    let title = nonempty(raw.title)
        .or_else(|| {
            raw.content
                .descriptive
                .title
                .and_then(|title| nonempty(title.content))
        })
        .ok_or(ArtworkDropReason::MissingTitle)?;
    let image = raw
        .content
        .descriptive
        .online_media
        .as_ref()
        .and_then(|online| select_image(&online.media))
        .ok_or(ArtworkDropReason::MissingImage)?;
    let object_url = nonempty(raw.content.descriptive.record_link)
        .and_then(|value| https_url(&value))
        .ok_or(ArtworkDropReason::MissingSourceId)?;
    let culture = record_culture(&raw.content.freetext, &raw.content.indexed);
    Ok(Artwork {
        source: SourceKind::Smithsonian,
        source_id,
        title,
        creator: first_text(&raw.content.freetext.name),
        date: first_text(&raw.content.freetext.date),
        culture,
        license: CommercialLicense::Cc0,
        image_urls: ImageUrls {
            thumbnail: image.thumbnail,
            display: image.display,
            original: Some(image.original),
        },
        institution: nonempty(raw.content.descriptive.data_source)
            .unwrap_or_else(|| SourceKind::Smithsonian.label().into()),
        provider_credit: first_text(&raw.content.freetext.credit_line),
        object_url,
    })
}

fn select_image(media: &[Media]) -> Option<SelectedImage> {
    media.iter().find_map(|item| {
        let is_image = item
            .media_type
            .as_deref()
            .is_some_and(|value| value == "Images");
        let is_cc0 = item
            .usage
            .as_ref()
            .and_then(|usage| usage.access.as_deref())
            .is_some_and(|value| value == "CC0");
        if !is_image || !is_cc0 {
            return None;
        }
        let original = https_url(item.content.as_deref()?)?;
        let thumbnail = item
            .thumbnail
            .as_deref()
            .and_then(https_url)
            .map(|url| resized_image_url(&url, 300))?;
        let display = resized_image_url(&original, 1200);
        Some(SelectedImage {
            thumbnail,
            display,
            original,
        })
    })
}

fn https_url(raw: &str) -> Option<String> {
    Url::parse(raw)
        .ok()
        .filter(browser_safe_image_url)
        .map(Into::into)
}

fn browser_safe_image_url(url: &Url) -> bool {
    let Some(host) = url.host() else {
        return false;
    };
    match url.scheme() {
        "https" => true,
        "http" => match host {
            Host::Domain(domain) => domain == "localhost",
            Host::Ipv4(address) => address.is_loopback(),
            Host::Ipv6(address) => address.is_loopback(),
        },
        _ => false,
    }
}

fn resized_image_url(raw: &str, maximum: u16) -> String {
    let Ok(mut url) = Url::parse(raw) else {
        return raw.to_owned();
    };
    if url.host_str() == Some("ids.si.edu") {
        let retained = url
            .query_pairs()
            .filter(|(name, _)| name != "max")
            .map(|(name, value)| (name.into_owned(), value.into_owned()))
            .collect::<Vec<_>>();
        url.set_query(None);
        {
            let mut query = url.query_pairs_mut();
            for (name, value) in retained {
                query.append_pair(&name, &value);
            }
        }
        url.query_pairs_mut()
            .append_pair("max", &maximum.to_string());
    }
    url.into()
}

fn first_text(values: &[LabeledText]) -> Option<String> {
    values
        .iter()
        .find_map(|value| nonempty(value.content.clone()))
}

fn record_culture(freetext: &Freetext, indexed: &Indexed) -> Option<String> {
    join_metadata(
        indexed
            .culture
            .iter()
            .chain(indexed.place.iter())
            .cloned()
            .chain(
                freetext
                    .culture
                    .iter()
                    .chain(freetext.place.iter())
                    .filter_map(|value| value.content.clone()),
            ),
    )
}

fn join_metadata(values: impl IntoIterator<Item = String>) -> Option<String> {
    let mut parts = Vec::new();
    for value in values.into_iter().filter(|value| !value.trim().is_empty()) {
        if !parts
            .iter()
            .any(|part: &String| part.eq_ignore_ascii_case(&value))
        {
            parts.push(value);
        }
    }
    (!parts.is_empty()).then(|| parts.join("; "))
}

fn nonempty(value: Option<String>) -> Option<String> {
    value.filter(|text| !text.trim().is_empty())
}

fn smithsonian_api_request(url: Url, api_key: Option<&ApiKey>) -> HttpRequest {
    let request = HttpRequest::get(url).with_header(USER_AGENT, SMITHSONIAN_USER_AGENT);
    match api_key {
        Some(key) => request.with_header(X_API_KEY, key.0.clone()),
        None => request,
    }
}

fn public_image_request(raw: &str) -> Result<HttpRequest, ProviderError> {
    let url = Url::parse(raw).map_err(|_| ProviderError::InvalidImageRequest)?;
    if !browser_safe_image_url(&url) {
        return Err(ProviderError::InvalidImageRequest);
    }
    Ok(HttpRequest::get(url).with_header(USER_AGENT, SMITHSONIAN_USER_AGENT))
}

fn content_endpoint(search_endpoint: &Url, object_id: &str) -> Url {
    let mut detail = search_endpoint.clone();
    let api_path = detail
        .path()
        .trim_end_matches('/')
        .strip_suffix("/search")
        .unwrap_or(detail.path());
    detail.set_path(&format!("{api_path}/content/{object_id}"));
    detail.set_query(None);
    detail
}

const fn drop_reason_to_provider_error(reason: ArtworkDropReason) -> ProviderError {
    match reason {
        ArtworkDropReason::NotPublicDomain => ProviderError::ArtworkUnavailable,
        ArtworkDropReason::MissingSourceId
        | ArtworkDropReason::MissingTitle
        | ArtworkDropReason::MissingImage => ProviderError::MalformedResponse,
    }
}

#[cfg(test)]
mod tests;
