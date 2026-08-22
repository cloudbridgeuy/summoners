//! Pure complete-page HTML rendering.

use crate::core::{ProviderNotice, SearchView, SourceKind};

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
        .map(|artwork| {
            format!(
                concat!(
                    "<article class=\"artwork-card\">",
                    "<div class=\"image-placeholder\" aria-label=\"Image preview is not loaded\"></div>",
                    "<h3>{}</h3><p>{}</p><span class=\"license-badge\">{}</span>",
                    "</article>"
                ),
                escape_html(&artwork.title),
                escape_html(&artwork.institution),
                artwork.license.label()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
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
    .image-placeholder {{ min-height: 9rem; border-radius: .4rem; background: #8883; }}
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
    )
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

    use crate::core::{
        Artwork, CommercialLicense, Culture, ImageUrls, ProviderOutcome, ProviderPage, QueryText,
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
            source_id: "1".into(),
            title: "Mask <One>".into(),
            creator: None,
            date: None,
            culture: None,
            license: CommercialLicense::PublicDomain,
            image_urls: ImageUrls {
                thumbnail: "https://example.test/thumb.jpg".into(),
                display: "https://example.test/display.jpg".into(),
                original: None,
            },
            institution: "AIC & Friends".into(),
            provider_credit: None,
            object_url: "https://example.test/object".into(),
        }
    }

    #[test]
    fn html_escaping_covers_markup_characters() {
        assert_eq!(escape_html("<&>\"'"), "&lt;&amp;&gt;&quot;&#39;");
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
    fn renderer_shows_loaded_count_and_placeholder_cards() {
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
        assert!(html.contains("class=\"artwork-grid\""));
        assert!(html.contains("class=\"image-placeholder\""));
        assert!(html.contains("Mask &lt;One&gt;"));
        assert!(html.contains("AIC &amp; Friends"));
        assert!(html.contains("class=\"license-badge\">Public domain"));
    }
}
