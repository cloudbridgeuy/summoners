//! Listener, browser, and process-signal shell for local search.

use std::net::SocketAddr;
use std::time::SystemTime;

use color_eyre::eyre::{Result, WrapErr};
use futures::future::join_all;
use thiserror::Error;
use tokio::net::TcpListener;
use tokio::sync::broadcast;

use crate::artwork::ArtworkKey;
use crate::asset_writer::write_atomic_replace;
use crate::cache::Cache;
use crate::core::{
    ProviderOutcome, ProviderPage, SearchQuery, SearchSeed, SearchSession, merge_page,
};
use crate::download::{DownloadError, DownloadJob, SavedAsset};
use crate::http::HttpClient;
use crate::image::{DownloadedImage, ImageError};
use crate::providers::{
    DisplayImageSize, DisplayMediaType, HttpRequest, Provider, ProviderCandidate,
    ProviderConfigError, ProviderEntry, ProviderSet,
};
use crate::rate_limit::RateLimiters;
use crate::server::{self, AppState};
use crate::xmp::{build_xmp_packet, embed_xmp};

/// Provider I/O dependencies shared by startup and form searches.
#[derive(Debug, Clone)]
pub struct SearchServices {
    providers: ProviderSet,
    cache: Cache,
    http: HttpClient,
    rate_limiters: RateLimiters,
}

impl SearchServices {
    pub fn from_env() -> std::result::Result<Self, ProviderConfigError> {
        let cache = Cache::from_user_cache_dir();
        let _ = cache.prune_expired(SystemTime::now());
        Ok(Self {
            providers: ProviderSet::from_env()?,
            cache,
            http: HttpClient::new(),
            rate_limiters: RateLimiters::new(),
        })
    }

    #[must_use]
    pub fn new(
        providers: ProviderSet,
        cache: Cache,
        http: HttpClient,
        rate_limiters: RateLimiters,
    ) -> Self {
        Self {
            providers,
            cache,
            http,
            rate_limiters,
        }
    }

    pub async fn search_batch(&self, query: &SearchQuery) -> Vec<ProviderOutcome> {
        let now = SystemTime::now();
        let searches = self
            .providers
            .selected(query.sources.as_slice())
            .into_iter()
            .map(|entry| self.search_one(entry, query, now));
        join_all(searches).await
    }

    /// Reconstruct one trusted artwork from its provider and metadata cache.
    pub async fn load_artwork(
        &self,
        key: &ArtworkKey,
    ) -> std::result::Result<crate::core::Artwork, ArtworkLoadError> {
        let ProviderEntry::Available(provider) = self.providers.get(key.source()) else {
            return Err(ArtworkLoadError::SourceUnavailable);
        };
        let request = provider
            .artwork_request(key)
            .map_err(|_| ArtworkLoadError::ArtworkUnavailable)?;
        let bytes = self
            .get_metadata(provider, &request, SystemTime::now())
            .await
            .map_err(|_| ArtworkLoadError::ArtworkUnavailable)?;
        provider
            .parse_artwork_response(&bytes)
            .map_err(|_| ArtworkLoadError::ArtworkUnavailable)
    }

    /// Load one provider-derived display image through the thumbnail byte cache.
    pub async fn load_display_image(
        &self,
        key: &ArtworkKey,
        size: DisplayImageSize,
    ) -> std::result::Result<DisplayImage, ArtworkLoadError> {
        let artwork = self.load_artwork(key).await?;
        let ProviderEntry::Available(provider) = self.providers.get(key.source()) else {
            return Err(ArtworkLoadError::SourceUnavailable);
        };
        let display_request = provider
            .display_image_request(&artwork, size)
            .map_err(|_| ArtworkLoadError::ImageUnavailable)?;
        let now = SystemTime::now();
        if let Some(cached) = self
            .cache
            .read_thumbnail(display_request.request().canonical(), now)
        {
            return Ok(DisplayImage {
                bytes: cached.bytes,
                media_type: cached.media_type,
            });
        }
        self.rate_limiters.acquire(provider).await;
        let bytes = self
            .http
            .execute(display_request.request())
            .await
            .map_err(|_| ArtworkLoadError::ImageUnavailable)?;
        let fetched_at = SystemTime::now();
        let media_type = display_request.media_type();
        let _ = self.cache.write_thumbnail(
            display_request.request().canonical(),
            media_type,
            &bytes,
            fetched_at,
        );
        Ok(DisplayImage { bytes, media_type })
    }

