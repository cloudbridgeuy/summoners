//! Deterministic full download pipeline evidence for native JPEG and TIFF inputs.
#![deny(clippy::unwrap_used, clippy::expect_used)]

use cceroby::artwork::format_attribution;
use cceroby::asset_writer::write_atomic_replace;
use cceroby::core::{Artwork, CommercialLicense, ImageUrls, SourceKind};
use cceroby::download::{Slug, Tags, WriteDisposition};
use cceroby::image::DownloadedImage;
use cceroby::xmp::{XMP_IDENTIFIER, build_xmp_packet, embed_xmp};
use img_parts::Bytes;
use img_parts::jpeg::{Jpeg, JpegSegment, markers};
use std::process::Command;

fn fixture(raw: &str) -> Vec<u8> {
    raw.trim()
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| match std::str::from_utf8(pair) {
            Ok(text) => match u8::from_str_radix(text, 16) {
                Ok(byte) => byte,
                Err(error) => panic!("fixture must contain hexadecimal bytes: {error}"),
            },
            Err(error) => panic!("fixture must be ASCII: {error}"),
        })
        .collect()
}

fn native_jpeg() -> Vec<u8> {
    fixture(include_str!("fixtures/images/native-jpeg.hex"))
}

fn source_tiff() -> Vec<u8> {
    fixture(include_str!("fixtures/images/source-tiff.hex"))
}

