//! Bounded standard XMP construction and JPEG container embedding.

use img_parts::Bytes;
use img_parts::jpeg::{Jpeg, JpegSegment, markers};
use quick_xml::Writer;
use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use thiserror::Error;

use crate::core::Artwork;
use crate::download::Tags;

pub const XMP_IDENTIFIER: &[u8; 29] = b"http://ns.adobe.com/xap/1.0/\0";
pub const MAX_XMP_PACKET_BYTES: usize = 65_502;
const CCEROBY_NAMESPACE: &str = "https://github.com/cloudbridgeuy/summoners/ns/cceroby/1.0/";

/// One standard XMP packet that is safe to place in a JPEG APP1 segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XmpPacket(Vec<u8>);

impl XmpPacket {
    pub fn try_from_bytes(bytes: Vec<u8>) -> Result<Self, XmpError> {
        if bytes.len() > MAX_XMP_PACKET_BYTES {
            Err(XmpError::PacketTooLarge)
        } else {
            Ok(Self(bytes))
        }
    }

    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// XMP construction or JPEG container editing failed safely.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum XmpError {
    #[error("the XMP packet could not be built")]
    BuildFailed,
    #[error("the XMP packet is too large")]
    PacketTooLarge,
    #[error("the JPEG container is invalid")]
    InvalidJpeg,
}

#[must_use]
fn xml_1_0_text_is_valid(value: &str) -> bool {
    value.chars().all(|character| {
        matches!(character, '\u{9}' | '\u{a}' | '\u{d}' | '\u{20}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}')
    })
}

/// Build escaped UTF-8 RDF/XML from trusted artwork metadata and parsed tags.
pub fn build_xmp_packet(
    artwork: &Artwork,
    attribution: &str,
    tags: &Tags,
) -> Result<XmpPacket, XmpError> {
    let mut writer = Writer::new(Vec::new());
    let write = |writer: &mut Writer<Vec<u8>>, event| {
        writer.write_event(event).map_err(|_| XmpError::BuildFailed)
    };
    let write_text_element =
        |writer: &mut Writer<Vec<u8>>, name: &str, value: &str| -> Result<(), XmpError> {
            if !xml_1_0_text_is_valid(value) {
                return Err(XmpError::BuildFailed);
            }
            write(writer, Event::Start(BytesStart::new(name).into_owned()))?;
            write(writer, Event::Text(BytesText::new(value).into_owned()))?;
            write(writer, Event::End(BytesEnd::new(name).into_owned()))
        };
    let write_array = |writer: &mut Writer<Vec<u8>>,
                       property: &str,
                       container: &str,
                       values: &[&str],
                       language: bool|
     -> Result<(), XmpError> {
        write(writer, Event::Start(BytesStart::new(property).into_owned()))?;
        write(
            writer,
            Event::Start(BytesStart::new(container).into_owned()),
        )?;
        for value in values {
            if !xml_1_0_text_is_valid(value) {
                return Err(XmpError::BuildFailed);
            }
            let mut item = BytesStart::new("rdf:li");
            if language {
                item.push_attribute(("xml:lang", "x-default"));
            }
            write(writer, Event::Start(item))?;
            write(writer, Event::Text(BytesText::new(value).into_owned()))?;
            write(writer, Event::End(BytesEnd::new("rdf:li")))?;
        }
        write(writer, Event::End(BytesEnd::new(container).into_owned()))?;
        write(writer, Event::End(BytesEnd::new(property).into_owned()))
    };

    let mut xmpmeta = BytesStart::new("x:xmpmeta");
    xmpmeta.push_attribute(("xmlns:x", "adobe:ns:meta/"));
    write(&mut writer, Event::Start(xmpmeta))?;
    let mut rdf = BytesStart::new("rdf:RDF");
    rdf.push_attribute(("xmlns:rdf", "http://www.w3.org/1999/02/22-rdf-syntax-ns#"));
    write(&mut writer, Event::Start(rdf))?;
    let mut description = BytesStart::new("rdf:Description");
    description.push_attribute(("rdf:about", ""));
    description.push_attribute(("xmlns:dc", "http://purl.org/dc/elements/1.1/"));
    description.push_attribute(("xmlns:xmpRights", "http://ns.adobe.com/xap/1.0/rights/"));
    description.push_attribute(("xmlns:photoshop", "http://ns.adobe.com/photoshop/1.0/"));
    description.push_attribute(("xmlns:cceroby", CCEROBY_NAMESPACE));
    write(&mut writer, Event::Start(description))?;

    write_array(&mut writer, "dc:title", "rdf:Alt", &[&artwork.title], true)?;
    if let Some(creator) = artwork.creator.as_deref() {
        write_array(&mut writer, "dc:creator", "rdf:Seq", &[creator], false)?;
    }
    if let Some(date) = artwork.date.as_deref() {
        write_array(&mut writer, "dc:date", "rdf:Seq", &[date], false)?;
    }
    write_text_element(&mut writer, "dc:identifier", &artwork.source_id)?;
    write_text_element(&mut writer, "dc:source", &artwork.object_url)?;
    write_array(
        &mut writer,
        "dc:rights",
        "rdf:Alt",
        &[artwork.license.label()],
        true,
    )?;
    if !tags.as_slice().is_empty() {
        let tag_values = tags
            .as_slice()
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        write_array(&mut writer, "dc:subject", "rdf:Bag", &tag_values, false)?;
    }
    write_text_element(&mut writer, "xmpRights:WebStatement", artwork.license.url())?;
    write_array(
        &mut writer,
        "xmpRights:UsageTerms",
        "rdf:Alt",
        &[attribution],
        true,
    )?;
    write_text_element(
        &mut writer,
        "photoshop:Credit",
        artwork
            .provider_credit
            .as_deref()
            .unwrap_or(&artwork.institution),
    )?;
    write_text_element(&mut writer, "cceroby:provider", artwork.source.key())?;
    write_text_element(&mut writer, "cceroby:providerObjectId", &artwork.source_id)?;
    if let Some(culture) = artwork.culture.as_deref() {
        write_text_element(&mut writer, "cceroby:culture", culture)?;
    }
    write_text_element(&mut writer, "cceroby:attribution", attribution)?;

    write(&mut writer, Event::End(BytesEnd::new("rdf:Description")))?;
    write(&mut writer, Event::End(BytesEnd::new("rdf:RDF")))?;
    write(&mut writer, Event::End(BytesEnd::new("x:xmpmeta")))?;
    XmpPacket::try_from_bytes(writer.into_inner())
}