    /// Fetch one full provider image without caching it, then write its self-contained JPEG.
    pub(crate) async fn download(
        &self,
        job: DownloadJob<'_>,
    ) -> std::result::Result<SavedAsset, DownloadError> {
        let ProviderEntry::Available(provider) = self.providers.get(job.artwork.source) else {
            return Err(DownloadError::Provider);
        };
        let request = provider
            .best_image_request(job.artwork)
            .map_err(|_| DownloadError::Provider)?;
        self.rate_limiters.acquire(provider).await;
        let bytes = self
            .http
            .execute(&request)
            .await
            .map_err(|_| DownloadError::Provider)?;
        let downloaded = DownloadedImage::try_from_magic(bytes).map_err(|error| match error {
            ImageError::UnsupportedFormat => DownloadError::UnsupportedFormat,
            ImageError::ConversionFailed => DownloadError::Image,
        })?;
        let jpeg = downloaded
            .into_jpeg_quality_100()
            .map_err(|_| DownloadError::Image)?;
        let packet = build_xmp_packet(job.artwork, job.attribution, job.tags)
            .map_err(|_| DownloadError::Xmp)?;
        let embedded = embed_xmp(&jpeg, &packet).map_err(|_| DownloadError::Xmp)?;
        write_atomic_replace(job.output, job.slug, &embedded).map_err(|_| DownloadError::Write)
    }

    async fn search_one(
        &self,
        entry: ProviderEntry<'_>,
        query: &SearchQuery,
        now: SystemTime,
    ) -> ProviderOutcome {
        let ProviderEntry::Available(provider) = entry else {
            return ProviderOutcome::Unavailable {
                source: entry.kind(),
            };
        };
        let request = provider.search_request(query, None);
        let result: std::result::Result<ProviderPage, ()> = async {
            let bytes = self.get_metadata(provider, &request, now).await?;
            let page = provider.parse_search(&bytes).map_err(|_| ())?;
            let candidates = page
                .candidates
                .iter()
                .map(|candidate| self.resolve_candidate(provider, candidate, now));
            let artworks = join_all(candidates).await.into_iter().flatten().collect();
            Ok(ProviderPage {
                source: provider.kind(),
                artworks,
                next_cursor: page.next_cursor,
            })
        }
        .await;
        ProviderOutcome::from_result(provider.kind(), result)
    }

    async fn resolve_candidate(
        &self,
        provider: &dyn Provider,
        candidate: &ProviderCandidate,
        now: SystemTime,
    ) -> Option<crate::core::Artwork> {
        let object_bytes = match provider.object_request(candidate) {
            Some(request) => Some(self.get_metadata(provider, &request, now).await.ok()?),
            None => None,
        };
        provider
            .parse_artwork(candidate, object_bytes.as_deref())
            .ok()
    }

    async fn get_metadata(
        &self,
        provider: &dyn Provider,
        request: &HttpRequest,
        now: SystemTime,
    ) -> std::result::Result<Vec<u8>, ()> {
        if let Some(bytes) = self
            .cache
            .read_metadata(provider.kind(), request.canonical(), now)
        {
            return Ok(bytes);
        }
        self.rate_limiters.acquire(provider).await;
        let bytes = self.http.execute(request).await.map_err(|_| ())?;
        let _ = self
            .cache
            .write_metadata(provider.kind(), request.canonical(), &bytes, now);
        Ok(bytes)
    }
}

/// Provider image bytes paired with a browser-safe media type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayImage {
    pub bytes: Vec<u8>,
    pub media_type: DisplayMediaType,
}