fn artwork() -> Artwork {
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

fn parse_jpeg(bytes: Vec<u8>) -> Jpeg {
    match Jpeg::from_bytes(Bytes::from(bytes)) {
        Ok(jpeg) => jpeg,
        Err(error) => panic!("fixture must be a valid JPEG: {error}"),
    }
}

#[test]
fn native_jpeg_keeps_scan_and_exif_and_replaces_xmp_once() {
    let mut source = parse_jpeg(native_jpeg());
    source.segments_mut().insert(
        1,
        JpegSegment::new_with_contents(markers::APP1, Bytes::from_static(b"Exif\0\0proof")),
    );
    let mut source_bytes = Vec::new();
    if let Err(error) = source.encoder().write_to(&mut source_bytes) {
        panic!("source JPEG must encode: {error}");
    }
    let attribution = format_attribution(&artwork());
    let first = match build_xmp_packet(&artwork(), &attribution, &Tags::parse("first")) {
        Ok(packet) => packet,
        Err(error) => panic!("first packet must build: {error}"),
    };
    let second = match build_xmp_packet(&artwork(), &attribution, &Tags::parse("ritual, blue")) {
        Ok(packet) => packet,
        Err(error) => panic!("second packet must build: {error}"),
    };
    let first_output = match embed_xmp(&source_bytes, &first) {
        Ok(bytes) => bytes,
        Err(error) => panic!("first XMP must embed: {error}"),
    };
    let final_output = match embed_xmp(&first_output, &second) {
        Ok(bytes) => bytes,
        Err(error) => panic!("second XMP must replace the first: {error}"),
    };
    let Some(source_scan) = source_bytes
        .windows(2)
        .position(|window| window == [0xff, 0xda])
    else {
        panic!("source must contain a scan");
    };
    let Some(final_scan) = final_output
        .windows(2)
        .position(|window| window == [0xff, 0xda])
    else {
        panic!("output must contain a scan");
    };
    assert_eq!(&source_bytes[source_scan..], &final_output[final_scan..]);

    let parsed = parse_jpeg(final_output);
    let exif_position = parsed
        .segments()
        .iter()
        .position(|segment| segment.contents().starts_with(b"Exif\0\0"));
    let xmp = parsed
        .segments()
        .iter()
        .enumerate()
        .filter(|(_, segment)| {
            segment.marker() == markers::APP1 && segment.contents().starts_with(XMP_IDENTIFIER)
        })
        .collect::<Vec<_>>();
    assert_eq!(xmp.len(), 1);
    assert!(matches!(exif_position, Some(position) if position < xmp[0].0));
    assert_eq!(
        &xmp[0].1.contents()[XMP_IDENTIFIER.len()..],
        second.as_bytes()
    );
}

#[test]
fn tiff_converts_then_uses_the_same_xmp_and_atomic_replace_path() {
    let downloaded = match DownloadedImage::try_from_magic(source_tiff()) {
        Ok(image) => image,
        Err(error) => panic!("TIFF magic must parse: {error}"),
    };
    let jpeg = match downloaded.into_jpeg_quality_100() {
        Ok(jpeg) => jpeg,
        Err(error) => panic!("TIFF must convert at quality 100: {error}"),
    };
    let decoded = match image::load_from_memory_with_format(&jpeg, image::ImageFormat::Jpeg) {
        Ok(image) => image,
        Err(error) => panic!("converted JPEG must decode: {error}"),
    };
    assert_eq!((decoded.width(), decoded.height()), (2, 2));

    let attribution = format_attribution(&artwork());
    let packet = match build_xmp_packet(&artwork(), &attribution, &Tags::parse("tiff, ritual")) {
        Ok(packet) => packet,
        Err(error) => panic!("XMP must build: {error}"),
    };
    let embedded = match embed_xmp(&jpeg, &packet) {
        Ok(bytes) => bytes,
        Err(error) => panic!("XMP must embed: {error}"),
    };
    let output = match tempfile::tempdir() {
        Ok(directory) => directory,
        Err(error) => panic!("temporary output must exist: {error}"),
    };
    let slug = match Slug::parse("tiff-mask") {
        Ok(slug) => slug,
        Err(error) => panic!("slug must parse: {error}"),
    };
    let created = match write_atomic_replace(output.path(), &slug, b"old") {
        Ok(saved) => saved,
        Err(error) => panic!("first write must succeed: {error}"),
    };
    assert_eq!(created.disposition, WriteDisposition::Created);
    let replaced = match write_atomic_replace(output.path(), &slug, &embedded) {
        Ok(saved) => saved,
        Err(error) => panic!("replacement must succeed: {error}"),
    };
    assert_eq!(replaced.disposition, WriteDisposition::Replaced);
    let written = match std::fs::read(&replaced.path) {
        Ok(bytes) => bytes,
        Err(error) => panic!("written JPEG must read: {error}"),
    };
    let parsed = parse_jpeg(written);
    assert_eq!(
        parsed
            .segments()
            .iter()
            .filter(|segment| segment.contents().starts_with(XMP_IDENTIFIER))
            .count(),
        1
    );
    assert_eq!(
        match std::fs::read_dir(output.path()) {
            Ok(entries) => entries.count(),
            Err(error) => panic!("output directory must read: {error}"),
        },
        1
    );
}

#[test]
fn imagemagick_recognizes_xmp_and_exif_profiles_when_available() {
    if Command::new("magick").arg("-version").output().is_err() {
        return;
    }
    let mut source = parse_jpeg(native_jpeg());
    source.segments_mut().insert(
        1,
        JpegSegment::new_with_contents(markers::APP1, Bytes::from_static(b"Exif\0\0proof")),
    );
    let mut source_bytes = Vec::new();
    if let Err(error) = source.encoder().write_to(&mut source_bytes) {
        panic!("source JPEG must encode: {error}");
    }
    let attribution = format_attribution(&artwork());
    let packet = match build_xmp_packet(&artwork(), &attribution, &Tags::parse("ritual")) {
        Ok(packet) => packet,
        Err(error) => panic!("XMP must build: {error}"),
    };
    let embedded = match embed_xmp(&source_bytes, &packet) {
        Ok(bytes) => bytes,
        Err(error) => panic!("XMP must embed: {error}"),
    };
    let output = match tempfile::tempdir() {
        Ok(directory) => directory,
        Err(error) => panic!("temporary output must exist: {error}"),
    };
    let path = output.path().join("profile.jpg");
    if let Err(error) = std::fs::write(&path, embedded) {
        panic!("inspection JPEG must write: {error}");
    }
    let inspected = match Command::new("magick")
        .arg("identify")
        .arg("-verbose")
        .arg(&path)
        .output()
    {
        Ok(output) => output,
        Err(error) => panic!("ImageMagick must inspect the JPEG: {error}"),
    };
    assert!(
        inspected.status.success(),
        "{}",
        String::from_utf8_lossy(&inspected.stderr)
    );
    let report = String::from_utf8_lossy(&inspected.stdout);
    assert!(report.contains("Profile-exif"), "{report}");
    assert!(report.contains("Profile-xmp"), "{report}");
}
