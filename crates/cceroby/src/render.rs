//! Pure complete-page HTML rendering.

use crate::artwork::ArtworkKey;
use crate::core::{Artwork, ProviderNotice, SearchView, SourceKind};
use crate::download::DownloadNotice;

const PAGE_LIFECYCLE_SCRIPT: &str = r#"<script>
(function () {
  var live = new EventSource('/live');
  window.addEventListener('pagehide', function () { live.close(); });
  document.addEventListener('keydown', function (event) {
    if (event.key !== 'Escape') return;
    event.preventDefault();
    fetch('/quit', { method: 'POST', credentials: 'same-origin', keepalive: true });
    window.close();
  });
}());
</script>"#;

/// Trusted data rendered on the artwork detail page.
///
/// A later download must pass `attribution` unchanged to XMP construction.
#[derive(Debug, Clone, Copy)]
pub struct DetailView<'a> {
    pub artwork: &'a Artwork,
    pub key: &'a ArtworkKey,
    pub attribution: &'a str,
    pub slug: &'a str,
    pub tags: &'a str,
    pub notice: Option<&'a DownloadNotice>,
}

/// Render a complete local search page from read-only page data.
#[must_use]
pub fn render_search_page(view: SearchView<'_>) -> String {
    let query = escape_html(view.query.query.as_str());
    let culture = view
        .query
        .culture
        .as_ref()
        .map_or_else(String::new, |value| escape_html(value.as_str()));
    let sources = SourceKind::ALL
        .into_iter()
        .map(|source| {
            format!(
                "<label><input type=\"checkbox\" name=\"{}\" value=\"true\"{}> {}</label>",
                source.key(),
                if view.query.sources.contains(source) {
                    " checked"
                } else {
                    ""
                },
                source.label()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let notices = view
        .notices
        .iter()
        .map(|notice| match notice {
            ProviderNotice::Unavailable { source } => format!(
                "<li><strong>{}</strong> is not available in this build.</li>",
                source.label()
            ),
            ProviderNotice::Failed { source } => format!(
                "<li><strong>{}</strong> could not complete the search.</li>",
                source.label()
            ),
        })
        .collect::<Vec<_>>()
        .join("\n");
    let notice_section = if notices.is_empty() {
        String::new()
    } else {
        format!("<section aria-live=\"polite\"><h2>Source status</h2><ul>{notices}</ul></section>")
    };
    let cards = view
        .artworks
        .iter()
        .filter_map(|artwork| {
            let key = ArtworkKey::try_from_parts(artwork.source.key(), &artwork.source_id).ok()?;
            let detail_url = escape_html(&artwork_route_url("/detail", &key));
            let thumbnail_url = escape_html(&artwork_route_url("/thumb", &key));
            Some(format!(
                concat!(
                    "<article class=\"artwork-card\">",
                    "<a href=\"{detail_url}\"><img src=\"{thumbnail_url}\" alt=\"\" loading=\"lazy\"></a>",
                    "<h3><a href=\"{detail_url}\">{}</a></h3><p>{}</p>",
                    "<span class=\"license-badge\">{}</span>",
                    "</article>"
                ),
                escape_html(&artwork.title),
                escape_html(&artwork.institution),
                artwork.license.label(),
                detail_url = detail_url,
                thumbnail_url = thumbnail_url,
            ))
        })
        .collect::<Vec<_>>()
        .join("\n");

    let html = format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Cceroby image search</title>
  <style>
    :root {{ color-scheme: light dark; font-family: system-ui, sans-serif; }}
    body {{ max-width: 52rem; margin: 4rem auto; padding: 0 1.25rem; }}
    form, section {{ display: grid; gap: 1rem; padding: 1.25rem; border: 1px solid #8886; border-radius: .75rem; }}
    fieldset {{ display: grid; gap: .5rem; border: 0; padding: 0; }}
    input[type="text"] {{ box-sizing: border-box; width: 100%; padding: .7rem; }}
    button {{ width: max-content; padding: .7rem 1.2rem; font-weight: 700; }}
    .artwork-grid {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(13rem, 1fr)); gap: 1rem; }}
    .artwork-card {{ display: grid; gap: .6rem; padding: .9rem; border: 1px solid #8886; border-radius: .6rem; }}
    .artwork-card h3, .artwork-card p {{ margin: 0; }}
    .artwork-card img {{ width: 100%; height: 13rem; object-fit: cover; border-radius: .4rem; background: #8883; }}
    .license-badge {{ width: max-content; padding: .2rem .5rem; border: 1px solid #8888; border-radius: 99rem; font-size: .85rem; }}
  </style>
</head>
<body>
  <main>
    <h1>Find card art</h1>
    <form action="/" method="get">
      <label>Search query <input name="query" type="text" required value="{query}"></label>
      <fieldset><legend>Sources</legend>{sources}</fieldset>
      <label>Culture or region <input name="culture" type="text" value="{culture}"></label>
      <button type="submit">Search</button>
    </form>
    {notice_section}
    <section aria-live="polite">
      <h2>Results</h2>
      <p>Loaded {} results.</p>
      <div class="artwork-grid">{cards}</div>
    </section>
  </main>
</body>
</html>"#,
        view.artworks.len()
    );
    with_page_lifecycle(html)
}

/// Render one trusted artwork and its exact attribution.
#[must_use]
pub fn render_detail_page(view: DetailView<'_>) -> String {
    let artwork = view.artwork;
    let preview_url = escape_html(&artwork_route_url("/preview", view.key));
    let optional_rows = [
        ("Creator", artwork.creator.as_deref()),
        ("Date", artwork.date.as_deref()),
        ("Culture or region", artwork.culture.as_deref()),
    ]
    .into_iter()
    .filter_map(|(label, value)| {
        value.map(|value| format!("<div><dt>{label}</dt><dd>{}</dd></div>", escape_html(value)))
    })
    .collect::<Vec<_>>()
    .join("\n");
    let license_url = escape_html(artwork.license.url());
    let object_url = escape_html(&artwork.object_url);
    let notice = view.notice.map_or_else(String::new, |notice| {
        let role = if notice.is_error() { "alert" } else { "status" };
        format!(
            "<p class=\"download-notice\" role=\"{role}\">{}</p>",
            escape_html(&notice.message())
        )
    });

    let html = format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{title} — Cceroby</title>
  <style>
    :root {{ color-scheme: light dark; font-family: system-ui, sans-serif; }}
    body {{ max-width: 58rem; margin: 3rem auto; padding: 0 1.25rem; }}
    main {{ display: grid; gap: 1.25rem; }}
    .preview {{ width: 100%; max-height: 70vh; object-fit: contain; border-radius: .65rem; background: #8882; }}
    dl {{ display: grid; gap: .65rem; }}
    dl div {{ display: grid; grid-template-columns: minmax(8rem, 12rem) 1fr; gap: .75rem; }}
    dt {{ font-weight: 700; }} dd {{ margin: 0; }}
    .attribution {{ padding: 1rem; border: 1px solid #8886; border-radius: .6rem; }}
    .download-form {{ display: grid; gap: .8rem; padding: 1rem; border: 1px solid #8886; border-radius: .6rem; }}
    .download-form input, .download-form textarea {{ box-sizing: border-box; width: 100%; padding: .7rem; }}
    .download-form textarea {{ min-height: 7rem; }}
    .download-notice {{ padding: .8rem; border: 1px solid #8888; border-radius: .5rem; }}
  </style>
</head>
<body>
  <main>
    <nav><a href="/">Back to search results</a></nav>
    <h1>{title}</h1>
    <img class="preview" src="{preview_url}" alt="{title}">
    <dl>
      {optional_rows}
      <div><dt>Institution</dt><dd>{institution}</dd></div>
      <div><dt>Source ID</dt><dd>{source_id}</dd></div>
      <div><dt>License</dt><dd><a href="{license_url}">{license}</a></dd></div>
      <div><dt>Source object</dt><dd><a href="{object_url}">{object_url}</a></dd></div>
    </dl>
    <section><h2>Ready-to-print attribution</h2><p class="attribution">{attribution}</p></section>
    <section>
      <h2>Download JPEG</h2>
      {notice}
      <form class="download-form" action="/download" method="post">
        <input name="source" type="hidden" value="{source}">
        <input name="id" type="hidden" value="{form_id}">
        <label>File name <input name="slug" type="text" required value="{slug}"></label>
        <label>Tags <textarea name="tags" placeholder="Separate tags with commas or new lines">{tags}</textarea></label>
        <button type="submit">Download</button>
      </form>
    </section>
  </main>
</body>
</html>"#,
        title = escape_html(&artwork.title),
        institution = escape_html(&artwork.institution),
        source_id = escape_html(&artwork.source_id),
        form_id = escape_html(view.key.id().as_str()),
        license = artwork.license.label(),
        attribution = escape_html(view.attribution),
        source = artwork.source.key(),
        slug = escape_html(view.slug),
        tags = escape_html(view.tags),
    );
    with_page_lifecycle(html)
}

#[must_use]
fn with_page_lifecycle(mut html: String) -> String {
    let position = html.rfind("</body>").unwrap_or(html.len());
    html.insert_str(position, PAGE_LIFECYCLE_SCRIPT);
    html
}

#[must_use]
fn artwork_route_url(path: &str, key: &ArtworkKey) -> String {
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("source", key.source().key())
        .append_pair("id", key.id().as_str())
        .finish();
    format!("{path}?{query}")
}

fn escape_html(raw: &str) -> String {
    raw.chars().fold(String::new(), |mut escaped, character| {
        let entity = match character {
            '&' => Some("&amp;"),
            '<' => Some("&lt;"),
            '>' => Some("&gt;"),
            '"' => Some("&quot;"),
            '\'' => Some("&#39;"),
            _ => None,
        };
        if let Some(entity) = entity {
            escaped.push_str(entity);
        } else {
            escaped.push(character);
        }
        escaped
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use crate::artwork::format_attribution;
    use crate::core::{
        CommercialLicense, Culture, ImageUrls, ProviderOutcome, ProviderPage, QueryText,
        SearchQuery, SourceSet, merge_page,
    };

    use super::*;

    fn query(raw: &str) -> SearchQuery {
        SearchQuery {
            query: QueryText::parse(raw).expect("query is valid"),
            sources: SourceSet::parse(&[SourceKind::MetropolitanMuseum])
                .expect("sources are valid"),
            culture: Culture::parse(Some("Japan".into())),
        }
    }

    fn artwork() -> Artwork {
        Artwork {
            source: SourceKind::ArtInstituteChicago,
            source_id: "1001".into(),
            title: "Mask <One>".into(),
            creator: Some("Maker unknown".into()),
            date: Some("1900".into()),
            culture: Some("Japan".into()),
            license: CommercialLicense::PublicDomain,
            image_urls: ImageUrls {
                thumbnail: "https://example.test/thumb.jpg".into(),
                display: "https://example.test/display.jpg".into(),
                original: None,
            },
            institution: "AIC & Friends".into(),
            provider_credit: Some("Gift of A & B".into()),
            object_url: "https://example.test/object?item=1001&view=1".into(),
        }
    }

    #[test]
    fn html_escaping_covers_markup_characters() {
        assert_eq!(escape_html("<&>\"'"), "&lt;&amp;&gt;&quot;&#39;");
    }

    #[test]
    fn local_artwork_urls_include_only_encoded_source_and_id() {
        let key =
            ArtworkKey::try_from_parts("wikimedia", "File:Mask (1900).jpg").expect("key is valid");
        assert_eq!(
            artwork_route_url("/thumb", &key),
            "/thumb?source=wikimedia&id=File%3AMask+%281900%29.jpg"
        );
        assert!(!artwork_route_url("/thumb", &key).contains("http"));
        assert!(!artwork_route_url("/detail", &key).contains("url="));
    }

    #[test]
    fn renderer_returns_complete_form_with_preserved_filters() {
        let search = query("mask");
        let session = crate::core::SearchSession::new(search);
        let html = render_search_page(session.view());
        assert!(html.starts_with("<!doctype html>"));
        assert!(html.ends_with("</html>"));
        assert!(html.contains("value=\"mask\""));
        assert!(html.contains("name=\"met\" value=\"true\" checked"));
        assert!(html.contains("name=\"culture\" type=\"text\" value=\"Japan\""));
        assert!(html.contains(">Search</button>"));
    }

    #[test]
    fn search_page_opens_liveness_and_sends_protected_escape_quit() {
        let session = crate::core::SearchSession::new(query("mask"));
        let html = render_search_page(session.view());

        assert!(html.contains("new EventSource('/live')"));
        assert!(html.contains("event.key !== 'Escape'"));
        assert!(html.contains("fetch('/quit', { method: 'POST'"));
        assert!(html.contains("credentials: 'same-origin'"));
        assert!(html.contains("keepalive: true"));
        assert!(html.find(PAGE_LIFECYCLE_SCRIPT).is_some_and(|position| {
            html.get(position + PAGE_LIFECYCLE_SCRIPT.len()..)
                .is_some_and(|tail| tail.starts_with("</body>"))
        }));
    }

    #[test]
    fn lifecycle_script_appends_when_a_fragment_has_no_body_end() {
        assert_eq!(
            with_page_lifecycle("fragment".into()),
            format!("fragment{PAGE_LIFECYCLE_SCRIPT}")
        );
    }

    #[test]
    fn renderer_escapes_query_and_shows_typed_provider_notices() {
        let mut session = crate::core::SearchSession::new(query("old"));
        session.begin_search(query("<mask>"));
        merge_page(
            &mut session,
            ProviderOutcome::Unavailable {
                source: SourceKind::MetropolitanMuseum,
            },
        );
        merge_page(
            &mut session,
            ProviderOutcome::Failed {
                source: SourceKind::ArtInstituteChicago,
            },
        );
        let html = render_search_page(session.view());
        assert!(html.contains("value=\"&lt;mask&gt;\""));
        assert!(html.contains("The Met</strong> is not available"));
        assert!(html.contains("Art Institute of Chicago</strong> could not complete"));
    }

    #[test]
    fn search_cards_use_only_local_thumbnail_and_detail_keys() {
        let mut session = crate::core::SearchSession::new(query("mask"));
        merge_page(
            &mut session,
            ProviderOutcome::Success(ProviderPage {
                source: SourceKind::ArtInstituteChicago,
                artworks: vec![artwork()],
                next_cursor: None,
            }),
        );
        let html = render_search_page(session.view());
        assert!(html.contains("Loaded 1 results."));
        assert!(html.contains("src=\"/thumb?source=aic&amp;id=1001\""));
        assert!(html.contains("href=\"/detail?source=aic&amp;id=1001\""));
        assert!(!html.contains("example.test/thumb.jpg"));
        assert!(html.contains("Mask &lt;One&gt;"));
        assert!(html.contains("AIC &amp; Friends"));
    }

    #[test]
    fn detail_renderer_shows_preview_metadata_license_attribution_and_back_navigation() {
        let artwork = artwork();
        let key = ArtworkKey::try_from_parts("aic", "1001").expect("key is valid");
        let attribution = format_attribution(&artwork);
        let html = render_detail_page(DetailView {
            artwork: &artwork,
            key: &key,
            attribution: &attribution,
            slug: "mask-one",
            tags: "",
            notice: None,
        });

        assert!(html.contains("href=\"/\">Back to search results"));
        assert!(html.contains("src=\"/preview?source=aic&amp;id=1001\""));
        for value in [
            "Maker unknown",
            "1900",
            "Japan",
            "AIC &amp; Friends",
            "1001",
            "Public domain",
        ] {
            assert!(html.contains(value), "missing {value}");
        }
        assert!(html.contains("Gift of A &amp; B"));
        assert!(!html.contains("example.test/display.jpg"));
    }

    #[test]
    fn displayed_attribution_uses_the_exact_future_xmp_value() {
        let mut artwork = artwork();
        artwork.title = "Mask One".into();
        artwork.provider_credit = Some("Museum gift".into());
        artwork.object_url = "https://example.test/object/1001".into();
        let key = ArtworkKey::try_from_parts("aic", "1001").expect("key is valid");
        let future_xmp_value = format_attribution(&artwork);
        let html = render_detail_page(DetailView {
            artwork: &artwork,
            key: &key,
            attribution: &future_xmp_value,
            slug: "mask-one",
            tags: "",
            notice: None,
        });
        assert!(html.contains(&format!("<p class=\"attribution\">{future_xmp_value}</p>")));
    }

    #[test]
    fn detail_renderer_omits_unavailable_optional_metadata_rows() {
        let mut artwork = artwork();
        artwork.creator = None;
        artwork.date = None;
        artwork.culture = None;
        let key = ArtworkKey::try_from_parts("aic", "1001").expect("key is valid");
        let attribution = format_attribution(&artwork);
        let html = render_detail_page(DetailView {
            artwork: &artwork,
            key: &key,
            attribution: &attribution,
            slug: "mask-one",
            tags: "",
            notice: None,
        });
        assert!(!html.contains("<dt>Creator</dt>"));
        assert!(!html.contains("<dt>Date</dt>"));
        assert!(!html.contains("<dt>Culture or region</dt>"));
    }

    #[test]
    fn detail_page_has_the_same_browser_lifecycle_script() {
        let artwork = artwork();
        let key = ArtworkKey::try_from_parts("aic", "1001").expect("key is valid");
        let attribution = format_attribution(&artwork);
        let html = render_detail_page(DetailView {
            artwork: &artwork,
            key: &key,
            attribution: &attribution,
            slug: "mask-one",
            tags: "",
            notice: None,
        });

        assert_eq!(html.matches(PAGE_LIFECYCLE_SCRIPT).count(), 1);
        assert!(html.contains("new EventSource('/live')"));
        assert!(html.contains("fetch('/quit', { method: 'POST'"));
    }

    #[test]
    fn detail_download_form_exposes_only_identity_slug_tags_and_a_typed_notice() {
        let artwork = artwork();
        let key = ArtworkKey::try_from_parts("aic", "1001").expect("key is valid");
        let attribution = format_attribution(&artwork);
        let notice = DownloadNotice::Replaced {
            path: "assets/mask.jpg".into(),
        };
        let html = render_detail_page(DetailView {
            artwork: &artwork,
            key: &key,
            attribution: &attribution,
            slug: "mask&lt;hostile",
            tags: "ritual & blue",
            notice: Some(&notice),
        });
        for field in ["source", "id", "slug", "tags"] {
            assert!(html.contains(&format!("name=\"{field}\"")));
        }
        for forbidden in ["url", "license", "attribution", "output_path"] {
            assert!(!html.contains(&format!("name=\"{forbidden}\"")));
        }
        assert!(html.contains(">Download</button>"));
        assert!(html.contains("Replaced assets/mask.jpg."));
        assert!(html.contains("mask&amp;lt;hostile"));
        assert!(html.contains("ritual &amp; blue"));
    }
}