/// A short artwork load failure that contains no transport data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ArtworkLoadError {
    #[error("the artwork source is not available")]
    SourceUnavailable,
    #[error("the artwork is not available")]
    ArtworkUnavailable,
    #[error("the artwork image is not available")]
    ImageUnavailable,
}

/// Bind a loopback-only listener on a free operating-system assigned port.
pub async fn bind_listener(address: &str) -> std::io::Result<TcpListener> {
    TcpListener::bind(address).await
}

/// Start the local page and stop it after Ctrl-C.
pub async fn run(seed: SearchSeed) -> Result<()> {
    let should_open = seed.open;
    let output = seed.output.clone();
    let query = SearchQuery::from_seed(&seed);
    let services = SearchServices::from_env().wrap_err("cannot configure providers")?;
    let outcomes = services.search_batch(&query).await;
    let mut session = SearchSession::new(query);
    for outcome in outcomes {
        merge_page(&mut session, outcome);
    }
    let listener = bind_listener("127.0.0.1:0")
        .await
        .wrap_err("cannot bind the local search server")?;
    let address = listener
        .local_addr()
        .wrap_err("cannot read the local search address")?;
    let url = serving_url(address);
    let (shutdown, receiver) = broadcast::channel(1);
    let state = AppState::new(session, services, output, shutdown);

    eprintln!("Serving {url}");
    eprintln!("Press Ctrl-C to stop.");
    if should_open && webbrowser::open(&url).is_err() {
        eprintln!("Could not open the default browser. Open {url} manually.");
    }

    let signal = shutdown_signal(state.shutdown_sender());
    let server = server::run(listener, state, receiver);
    let (signal_result, server_result) = tokio::join!(signal, server);
    signal_result?;
    server_result.wrap_err("the local search server stopped with an error")
}

/// Wait for Ctrl-C and notify all local server tasks.
pub async fn shutdown_signal(shutdown: broadcast::Sender<()>) -> Result<()> {
    tokio::signal::ctrl_c()
        .await
        .wrap_err("cannot install the Ctrl-C handler")?;
    let _ = shutdown.send(());
    Ok(())
}

