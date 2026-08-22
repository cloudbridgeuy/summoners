use crate::{error::EncodeError, wire::RecordV1};

/// Encode exactly one compact V1 JSON record followed by one line-feed.
pub fn encode_record(record: &RecordV1) -> Result<Vec<u8>, EncodeError> {
    let mut bytes = serde_json::to_vec(record).map_err(EncodeError::new)?;
    bytes.push(b'\n');
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use std::collections::BTreeMap;

    use super::*;
    use crate::wire::HeaderV1;

    #[test]
    fn encode_record_writes_one_exact_compact_header_line() {
        let metadata = BTreeMap::from([
            ("engine".to_string(), serde_json::json!("0.1.0")),
            ("shell".to_string(), serde_json::json!("cli")),
        ]);

        let encoded = encode_record(&RecordV1::Header(HeaderV1::new(metadata)))
            .expect("the header is serializable");

        assert_eq!(
            encoded,
            b"{\"sequence\":0,\"record\":\"header\",\"format\":\"summoners_match\",\"format_version\":1,\"metadata\":{\"engine\":\"0.1.0\",\"shell\":\"cli\"}}\n"
        );
    }
}
