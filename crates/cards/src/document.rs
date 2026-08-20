use crate::{LoadPhase, SetLoadCause, SetLoadError, v1};

pub(crate) fn decode(bytes: &[u8]) -> Result<v1::dto::Set, SetLoadError> {
    let source = decode_utf8(bytes)?;
    let version = read_version(source)?;
    match version {
        1 => decode_v1(source),
        found => Err(SetLoadError::new(
            LoadPhase::Version,
            Some(found),
            "schema_version",
            SetLoadCause::UnsupportedSchemaVersion { found },
        )),
    }
}

fn decode_utf8(bytes: &[u8]) -> Result<&str, SetLoadError> {
    std::str::from_utf8(bytes).map_err(|error| {
        SetLoadError::new(
            LoadPhase::Utf8,
            None,
            "$",
            SetLoadCause::InvalidUtf8 {
                valid_up_to: error.valid_up_to(),
                error_len: error.error_len(),
            },
        )
    })
}

fn read_version(source: &str) -> Result<i64, SetLoadError> {
    let table: toml::Table = toml::from_str(source).map_err(|error| {
        SetLoadError::new(
            LoadPhase::Version,
            None,
            "$",
            SetLoadCause::TomlSyntax {
                message: error.message().to_string(),
            },
        )
    })?;
    let Some(value) = table.get("schema_version") else {
        return Err(SetLoadError::new(
            LoadPhase::Version,
            None,
            "schema_version",
            SetLoadCause::MissingSchemaVersion,
        ));
    };
    value.as_integer().ok_or_else(|| {
        SetLoadError::new(
            LoadPhase::Version,
            None,
            "schema_version",
            SetLoadCause::InvalidSchemaVersion {
                found: value.type_str().to_string(),
            },
        )
    })
}

fn decode_v1(source: &str) -> Result<v1::dto::Set, SetLoadError> {
    let deserializer = toml::de::Deserializer::parse(source).map_err(|error| {
        SetLoadError::new(
            LoadPhase::Decode,
            Some(1),
            "$",
            SetLoadCause::SchemaDecode {
                message: error.message().to_string(),
            },
        )
    })?;
    serde_path_to_error::deserialize(deserializer).map_err(|error| {
        let path = error.path().to_string();
        let message = error.inner().message().to_string();
        SetLoadError::new(
            LoadPhase::Decode,
            Some(1),
            if path.is_empty() { "$" } else { &path },
            SetLoadCause::SchemaDecode { message },
        )
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use crate::{DocumentKind, SetLoadCause};

    const MINIMAL: &str = r#"
schema_version = 1
id = "test-set"
revision = 1
name = "Test Set"
cards = []
"#;

    #[test]
    fn utf8_decode_accepts_text() {
        assert_eq!(decode_utf8(b"Set").unwrap(), "Set");
    }

    #[test]
    fn utf8_decode_reports_the_invalid_byte_location() {
        let error = decode_utf8(&[b'a', 0xff]).unwrap_err();
        assert_eq!(error.document, DocumentKind::Set);
        assert_eq!(error.phase, LoadPhase::Utf8);
        assert_eq!(error.path, "$");
        assert_eq!(
            error.cause,
            SetLoadCause::InvalidUtf8 {
                valid_up_to: 1,
                error_len: Some(1),
            }
        );
    }

    #[test]
    fn version_reader_requires_the_field() {
        let error = read_version("id = \"test\"").unwrap_err();
        assert_eq!(error.phase, LoadPhase::Version);
        assert_eq!(error.path, "schema_version");
        assert_eq!(error.cause, SetLoadCause::MissingSchemaVersion);
    }

    #[test]
    fn version_reader_requires_an_integer() {
        let error = read_version("schema_version = \"1\"").unwrap_err();
        assert_eq!(
            error.cause,
            SetLoadCause::InvalidSchemaVersion {
                found: "string".to_string(),
            }
        );
    }

    #[test]
    fn version_reader_reports_toml_syntax() {
        let error = read_version("schema_version = [").unwrap_err();
        assert!(matches!(error.cause, SetLoadCause::TomlSyntax { .. }));
    }

    #[test]
    fn exact_dispatch_rejects_unsupported_versions_before_v1_fields() {
        let error = decode(b"schema_version = 2\nunknown = true").unwrap_err();
        assert_eq!(error.phase, LoadPhase::Version);
        assert_eq!(error.schema_version, Some(2));
        assert_eq!(
            error.cause,
            SetLoadCause::UnsupportedSchemaVersion { found: 2 }
        );
    }

    #[test]
    fn v1_decode_accepts_the_private_shape() {
        let decoded = decode_v1(MINIMAL).unwrap();
        assert_eq!(decoded.schema_version, 1);
        assert_eq!(decoded.id, "test-set");
    }

    #[test]
    fn v1_decode_rejects_unknown_top_level_fields() {
        let source = format!("{MINIMAL}\nunknown = true\n");
        let error = decode_v1(&source).unwrap_err();
        assert_eq!(error.phase, LoadPhase::Decode);
        assert_eq!(error.schema_version, Some(1));
        assert!(matches!(error.cause, SetLoadCause::SchemaDecode { .. }));
    }
}