fn serving_url(address: SocketAddr) -> String {
    format!("http://{address}")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use axum::Router;
    use axum::extract::State;
    use axum::routing::get;
    use tempfile::tempdir;
    use url::Url;

    use crate::core::{
        Artwork, CommercialLicense, Culture, ImageUrls, QueryText, SourceKind, SourceSet,
    };
    use crate::providers::{ArtworkDropReason, ProviderError, ProviderSearchPage};

    use super::*;

    struct ObjectProvider {
        endpoint: Url,
    }

    impl Provider for ObjectProvider {
        fn kind(&self) -> SourceKind {
            SourceKind::ClevelandMuseum
        }

        fn rate_policy(&self) -> crate::providers::RatePolicy {
            crate::providers::RatePolicy::Unlimited
        }

        fn search_request(&self, _query: &SearchQuery, _cursor: Option<&str>) -> HttpRequest {
            HttpRequest::get(self.endpoint.join("search").expect("search URL is valid"))
        }

        fn parse_search(&self, _bytes: &[u8]) -> Result<ProviderSearchPage, ProviderError> {
            Ok(ProviderSearchPage {
                candidates: vec![ProviderCandidate {
                    raw: serde_json::json!({ "id": "1" }),
                    context: None,
                }],
                next_cursor: None,
            })
        }

        fn object_request(&self, _candidate: &ProviderCandidate) -> Option<HttpRequest> {
            Some(HttpRequest::get(
                self.endpoint.join("object/1").expect("object URL is valid"),
            ))
        }

        fn parse_artwork(
            &self,
            _candidate: &ProviderCandidate,
            object_bytes: Option<&[u8]>,
        ) -> Result<Artwork, ArtworkDropReason> {
            if object_bytes != Some(b"object".as_slice()) {
                return Err(ArtworkDropReason::MissingImage);
            }
            Ok(Artwork {
                source: self.kind(),
                source_id: "1".into(),
                title: "Mask".into(),
                creator: None,
                date: None,
                culture: None,
                license: CommercialLicense::PublicDomain,
                image_urls: ImageUrls {
                    thumbnail: "https://example.test/thumb.jpg".into(),
                    display: "https://example.test/display.jpg".into(),
                    original: None,
                },
                institution: self.kind().label().into(),
                provider_credit: None,
                object_url: "https://example.test/object/1".into(),
            })
        }

        fn artwork_request(&self, _key: &ArtworkKey) -> Result<HttpRequest, ProviderError> {
            Ok(HttpRequest::get(
                self.endpoint.join("object/1").expect("object URL is valid"),
            ))
        }

        fn parse_artwork_response(&self, bytes: &[u8]) -> Result<Artwork, ProviderError> {
            self.parse_artwork(
                &ProviderCandidate {
                    raw: serde_json::json!({ "id": "1" }),
                    context: None,
                },
                Some(bytes),
            )
            .map_err(|_| ProviderError::MalformedResponse)
        }

        fn display_image_request(
            &self,
            artwork: &Artwork,
            size: DisplayImageSize,
        ) -> Result<crate::providers::DisplayImageRequest, ProviderError> {
            let raw = match size {
                DisplayImageSize::Card => &artwork.image_urls.thumbnail,
                DisplayImageSize::Preview => &artwork.image_urls.display,
            };
            Url::parse(raw)
                .map(HttpRequest::get)
                .map(|request| {
                    crate::providers::DisplayImageRequest::new(
                        request,
                        crate::providers::DisplayMediaType::Jpeg,
                    )
                })
                .map_err(|_| ProviderError::InvalidImageRequest)
        }
    }

    async fn counted_response(
        State(requests): State<Arc<AtomicUsize>>,
        body: &'static str,
    ) -> &'static str {
        requests.fetch_add(1, Ordering::SeqCst);
        body
    }

    #[tokio::test]
    async fn listener_uses_loopback_and_an_assigned_port() {
        let listener = bind_listener("127.0.0.1:0").await.expect("listener binds");
        let address = listener.local_addr().expect("address is available");
        assert!(address.ip().is_loopback());
        assert_ne!(address.port(), 0);
    }

    #[test]
    fn serving_url_includes_the_assigned_address() {
        let address: SocketAddr = "127.0.0.1:45123".parse().expect("address is valid");
        assert_eq!(serving_url(address), "http://127.0.0.1:45123");
    }

    #[tokio::test]
    async fn generic_search_path_fetches_an_optional_object_request() {
        let requests = Arc::new(AtomicUsize::new(0));
        let app = Router::new()
            .route("/search", get(|state| counted_response(state, "search")))
            .route("/object/1", get(|state| counted_response(state, "object")))
            .with_state(requests.clone());
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("mock listener binds");
        let address = listener.local_addr().expect("mock address exists");
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("mock server runs");
        });
        let endpoint = Url::parse(&format!("http://{address}/")).expect("endpoint is valid");
        let provider = ObjectProvider { endpoint };
        let cache_root = tempdir().expect("temporary cache exists");
        let services = SearchServices::new(
            ProviderSet::from_env().expect("provider set is valid"),
            Cache::new(cache_root.path().to_path_buf()),
            HttpClient::new(),
            RateLimiters::new(),
        );
        let query = SearchQuery {
            query: QueryText::parse("mask").expect("query is valid"),
            sources: SourceSet::parse(&[SourceKind::ClevelandMuseum]).expect("source is valid"),
            culture: Culture::parse(None),
        };

        let outcome = services
            .search_one(
                ProviderEntry::Available(&provider),
                &query,
                SystemTime::now(),
            )
            .await;

        let ProviderOutcome::Success(page) = outcome else {
            panic!("provider succeeds");
        };
        assert_eq!(page.artworks.len(), 1);
        assert_eq!(requests.load(Ordering::SeqCst), 2);
        task.abort();
    }
}
