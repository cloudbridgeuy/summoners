//! Wikimedia Commons request construction and response parsing.

use std::time::Duration;

use reqwest::header::{HeaderValue, USER_AGENT};
use serde::{Deserialize, Serialize};
use url::{Host, Url};

use crate::artwork::ArtworkKey;
use crate::core::{Artwork, CommercialLicense, ImageUrls, SearchQuery, SourceKind};

use super::{
    ArtworkDropReason, DisplayImageRequest, DisplayImageSize, DisplayMediaType, HttpRequest,
    Provider, ProviderCandidate, ProviderEntry, ProviderError, ProviderSearchPage, RatePolicy,
    TokenBucketPolicy,
};

const OFFICIAL_ENDPOINT: &str = "https://commons.wikimedia.org/w/api.php";
const PAGE_SIZE: u32 = 20;
const COMMONS_USER_AGENT: HeaderValue = HeaderValue::from_static(concat!(
    "cceroby/",
    env!("CARGO_PKG_VERSION"),
    " (https://github.com/cloudbridgeuy/summoners; contact: https://github.com/cloudbridgeuy)"
));
const MNAV_CATEGORY: &str = "Files provided by Museo Nacional de Artes Visuales de Uruguay";
const CDF_CATEGORY: &str = "Files provided by Centro de Fotografía de Montevideo";

/// Wikimedia Commons provider configuration.
#[derive(Debug, Clone)]
pub struct CommonsProvider {
    endpoint: Url,
}

