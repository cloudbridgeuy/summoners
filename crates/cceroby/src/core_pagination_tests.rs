#![allow(clippy::expect_used)]

use super::*;

fn query(text: &str, sources: &[SourceKind], culture: Option<&str>) -> SearchQuery {
    SearchQuery {
        query: QueryText::parse(text).expect("query is valid"),
        sources: SourceSet::parse(sources).expect("sources are valid"),
        culture: Culture::parse(culture.map(str::to_owned)),
    }
}

fn artwork(source: SourceKind, source_id: &str) -> Artwork {
    Artwork {
        source,
        source_id: source_id.into(),
        title: source_id.into(),
        creator: None,
        date: None,
        culture: None,
        license: CommercialLicense::Cc0,
        image_urls: ImageUrls {
            thumbnail: "https://example.test/thumb.jpg".into(),
            display: "https://example.test/display.jpg".into(),
            original: Some("https://example.test/original.jpg".into()),
        },
        institution: source.label().into(),
        provider_credit: None,
        object_url: "https://example.test/object".into(),
    }
}

#[test]
fn batch_merge_round_robins_and_deduplicates_source_objects() {
    let mut session = SearchSession::new(query("mask", &SourceKind::ALL, None));
    let added = session.merge_batch(vec![
        ProviderOutcome::Success(ProviderPage {
            source: SourceKind::WikimediaCommons,
            artworks: vec![
                artwork(SourceKind::WikimediaCommons, "a"),
                artwork(SourceKind::WikimediaCommons, "b"),
            ],
            next_cursor: Some("20".into()),
        }),
        ProviderOutcome::Success(ProviderPage {
            source: SourceKind::ArtInstituteChicago,
            artworks: vec![
                artwork(SourceKind::ArtInstituteChicago, "1"),
                artwork(SourceKind::ArtInstituteChicago, "1"),
                artwork(SourceKind::ArtInstituteChicago, "2"),
            ],
            next_cursor: Some("2".into()),
        }),
        ProviderOutcome::Success(ProviderPage {
            source: SourceKind::MetropolitanMuseum,
            artworks: vec![artwork(SourceKind::MetropolitanMuseum, "7")],
            next_cursor: None,
        }),
    ]);

    assert_eq!(
        added
            .iter()
            .map(|artwork| (artwork.source, artwork.source_id.as_str()))
            .collect::<Vec<_>>(),
        vec![
            (SourceKind::ArtInstituteChicago, "1"),
            (SourceKind::MetropolitanMuseum, "7"),
            (SourceKind::WikimediaCommons, "a"),
            (SourceKind::ArtInstituteChicago, "2"),
            (SourceKind::WikimediaCommons, "b"),
        ]
    );
    assert!(session.view().has_more);
    assert_eq!(
        session.next_batch(),
        vec![
            ProviderCursor {
                source: SourceKind::ArtInstituteChicago,
                cursor: Some("2".into()),
            },
            ProviderCursor {
                source: SourceKind::ClevelandMuseum,
                cursor: None,
            },
            ProviderCursor {
                source: SourceKind::Smithsonian,
                cursor: None,
            },
            ProviderCursor {
                source: SourceKind::WikimediaCommons,
                cursor: Some("20".into()),
            },
        ]
    );
}

#[test]
fn empty_or_duplicate_only_pages_keep_cursor_and_later_exhaust() {
    let mut session = SearchSession::new(query("mask", &[SourceKind::ArtInstituteChicago], None));
    assert!(
        session
            .merge_batch(vec![ProviderOutcome::Success(ProviderPage {
                source: SourceKind::ArtInstituteChicago,
                artworks: Vec::new(),
                next_cursor: Some("2".into()),
            })])
            .is_empty()
    );
    assert!(session.view().has_more);
    assert_eq!(
        session
            .merge_batch(vec![ProviderOutcome::Success(ProviderPage {
                source: SourceKind::ArtInstituteChicago,
                artworks: vec![artwork(SourceKind::ArtInstituteChicago, "1")],
                next_cursor: Some("3".into()),
            })])
            .len(),
        1
    );
    assert!(
        session
            .merge_batch(vec![ProviderOutcome::Success(ProviderPage {
                source: SourceKind::ArtInstituteChicago,
                artworks: vec![artwork(SourceKind::ArtInstituteChicago, "1")],
                next_cursor: None,
            })])
            .is_empty()
    );
    assert!(!session.view().has_more);
}

#[test]
fn provider_failure_adds_one_notice_and_stops_only_that_provider() {
    let mut session = SearchSession::new(query(
        "mask",
        &[SourceKind::ArtInstituteChicago, SourceKind::ClevelandMuseum],
        None,
    ));
    let added = session.merge_batch(vec![
        ProviderOutcome::Failed {
            source: SourceKind::ArtInstituteChicago,
        },
        ProviderOutcome::Success(ProviderPage {
            source: SourceKind::ClevelandMuseum,
            artworks: vec![artwork(SourceKind::ClevelandMuseum, "2")],
            next_cursor: Some("20".into()),
        }),
    ]);
    assert_eq!(added.len(), 1);
    let _ = session.merge_batch(vec![ProviderOutcome::Failed {
        source: SourceKind::ArtInstituteChicago,
    }]);
    assert_eq!(
        session.view().notices,
        &[ProviderNotice::Failed {
            source: SourceKind::ArtInstituteChicago,
        }]
    );
    assert_eq!(
        session.next_batch(),
        vec![ProviderCursor {
            source: SourceKind::ClevelandMuseum,
            cursor: Some("20".into()),
        }]
    );
}

#[test]
fn changed_query_resets_results_notices_keys_and_cursors() {
    let mut session = SearchSession::new(query("mask", &[SourceKind::ArtInstituteChicago], None));
    let _ = session.merge_batch(vec![ProviderOutcome::Success(ProviderPage {
        source: SourceKind::ArtInstituteChicago,
        artworks: vec![artwork(SourceKind::ArtInstituteChicago, "1")],
        next_cursor: Some("2".into()),
    })]);
    let _ = session.merge_batch(vec![ProviderOutcome::Failed {
        source: SourceKind::ArtInstituteChicago,
    }]);
    assert!(session.begin_search(query(
        "new mask",
        &[SourceKind::WikimediaCommons],
        Some("MNAV"),
    )));
    assert!(session.view().artworks.is_empty());
    assert!(session.view().notices.is_empty());
    assert_eq!(
        session.next_batch(),
        vec![ProviderCursor {
            source: SourceKind::WikimediaCommons,
            cursor: None,
        }]
    );
}