/// Replace standard XMP in a JPEG without decoding its pixels.
pub fn embed_xmp(jpeg_bytes: &[u8], packet: &XmpPacket) -> Result<Vec<u8>, XmpError> {
    let mut jpeg =
        Jpeg::from_bytes(Bytes::copy_from_slice(jpeg_bytes)).map_err(|_| XmpError::InvalidJpeg)?;
    jpeg.segments_mut().retain(|segment| {
        segment.marker() != markers::APP1 || !segment.contents().starts_with(XMP_IDENTIFIER)
    });

    let insertion = jpeg
        .segments()
        .iter()
        .enumerate()
        .filter(|(_, segment)| {
            (segment.marker() == markers::APP0
                && (segment.contents().starts_with(b"JFIF\0")
                    || segment.contents().starts_with(b"JFXX\0")))
                || (segment.marker() == markers::APP1
                    && segment.contents().starts_with(b"Exif\0\0"))
        })
        .map(|(position, _)| position + 1)
        .max()
        .unwrap_or(0);
    let mut contents = Vec::with_capacity(XMP_IDENTIFIER.len() + packet.as_bytes().len());
    contents.extend_from_slice(XMP_IDENTIFIER);
    contents.extend_from_slice(packet.as_bytes());
    jpeg.segments_mut().insert(
        insertion,
        JpegSegment::new_with_contents(markers::APP1, Bytes::from(contents)),
    );
    let mut output = Vec::with_capacity(jpeg.len());
    jpeg.encoder()
        .write_to(&mut output)
        .map_err(|_| XmpError::InvalidJpeg)?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use crate::core::{CommercialLicense, ImageUrls, SourceKind};

    use super::*;

    fn fixture(raw: &str) -> Vec<u8> {
        raw.trim()
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let text = std::str::from_utf8(pair).expect("fixture is ASCII");
                u8::from_str_radix(text, 16).expect("fixture is hexadecimal")
            })
            .collect()
    }

    fn native_jpeg() -> Vec<u8> {
        fixture(include_str!("../tests/fixtures/images/native-jpeg.hex"))
    }

    fn artwork() -> Artwork {
        Artwork {
            source: SourceKind::ArtInstituteChicago,
            source_id: "1001<&".into(),
            title: "Mask <One> & Two".into(),
            creator: Some("Maker & Co.".into()),
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
            object_url: "https://example.test/object?id=1001&view=1".into(),
        }
    }

    fn xmp_segments(jpeg: &Jpeg) -> Vec<&JpegSegment> {
        jpeg.segments()
            .iter()
            .filter(|segment| {
                segment.marker() == markers::APP1 && segment.contents().starts_with(XMP_IDENTIFIER)
            })
            .collect()
    }

    #[test]
    fn packet_maps_every_property_escapes_xml_and_preserves_tag_bag_order() {
        let attribution = "“Mask <One> & Two” — Maker & Co. Exact <print> line.";
        let tags = Tags::parse("ritual & mask, <blue>, third");
        let packet = build_xmp_packet(&artwork(), attribution, &tags).expect("XMP builds");
        let xml = std::str::from_utf8(packet.as_bytes()).expect("XMP is UTF-8");
        for property in [
            "dc:title",
            "dc:creator",
            "dc:date",
            "dc:identifier",
            "dc:source",
            "dc:rights",
            "dc:subject",
            "xmpRights:WebStatement",
            "xmpRights:UsageTerms",
            "photoshop:Credit",
            "cceroby:provider",
            "cceroby:providerObjectId",
            "cceroby:culture",
            "cceroby:attribution",
        ] {
            assert!(xml.contains(property), "missing {property}");
        }
        assert!(xml.contains(
            "xmlns:cceroby=\"https://github.com/cloudbridgeuy/summoners/ns/cceroby/1.0/\""
        ));
        assert!(!xml.contains("Mask <One> & Two"));
        assert!(xml.contains("Mask &lt;One&gt; &amp; Two"));
        assert!(xml.contains("Exact &lt;print&gt; line."));
        let first = xml.find("ritual &amp; mask").expect("first tag exists");
        let second = xml.find("&lt;blue&gt;").expect("second tag exists");
        let third = xml.find("third").expect("third tag exists");
        assert!(first < second && second < third);
        let escaped_attribution = attribution
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
        assert!(xml.contains(&escaped_attribution));
    }

    #[test]
    fn packet_omits_missing_optional_values_and_uses_institution_credit() {
        let mut artwork = artwork();
        artwork.creator = None;
        artwork.date = None;
        artwork.culture = None;
        artwork.provider_credit = None;
        let packet =
            build_xmp_packet(&artwork, "Exact attribution", &Tags::default()).expect("XMP builds");
        let xml = std::str::from_utf8(packet.as_bytes()).expect("XMP is UTF-8");
        for absent in ["dc:creator", "dc:date", "dc:subject", "cceroby:culture"] {
            assert!(!xml.contains(absent), "unexpected {absent}");
        }
        assert!(xml.contains("<photoshop:Credit>Art Institute of Chicago</photoshop:Credit>"));
        assert!(xml.contains("<cceroby:attribution>Exact attribution</cceroby:attribution>"));
    }

    #[test]
    fn bounded_packet_accepts_exact_limit_and_rejects_one_byte_more() {
        assert_eq!(
            XmpPacket::try_from_bytes(vec![b'x'; MAX_XMP_PACKET_BYTES])
                .expect("limit is accepted")
                .as_bytes()
                .len(),
            MAX_XMP_PACKET_BYTES
        );
        assert_eq!(
            XmpPacket::try_from_bytes(vec![b'x'; MAX_XMP_PACKET_BYTES + 1]),
            Err(XmpError::PacketTooLarge)
        );
    }

    #[test]
    fn packet_rejects_xml_1_0_invalid_provider_attribution_and_tag_text() {
        for invalid in ["bad\0text", "bad\u{b}text"] {
            let mut invalid_artwork = artwork();
            invalid_artwork.title = invalid.into();
            assert_eq!(
                build_xmp_packet(&invalid_artwork, "Attribution", &Tags::default()),
                Err(XmpError::BuildFailed)
            );
            assert_eq!(
                build_xmp_packet(&artwork(), invalid, &Tags::default()),
                Err(XmpError::BuildFailed)
            );
            assert_eq!(
                build_xmp_packet(&artwork(), "Attribution", &Tags::parse(invalid)),
                Err(XmpError::BuildFailed)
            );
        }
    }

    #[test]
    fn packet_accepts_xml_1_0_tab_newline_and_carriage_return() {
        let allowed = "tab\tline\nreturn\rtext";
        assert!(xml_1_0_text_is_valid(allowed));
        let mut allowed_artwork = artwork();
        allowed_artwork.title = allowed.into();
        assert!(build_xmp_packet(&allowed_artwork, allowed, &Tags::parse(allowed)).is_ok());
        assert!(!xml_1_0_text_is_valid("bad\0text"));
        assert!(!xml_1_0_text_is_valid("bad\u{b}text"));
    }

    #[test]
    fn native_jpeg_round_trip_keeps_scan_data_and_reads_the_packet() {
        let input = native_jpeg();
        let packet = build_xmp_packet(&artwork(), "Exact attribution", &Tags::parse("mask"))
            .expect("XMP builds");
        let output = embed_xmp(&input, &packet).expect("XMP embeds");
        let parsed = Jpeg::from_bytes(Bytes::from(output.clone())).expect("output parses");
        let segment = xmp_segments(&parsed)[0];
        assert_eq!(
            &segment.contents()[XMP_IDENTIFIER.len()..],
            packet.as_bytes()
        );
        let input_scan = input
            .windows(2)
            .position(|bytes| bytes == [0xff, 0xda])
            .expect("SOS");
        let output_scan = output
            .windows(2)
            .position(|bytes| bytes == [0xff, 0xda])
            .expect("SOS");
        assert_eq!(&input[input_scan..], &output[output_scan..]);
    }

    #[test]
    fn embedding_preserves_exif_orders_it_before_xmp_and_replaces_old_xmp() {
        let mut with_exif = Jpeg::from_bytes(Bytes::from(native_jpeg())).expect("JPEG parses");
        with_exif.segments_mut().insert(
            1,
            JpegSegment::new_with_contents(markers::APP1, Bytes::from_static(b"Exif\0\0proof")),
        );
        let mut source = Vec::new();
        with_exif
            .encoder()
            .write_to(&mut source)
            .expect("JPEG writes");
        let first = XmpPacket::try_from_bytes(b"<first/>".to_vec()).expect("packet fits");
        let second = XmpPacket::try_from_bytes(b"<second/>".to_vec()).expect("packet fits");
        let first_output = embed_xmp(&source, &first).expect("first XMP embeds");
        let second_output = embed_xmp(&first_output, &second).expect("second XMP replaces first");
        let parsed = Jpeg::from_bytes(Bytes::from(second_output)).expect("output parses");
        let xmp = xmp_segments(&parsed);
        assert_eq!(xmp.len(), 1);
        assert_eq!(
            &xmp[0].contents()[XMP_IDENTIFIER.len()..],
            second.as_bytes()
        );
        let exif_position = parsed
            .segments()
            .iter()
            .position(|segment| segment.contents().starts_with(b"Exif\0\0"))
            .expect("EXIF remains");
        let xmp_position = parsed
            .segments()
            .iter()
            .position(|segment| segment.contents().starts_with(XMP_IDENTIFIER))
            .expect("XMP exists");
        assert!(exif_position < xmp_position);
    }

    #[test]
    fn embedding_inserts_after_noncontiguous_jfif_and_exif_metadata() {
        let mut source = Jpeg::from_bytes(Bytes::from(native_jpeg())).expect("JPEG parses");
        source.segments_mut().insert(
            1,
            JpegSegment::new_with_contents(
                markers::APP2,
                Bytes::from_static(b"ICC_PROFILE\0proof"),
            ),
        );
        source.segments_mut().insert(
            2,
            JpegSegment::new_with_contents(markers::APP1, Bytes::from_static(b"Exif\0\0proof")),
        );
        let mut bytes = Vec::new();
        source.encoder().write_to(&mut bytes).expect("JPEG writes");
        let packet = XmpPacket::try_from_bytes(b"<proof/>".to_vec()).expect("packet fits");
        let output = embed_xmp(&bytes, &packet).expect("XMP embeds");
        let parsed = Jpeg::from_bytes(Bytes::from(output)).expect("output parses");
        let segments = parsed.segments();
        let icc = segments
            .iter()
            .position(|segment| segment.contents().starts_with(b"ICC_PROFILE\0"))
            .expect("ICC remains");
        let exif = segments
            .iter()
            .position(|segment| segment.contents().starts_with(b"Exif\0\0"))
            .expect("EXIF remains");
        let xmp = segments
            .iter()
            .position(|segment| segment.contents().starts_with(XMP_IDENTIFIER))
            .expect("XMP exists");
        assert!(icc < exif && exif < xmp);
    }

    #[test]
    fn invalid_jpeg_returns_a_typed_error() {
        let packet = XmpPacket::try_from_bytes(Vec::new()).expect("empty packet fits");
        assert_eq!(
            embed_xmp(b"not a JPEG", &packet),
            Err(XmpError::InvalidJpeg)
        );
    }
}