impl CommonsProvider {
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
    pub const fn kind(&self) -> SourceKind {
        SourceKind::WikimediaCommons
    }
    #[must_use]
    pub fn entry(&self) -> ProviderEntry<'_> {
        ProviderEntry::Available(self)
    }
}
impl Provider for CommonsProvider {
    fn kind(&self) -> SourceKind {
        self.kind()
    }
    fn rate_policy(&self) -> RatePolicy {
        RatePolicy::TokenBucket(TokenBucketPolicy::new(
            std::num::NonZeroU32::MIN,
            Duration::from_millis(200),
        ))
    }
    fn validate_search_cursor(&self, cursor: Option<&str>) -> Result<(), ProviderError> {
        CommonsCursor::parse(cursor).map(|_| ())
    }
    fn search_request(&self, query: &SearchQuery, cursor: Option<&str>) -> HttpRequest {
        let cursor = CommonsCursor::parse(cursor).unwrap_or_default();
        let mut url = self.endpoint.clone();
        let mut pairs = url.query_pairs_mut();
        pairs.append_pair("action", "query");
        pairs.append_pair("format", "json");
        pairs.append_pair("generator", "search");
        pairs.append_pair("gsrsearch", &commons_search_text(query));
        pairs.append_pair("gsrnamespace", "6");
        pairs.append_pair("gsrlimit", &PAGE_SIZE.to_string());
        pairs.append_pair("prop", "imageinfo");
        pairs.append_pair("iiprop", "url|mime|thumbmime|extmetadata");
        pairs.append_pair("iiurlwidth", "843");
        if let Some(offset) = cursor.offset {
            pairs.append_pair("continue", "gsroffset||");
            pairs.append_pair("gsroffset", &offset.to_string());
        }
        drop(pairs);
        commons_request(url)
    }
    fn parse_search(
        &self,
        bytes: &[u8],
        _cursor: Option<&str>,
    ) -> Result<ProviderSearchPage, ProviderError> {
        let response: CommonsResponse =
            serde_json::from_slice(bytes).map_err(|_| ProviderError::MalformedResponse)?;
        let next_cursor = response
            .continuation
            .map(CommonsContinuation::into_cursor)
            .transpose()?;
        let mut pages = response
            .query
            .map_or_else(Vec::new, |query| query.pages.into_values().collect());
        pages.sort_by_key(|page| page.index.unwrap_or(u32::MAX));
        let candidates = pages
            .into_iter()
            .map(|raw| {
                serde_json::to_value(raw)
                    .map(|raw| ProviderCandidate { raw, context: None })
                    .map_err(|_| ProviderError::MalformedResponse)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(ProviderSearchPage {
            candidates,
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
            .and_then(normalize_page)
    }
    fn artwork_request(&self, key: &ArtworkKey) -> Result<HttpRequest, ProviderError> {
        let title = key.id().as_str();
        if !title.starts_with("File:") || title.contains('|') {
            return Err(ProviderError::ArtworkUnavailable);
        }
        Ok(detail_request(self.endpoint.clone(), title))
    }
    fn parse_artwork_response(&self, bytes: &[u8]) -> Result<Artwork, ProviderError> {
        let response: CommonsResponse =
            serde_json::from_slice(bytes).map_err(|_| ProviderError::MalformedResponse)?;
        let mut pages = response
            .query
            .map(|query| query.pages.into_values())
            .ok_or(ProviderError::MalformedResponse)?;
        let page = pages.next().ok_or(ProviderError::MalformedResponse)?;
        if pages.next().is_some() {
            return Err(ProviderError::MalformedResponse);
        }
        normalize_page(page).map_err(drop_reason_to_provider_error)
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
        trusted_remote_url(raw)
            .map(commons_request)
            .map(|request| DisplayImageRequest::new(request, media_type_from_url(raw)))
            .ok_or(ProviderError::InvalidImageRequest)
    }
    fn best_image_request(&self, artwork: &Artwork) -> Result<HttpRequest, ProviderError> {
        artwork
            .image_urls
            .original
            .as_deref()
            .and_then(trusted_remote_url)
            .map(commons_request)
            .ok_or(ProviderError::MissingImageService)
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct CommonsCursor {
    offset: Option<u32>,
}
impl CommonsCursor {
    fn parse(raw: Option<&str>) -> Result<Self, ProviderError> {
        match raw {
            None => Ok(Self::default()),
            Some(value) if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) => {
                value
                    .parse::<u32>()
                    .map(|offset| Self {
                        offset: Some(offset),
                    })
                    .map_err(|_| ProviderError::MalformedResponse)
            }
            Some(_) => Err(ProviderError::MalformedResponse),
        }
    }
}
#[derive(Debug, Deserialize)]
struct CommonsResponse {
    #[serde(rename = "continue")]
    continuation: Option<CommonsContinuation>,
    query: Option<CommonsQuery>,
}
#[derive(Debug, Deserialize)]
struct CommonsContinuation {
    #[serde(rename = "continue")]
    token: String,
    gsroffset: Option<u32>,
}
impl CommonsContinuation {
    fn into_cursor(self) -> Result<String, ProviderError> {
        (self.token == "gsroffset||")
            .then_some(())
            .and(self.gsroffset)
            .map(|offset| offset.to_string())
            .ok_or(ProviderError::MalformedResponse)
    }
}
#[derive(Debug, Deserialize)]
struct CommonsQuery {
    pages: std::collections::BTreeMap<String, CommonsPage>,
}
#[derive(Debug, Deserialize, Serialize)]
struct CommonsPage {
    index: Option<u32>,
    title: Option<String>,
    #[serde(default)]
    imageinfo: Vec<CommonsImageInfo>,
}
#[derive(Debug, Deserialize, Serialize)]
struct CommonsImageInfo {
    url: Option<String>,
    thumburl: Option<String>,
    descriptionurl: Option<String>,
    mime: Option<String>,
    thumbmime: Option<String>,
    #[serde(default)]
    extmetadata: CommonsMetadata,
}
#[derive(Debug, Default, Deserialize, Serialize)]
struct CommonsMetadata {
    #[serde(rename = "LicenseShortName")]
    license_short_name: Option<MetadataValue>,
    #[serde(rename = "LicenseUrl")]
    license_url: Option<MetadataValue>,
    #[serde(rename = "Artist")]
    artist: Option<MetadataValue>,
    #[serde(rename = "DateTime")]
    date: Option<MetadataValue>,
    #[serde(rename = "Credit")]
    credit: Option<MetadataValue>,
}
#[derive(Debug, Deserialize, Serialize)]
struct MetadataValue {
    value: Option<String>,
}

fn normalize_page(page: CommonsPage) -> Result<Artwork, ArtworkDropReason> {
    let source_id = page
        .title
        .and_then(nonempty)
        .filter(|title| title.starts_with("File:"))
        .ok_or(ArtworkDropReason::MissingSourceId)?;
    let info = page
        .imageinfo
        .into_iter()
        .next()
        .ok_or(ArtworkDropReason::MissingImage)?;
    let license =
        license_from_metadata(&info.extmetadata).ok_or(ArtworkDropReason::NotPublicDomain)?;
    let original = info
        .url
        .as_deref()
        .and_then(canonical_trusted_url)
        .ok_or(ArtworkDropReason::MissingImage)?;
    let display = info
        .thumburl
        .as_deref()
        .and_then(canonical_trusted_url)
        .ok_or(ArtworkDropReason::MissingImage)?;
    if !downloadable_original_mime(info.mime.as_deref())
        || !display_mime(info.thumbmime.as_deref())
        || !thumbnail_mime_matches_url(info.thumbmime.as_deref(), &display)
    {
        return Err(ArtworkDropReason::MissingImage);
    }
    let title = source_id
        .trim_start_matches("File:")
        .rsplit_once('.')
        .map_or_else(|| source_id.clone(), |(title, _)| title.to_owned());
    Ok(Artwork {
        source: SourceKind::WikimediaCommons,
        source_id: source_id.clone(),
        title,
        creator: metadata_text(info.extmetadata.artist),
        date: metadata_text(info.extmetadata.date),
        culture: None,
        license,
        image_urls: ImageUrls {
            thumbnail: display.clone(),
            display,
            original: Some(original),
        },
        institution: SourceKind::WikimediaCommons.label().into(),
        provider_credit: metadata_text(info.extmetadata.credit),
        object_url: info
            .descriptionurl
            .as_deref()
            .and_then(canonical_trusted_url)
            .ok_or(ArtworkDropReason::MissingSourceId)?,
    })
}
fn license_from_metadata(metadata: &CommonsMetadata) -> Option<CommercialLicense> {
    let url_license = match metadata
        .license_url
        .as_ref()
        .and_then(|value| value.value.as_deref())
        .map(strip_html)
    {
        Some(value) => Some(commons_license_url(&value)?),
        None => None,
    };
    let name_license = match metadata
        .license_short_name
        .as_ref()
        .and_then(|value| value.value.as_deref())
        .map(strip_html)
    {
        Some(value) => Some(CommercialLicense::try_from(value.as_str()).ok()?),
        None => None,
    };
    match (url_license, name_license) {
        (Some(url), Some(name)) if url == name => Some(url),
        (Some(_), Some(_)) => None,
        (Some(url), None) => Some(url),
        (None, Some(name)) => Some(name),
        (None, None) => None,
    }
}

fn commons_license_url(raw: &str) -> Option<CommercialLicense> {
    let authority = raw.split_once("://")?.1.split('/').next()?;
    if authority.contains('@') || authority.contains(':') {
        return None;
    }
    let url = Url::parse(raw).ok()?;
    if !matches!(url.scheme(), "https" | "http")
        || url.host_str()? != "creativecommons.org"
        || url.username() != ""
        || url.password().is_some()
        || url.port().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    match url.path() {
        "/publicdomain/zero/1.0"
        | "/publicdomain/zero/1.0/"
        | "/publicdomain/zero/1.0/deed.en"
        | "/publicdomain/zero/1.0/deed.en/" => Some(CommercialLicense::Cc0),
        "/publicdomain/mark/1.0"
        | "/publicdomain/mark/1.0/"
        | "/publicdomain/mark/1.0/deed.en"
        | "/publicdomain/mark/1.0/deed.en/" => Some(CommercialLicense::PublicDomain),
        "/licenses/by/1.0"
        | "/licenses/by/1.0/"
        | "/licenses/by/1.0/deed.en"
        | "/licenses/by/1.0/deed.en/"
        | "/licenses/by/2.0"
        | "/licenses/by/2.0/"
        | "/licenses/by/2.0/deed.en"
        | "/licenses/by/2.0/deed.en/"
        | "/licenses/by/2.5"
        | "/licenses/by/2.5/"
        | "/licenses/by/2.5/deed.en"
        | "/licenses/by/2.5/deed.en/"
        | "/licenses/by/3.0"
        | "/licenses/by/3.0/"
        | "/licenses/by/3.0/deed.en"
        | "/licenses/by/3.0/deed.en/"
        | "/licenses/by/4.0"
        | "/licenses/by/4.0/"
        | "/licenses/by/4.0/deed.en"
        | "/licenses/by/4.0/deed.en/" => Some(CommercialLicense::CcBy),
        _ => None,
    }
}
fn metadata_text(value: Option<MetadataValue>) -> Option<String> {
    value
        .and_then(|value| value.value)
        .map(|value| strip_html(&value))
        .and_then(nonempty)
}
fn commons_search_text(query: &SearchQuery) -> String {
    institution_category(query.culture.as_ref().map(crate::core::Culture::as_str)).map_or_else(
        || query.query.as_str().to_owned(),
        |category| format!("incategory:\"{category}\" {}", query.query.as_str()),
    )
}
fn institution_category(culture: Option<&str>) -> Option<&'static str> {
    match culture?.trim().to_ascii_lowercase().as_str() {
        "mnav"
        | "museo nacional de artes visuales"
        | "museo nacional de artes visuales de uruguay" => Some(MNAV_CATEGORY),
        "cdf" | "centro de fotografía de montevideo" | "centro de fotografia de montevideo" => {
            Some(CDF_CATEGORY)
        }
        _ => None,
    }
}
fn detail_request(mut url: Url, title: &str) -> HttpRequest {
    let mut pairs = url.query_pairs_mut();
    pairs.append_pair("action", "query");
    pairs.append_pair("format", "json");
    pairs.append_pair("titles", title);
    pairs.append_pair("prop", "imageinfo");
    pairs.append_pair("iiprop", "url|mime|thumbmime|extmetadata");
    pairs.append_pair("iiurlwidth", "843");
    drop(pairs);
    commons_request(url)
}
fn commons_request(url: Url) -> HttpRequest {
    HttpRequest::get(url).with_header(USER_AGENT, COMMONS_USER_AGENT)
}
fn nonempty(value: String) -> Option<String> {
    (!value.trim().is_empty()).then_some(value)
}
fn strip_html(raw: &str) -> String {
    let mut text = String::new();
    let mut in_tag = false;
    for character in raw.chars() {
        match character {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => text.push(character),
            _ => {}
        }
    }
    text.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .trim()
        .to_owned()
}
fn display_mime(value: Option<&str>) -> bool {
    matches!(value, Some("image/jpeg" | "image/png" | "image/webp"))
}

fn downloadable_original_mime(value: Option<&str>) -> bool {
    matches!(value, Some("image/jpeg" | "image/tiff"))
}
fn media_type_from_url(raw: &str) -> DisplayMediaType {
    media_type_from_extension(raw).unwrap_or(DisplayMediaType::Jpeg)
}

fn media_type_from_extension(raw: &str) -> Option<DisplayMediaType> {
    let extension = Url::parse(raw).ok().and_then(|url| {
        url.path()
            .rsplit_once('.')
            .map(|(_, extension)| extension.to_ascii_lowercase())
    });
    match extension.as_deref() {
        Some("jpg" | "jpeg") => Some(DisplayMediaType::Jpeg),
        Some("png") => Some(DisplayMediaType::Png),
        Some("webp") => Some(DisplayMediaType::Webp),
        _ => None,
    }
}

fn thumbnail_mime_matches_url(mime: Option<&str>, url: &str) -> bool {
    let expected = match mime {
        Some("image/jpeg") => DisplayMediaType::Jpeg,
        Some("image/png") => DisplayMediaType::Png,
        Some("image/webp") => DisplayMediaType::Webp,
        _ => return false,
    };
    media_type_from_extension(url) == Some(expected)
}
fn canonical_trusted_url(raw: &str) -> Option<String> {
    trusted_remote_url(raw).map(Into::into)
}
fn trusted_remote_url(raw: &str) -> Option<Url> {
    let url = Url::parse(raw).ok()?;
    let allowed = match (url.scheme(), url.host()?) {
        ("https", Host::Domain(domain)) => domain != "localhost",
        ("http", Host::Domain(domain)) => domain == "localhost",
        ("http", Host::Ipv4(address)) => address.is_loopback(),
        ("http", Host::Ipv6(address)) => address.is_loopback(),
        _ => false,
    };
    allowed.then_some(url)
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
mod tests {
    #![allow(clippy::expect_used)]
    use super::*;
    use crate::core::{Culture, QueryText, SourceSet};
    use reqwest::header::USER_AGENT;
    fn query(culture: Option<&str>) -> SearchQuery {
        SearchQuery {
            query: QueryText::parse("mask").expect("query"),
            sources: SourceSet::parse(&[SourceKind::WikimediaCommons]).expect("sources"),
            culture: Culture::parse(culture.map(str::to_owned)),
        }
    }
    fn page(license: &str) -> CommonsPage {
        CommonsPage {
            index: Some(0),
            title: Some("File:Mask.jpg".into()),
            imageinfo: vec![CommonsImageInfo {
                url: Some("https://upload.wikimedia.org/mask.jpg".into()),
                thumburl: Some("https://upload.wikimedia.org/thumb/mask.jpg".into()),
                descriptionurl: Some("https://commons.wikimedia.org/wiki/File:Mask.jpg".into()),
                mime: Some("image/jpeg".into()),
                thumbmime: Some("image/jpeg".into()),
                extmetadata: CommonsMetadata {
                    license_short_name: Some(MetadataValue {
                        value: Some(license.into()),
                    }),
                    license_url: Some(MetadataValue {
                        value: Some(
                            match license {
                                "CC0 1.0" | "CC0" => {
                                    "http://creativecommons.org/publicdomain/zero/1.0/deed.en"
                                }
                                "Public Domain Mark 1.0" => {
                                    "https://creativecommons.org/publicdomain/mark/1.0/"
                                }
                                _ => "https://creativecommons.org/licenses/by/4.0/",
                            }
                            .into(),
                        ),
                    }),
                    artist: Some(MetadataValue {
                        value: Some("<b>Artist</b>".into()),
                    }),
                    date: None,
                    credit: Some(MetadataValue {
                        value: Some("<i>Gift</i>".into()),
                    }),
                },
            }],
        }
    }
    #[test]
    fn cursor_accepts_only_numeric_offsets() {
        assert_eq!(CommonsCursor::parse(None), Ok(CommonsCursor::default()));
        assert_eq!(
            CommonsCursor::parse(Some("20")).expect("cursor").offset,
            Some(20)
        );
        for raw in ["", "-1", "20&url=x", "x"] {
            assert!(CommonsCursor::parse(Some(raw)).is_err());
        }
    }
    #[test]
    fn metadata_license_policy_accepts_and_rejects_exact_forms() {
        for (raw, expected) in [
            ("CC0 1.0", CommercialLicense::Cc0),
            ("Public Domain Mark 1.0", CommercialLicense::PublicDomain),
            ("CC BY 4.0", CommercialLicense::CcBy),
        ] {
            assert_eq!(
                normalize_page(page(raw)).expect("accepted").license,
                expected
            );
        }
        for raw in [
            "CC BY-SA 4.0",
            "CC BY-NC 4.0",
            "CC BY-ND 4.0",
            "CC BY-NC-SA 4.0",
            "CC BY-NC-ND 4.0",
            "free license",
            "",
        ] {
            assert_eq!(
                normalize_page(page(raw)),
                Err(ArtworkDropReason::NotPublicDomain),
                "{raw}"
            );
        }
    }
    #[test]
    fn metadata_text_is_plain_text() {
        let art = normalize_page(page("CC0")).expect("accepted");
        assert_eq!(art.creator.as_deref(), Some("Artist"));
        assert_eq!(art.provider_credit.as_deref(), Some("Gift"));
        assert_eq!(strip_html("<a>Tom &amp; Jo</a>"), "Tom & Jo");
    }
    #[test]
    fn category_search_forms_are_exact() {
        assert_eq!(institution_category(Some("MNAV")), Some(MNAV_CATEGORY));
        assert_eq!(
            institution_category(Some("Centro de Fotografía de Montevideo")),
            Some(CDF_CATEGORY)
        );
        assert_eq!(
            commons_search_text(&query(Some("mnav"))),
            format!("incategory:\"{MNAV_CATEGORY}\" mask")
        );
    }
    #[test]
    fn request_has_header_limit_namespace_and_cursor() {
        let request = CommonsProvider::official()
            .expect("endpoint is valid")
            .search_request(&query(Some("cdf")), Some("20"));
        let pairs = request
            .url()
            .query_pairs()
            .collect::<std::collections::HashMap<_, _>>();
        assert_eq!(request.headers().get(USER_AGENT), Some(&COMMONS_USER_AGENT));
        assert_eq!(
            COMMONS_USER_AGENT.to_str().expect("user agent is valid"),
            concat!(
                "cceroby/",
                env!("CARGO_PKG_VERSION"),
                " (https://github.com/cloudbridgeuy/summoners; contact: https://github.com/cloudbridgeuy)"
            )
        );
        assert_eq!(pairs.get("gsrlimit").map(AsRef::as_ref), Some("20"));
        assert_eq!(pairs.get("gsrnamespace").map(AsRef::as_ref), Some("6"));
        assert_eq!(
            pairs.get("continue").map(AsRef::as_ref),
            Some("gsroffset||")
        );
        assert_eq!(pairs.get("gsroffset").map(AsRef::as_ref), Some("20"));
        assert!(
            pairs
                .get("gsrsearch")
                .is_some_and(|value| value.contains(CDF_CATEGORY))
        );
    }
    #[test]
    fn safe_urls_and_mime_are_required() {
        assert!(trusted_remote_url("https://upload.wikimedia.org/a.jpg").is_some());
        assert!(trusted_remote_url("http://127.0.0.1/a.jpg").is_some());
        for value in [
            "https://localhost/a.jpg",
            "https://127.0.0.1/a.jpg",
            "http://evil.test/a.jpg",
            "file:///a.jpg",
            "javascript:x",
        ] {
            assert!(trusted_remote_url(value).is_none());
        }
        let mut invalid = page("CC0");
        invalid.imageinfo[0].mime = Some("image/svg+xml".into());
        assert_eq!(
            normalize_page(invalid),
            Err(ArtworkDropReason::MissingImage)
        );
        let mut mismatched_thumbnail = page("CC0");
        mismatched_thumbnail.imageinfo[0].thumbmime = Some("image/png".into());
        assert_eq!(
            normalize_page(mismatched_thumbnail),
            Err(ArtworkDropReason::MissingImage)
        );
    }

    #[test]
    fn thumbnail_media_type_requires_matching_url_extension() {
        assert!(thumbnail_mime_matches_url(
            Some("image/png"),
            "https://upload.wikimedia.org/thumb/mask.png"
        ));
        assert!(thumbnail_mime_matches_url(
            Some("image/webp"),
            "https://upload.wikimedia.org/thumb/mask.webp"
        ));
        assert!(!thumbnail_mime_matches_url(
            Some("image/png"),
            "https://upload.wikimedia.org/thumb/mask.jpg"
        ));
        assert!(!thumbnail_mime_matches_url(
            Some("image/webp"),
            "https://upload.wikimedia.org/thumb/mask"
        ));
        assert_eq!(
            media_type_from_url("https://upload.wikimedia.org/thumb/mask.webp"),
            DisplayMediaType::Webp
        );
    }
    #[test]
    fn continuation_is_strict() {
        assert_eq!(
            CommonsContinuation {
                token: "gsroffset||".into(),
                gsroffset: Some(20)
            }
            .into_cursor(),
            Ok("20".into())
        );
        assert!(
            CommonsContinuation {
                token: "bad".into(),
                gsroffset: Some(20)
            }
            .into_cursor()
            .is_err()
        );
    }

    #[test]
    fn license_urls_allow_only_exact_official_forms() {
        for raw in [
            "http://creativecommons.org/publicdomain/zero/1.0/deed.en",
            "https://creativecommons.org/publicdomain/mark/1.0/",
            "https://creativecommons.org/licenses/by/4.0/deed.en",
        ] {
            assert!(commons_license_url(raw).is_some(), "{raw}");
        }
        for raw in [
            "https://user@creativecommons.org/licenses/by/4.0/",
            "https://creativecommons.org:443/licenses/by/4.0/",
            "https://creativecommons.org/licenses/by/4.0/?x=1",
            "https://creativecommons.org/licenses/by/4.0/#terms",
            "https://creativecommons.org/licenses/by/4.0//",
            "https://creativecommons.org/licenses/by/4.0/deed.fr",
        ] {
            assert_eq!(commons_license_url(raw), None, "{raw}");
        }
    }
    #[test]
    fn malformed_json_and_required_fields_are_rejected() {
        let provider = CommonsProvider::official().expect("endpoint is valid");
        assert!(provider.parse_search(b"{", None).is_err());
        let mut missing_title = page("CC0");
        missing_title.title = None;
        assert_eq!(
            normalize_page(missing_title),
            Err(ArtworkDropReason::MissingSourceId)
        );
        let mut missing_thumbnail_type = page("CC0");
        missing_thumbnail_type.imageinfo[0].thumbmime = None;
        assert_eq!(
            normalize_page(missing_thumbnail_type),
            Err(ArtworkDropReason::MissingImage)
        );
    }

    #[test]
    fn detail_response_binds_to_one_exact_typed_key() {
        let provider = CommonsProvider::official().expect("endpoint is valid");
        let key = ArtworkKey::try_from_parts("wikimedia", "File:Mask.jpg").expect("key is valid");
        let page_value = serde_json::to_value(page("CC0")).expect("page serializes");
        let exact = serde_json::json!({ "query": { "pages": { "1": page_value } } });
        let exact = serde_json::to_vec(&exact).expect("detail response serializes");
        assert!(
            provider
                .parse_artwork_response_for_key(&key, &exact)
                .is_ok()
        );

        let mismatch = ArtworkKey::try_from_parts("wikimedia", "File:Other.jpg")
            .expect("different key is valid");
        assert_eq!(
            provider.parse_artwork_response_for_key(&mismatch, &exact),
            Err(ProviderError::ArtworkUnavailable)
        );

        let mut other = page("CC0");
        other.title = Some("File:Other.jpg".into());
        let multiple = serde_json::json!({
            "query": { "pages": {
                "1": serde_json::to_value(page("CC0")).expect("page serializes"),
                "2": serde_json::to_value(other).expect("other page serializes")
            }}
        });
        let multiple = serde_json::to_vec(&multiple).expect("detail response serializes");
        assert_eq!(
            provider.parse_artwork_response_for_key(&key, &multiple),
            Err(ProviderError::MalformedResponse)
        );
        let separator = ArtworkKey::try_from_parts("wikimedia", "File:Mask.jpg|evil")
            .expect("typed key accepts a provider-specific candidate");
        assert_eq!(
            provider.artwork_request(&separator),
            Err(ProviderError::ArtworkUnavailable)
        );
    }

    #[test]
    fn live_shape_uses_gsroffset_thumbmime_and_generator_index() {
        let provider = CommonsProvider::official().expect("endpoint is valid");
        let first = serde_json::json!({
            "index": 0,
            "title": "File:First.jpg",
            "imageinfo": [{
                "url": "https://upload.wikimedia.org/first.jpg",
                "thumburl": "https://upload.wikimedia.org/thumb/first.jpg",
                "descriptionurl": "https://commons.wikimedia.org/wiki/File:First.jpg",
                "mime": "image/jpeg",
                "thumbmime": "image/jpeg",
                "extmetadata": {
                    "LicenseShortName": { "value": "CC BY 4.0" },
                    "LicenseUrl": { "value": "https://creativecommons.org/licenses/by/4.0/" }
                }
            }]
        });
        let second = serde_json::json!({
            "index": 1,
            "title": "File:Second.jpg",
            "imageinfo": [{
                "url": "https://upload.wikimedia.org/second.jpg",
                "thumburl": "https://upload.wikimedia.org/thumb/second.jpg",
                "descriptionurl": "https://commons.wikimedia.org/wiki/File:Second.jpg",
                "mime": "image/jpeg",
                "thumbmime": "image/jpeg",
                "extmetadata": {
                    "LicenseShortName": { "value": "CC0" },
                    "LicenseUrl": { "value": "http://creativecommons.org/publicdomain/zero/1.0/deed.en" }
                }
            }]
        });
        let response = serde_json::json!({
            "continue": { "gsroffset": 2, "continue": "gsroffset||" },
            "query": { "pages": { "7": second, "4": first } }
        });
        let bytes = serde_json::to_vec(&response).expect("live shape serializes");
        let parsed = provider
            .parse_search(&bytes, None)
            .expect("live shape parses");
        assert_eq!(parsed.next_cursor.as_deref(), Some("2"));
        let titles = parsed
            .candidates
            .iter()
            .map(|candidate| {
                provider
                    .parse_artwork(candidate, None)
                    .expect("accepted")
                    .source_id
            })
            .collect::<Vec<_>>();
        assert_eq!(titles, vec!["File:First.jpg", "File:Second.jpg"]);
    }

    #[test]
    fn conflicting_license_metadata_and_non_downloadable_original_are_rejected() {
        let mut conflict = page("CC BY-SA 4.0");
        assert_eq!(
            normalize_page(conflict),
            Err(ArtworkDropReason::NotPublicDomain)
        );
        conflict = page("CC BY 4.0");
        conflict.imageinfo[0].mime = Some("image/png".into());
        assert_eq!(
            normalize_page(conflict),
            Err(ArtworkDropReason::MissingImage)
        );
    }

    #[test]
    fn detail_display_and_download_requests_keep_the_user_agent() {
        let provider = CommonsProvider::official().expect("endpoint is valid");
        let response = serde_json::json!({
            "query": { "pages": { "1": serde_json::to_value(page("CC0")).expect("page serializes") } }
        });
        let artwork = provider
            .parse_artwork_response(
                &serde_json::to_vec(&response).expect("detail response serializes"),
            )
            .expect("detail response is accepted");
        let key = ArtworkKey::try_from_parts("wikimedia", "File:Mask.jpg").expect("key is valid");
        let requests = [
            provider.artwork_request(&key).expect("detail request"),
            provider
                .display_image_request(&artwork, DisplayImageSize::Card)
                .expect("card request")
                .request()
                .clone(),
            provider
                .display_image_request(&artwork, DisplayImageSize::Preview)
                .expect("preview request")
                .request()
                .clone(),
            provider
                .best_image_request(&artwork)
                .expect("download request"),
        ];
        assert!(
            requests
                .into_iter()
                .all(|request| request.headers().get(USER_AGENT) == Some(&COMMONS_USER_AGENT))
        );
        assert_eq!(
            artwork.object_url,
            "https://commons.wikimedia.org/wiki/File:Mask.jpg"
        );
        let attribution = crate::artwork::format_attribution(&artwork);
        let tags = crate::download::Tags::parse("Commons");
        let xmp =
            crate::xmp::build_xmp_packet(&artwork, &attribution, &tags).expect("XMP is valid");
        let xmp = std::str::from_utf8(xmp.as_bytes()).expect("XMP is UTF-8");
        assert!(xmp.contains("Gift"));
        assert!(xmp.contains("wikimedia"));
        assert!(xmp.contains("CC0"));
        assert!(xmp.contains("File:Mask.jpg"));
    }

    #[ignore = "requires the public Commons service"]
    #[tokio::test]
    async fn live_search_parses_accepted_artwork_count() {
        let provider = CommonsProvider::official().expect("endpoint is valid");
        let client = reqwest::Client::new();
        let request = provider.search_request(&query(None), None);
        let response = client
            .get(request.url().clone())
            .headers(request.headers().clone())
            .send()
            .await
            .expect("Commons request succeeds");
        assert!(response.status().is_success());
        let bytes = response.bytes().await.expect("Commons body is readable");
        let first = provider
            .parse_search(&bytes, None)
            .expect("Commons page parses");
        let first_accepted = first
            .candidates
            .iter()
            .filter(|candidate| provider.parse_artwork(candidate, None).is_ok())
            .count();
        let cursor = first.next_cursor.as_deref().expect("first page continues");
        let request = provider.search_request(&query(None), Some(cursor));
        let response = client
            .get(request.url().clone())
            .headers(request.headers().clone())
            .send()
            .await
            .expect("second Commons request succeeds");
        assert!(response.status().is_success());
        let bytes = response
            .bytes()
            .await
            .expect("second Commons body is readable");
        let second = provider
            .parse_search(&bytes, Some(cursor))
            .expect("second Commons page parses");
        let second_accepted = second
            .candidates
            .iter()
            .filter(|candidate| provider.parse_artwork(candidate, None).is_ok())
            .count();
        eprintln!(
            "Commons live first page: {} candidates, {first_accepted} accepted; second page: {} candidates, {second_accepted} accepted",
            first.candidates.len(),
            second.candidates.len()
        );
        assert!(first_accepted + second_accepted > 0);
    }
}
