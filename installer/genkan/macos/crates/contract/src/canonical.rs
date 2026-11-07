// SPDX-License-Identifier: GPL-3.0-or-later
//! Canonical JSON: the one byte form that every digest, signature and
//! journal record covers.
//!
//! - Objects have their keys in byte order, whatever order the serializer
//!   produced them in (so no crate enabling `serde_json/preserve_order` can
//!   change a digest).
//! - No insignificant whitespace.
//! - Numbers are integers. Sizes and offsets are bytes; there is no
//!   floating-point value in any document, so there is no float formatting to
//!   agree on.
//! - Strings are serde_json's escaping of UTF-8: `"`, `\` and control
//!   characters are escaped, everything else is literal.
//!
//! Readers require canonical input: a document whose bytes are not exactly the
//! canonical form of what they parse to is refused, so two byte strings never
//! name the same document.
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::fmt::Write as _;

use crate::{Digest, Error};

/// Serialize to canonical bytes.
pub fn to_vec<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>, Error> {
    let value = serde_json::to_value(value).map_err(|e| Error::Invalid(e.to_string()))?;
    let mut out = String::new();
    write(&value, &mut out)?;
    Ok(out.into_bytes())
}

/// The digest of a value's canonical form.
pub fn digest<T: Serialize + ?Sized>(value: &T) -> Result<Digest, Error> {
    Ok(Digest::of(&to_vec(value)?))
}

/// Parse canonical bytes of at most `limit` bytes. The document's `schema`
/// is checked before anything else, so a newer or unknown version is reported
/// as such rather than as whichever field first failed to parse.
pub fn from_slice<T: DeserializeOwned>(
    bytes: &[u8],
    limit: usize,
    schema: &str,
) -> Result<T, Error> {
    if bytes.len() > limit {
        return Err(Error::TooLarge {
            limit,
            actual: bytes.len(),
        });
    }
    let value: Value = serde_json::from_slice(bytes).map_err(|e| Error::Malformed(e.to_string()))?;
    check_schema(&value, schema)?;
    let mut canonical = String::new();
    write(&value, &mut canonical)?;
    if canonical.as_bytes() != bytes {
        return Err(Error::NotCanonical);
    }
    serde_json::from_value(value).map_err(|e| Error::Invalid(e.to_string()))
}

/// Parse a document whose bytes need not be canonical (a person's file, or
/// output from a tool), still checking its size and schema first. Nothing
/// read this way is digested or signed as it was received.
pub fn from_slice_relaxed<T: DeserializeOwned>(
    bytes: &[u8],
    limit: usize,
    schema: &str,
) -> Result<T, Error> {
    if bytes.len() > limit {
        return Err(Error::TooLarge {
            limit,
            actual: bytes.len(),
        });
    }
    let value: Value = serde_json::from_slice(bytes).map_err(|e| Error::Malformed(e.to_string()))?;
    check_schema(&value, schema)?;
    serde_json::from_value(value).map_err(|e| Error::Invalid(e.to_string()))
}

fn check_schema(value: &Value, schema: &str) -> Result<(), Error> {
    match value.get("schema").and_then(Value::as_str) {
        Some(found) if found == schema => Ok(()),
        Some(found) => Err(Error::UnsupportedSchema {
            expected: schema.into(),
            found: found.chars().take(80).collect(),
        }),
        None => Err(Error::UnsupportedSchema {
            expected: schema.into(),
            found: String::new(),
        }),
    }
}

/// How deep a document may nest. Every schema here is shallow; a deep
/// document is hostile or broken.
const MAX_DEPTH: usize = 32;

fn write(value: &Value, out: &mut String) -> Result<(), Error> {
    write_at(value, out, 0)
}

fn write_at(value: &Value, out: &mut String, depth: usize) -> Result<(), Error> {
    if depth > MAX_DEPTH {
        return Err(Error::Invalid("document nests too deeply".into()));
    }
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => {
            if let Some(i) = n.as_u64() {
                let _ = write!(out, "{i}");
            } else if let Some(i) = n.as_i64() {
                let _ = write!(out, "{i}");
            } else {
                return Err(Error::Invalid(
                    "documents contain integers only, never fractions".into(),
                ));
            }
        }
        Value::String(s) => {
            out.push_str(&serde_json::to_string(s).map_err(|e| Error::Invalid(e.to_string()))?)
        }
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_at(item, out, depth + 1)?;
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_unstable_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
            out.push('{');
            for (i, key) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(
                    &serde_json::to_string(key).map_err(|e| Error::Invalid(e.to_string()))?,
                );
                out.push(':');
                write_at(&map[key], out, depth + 1)?;
            }
            out.push('}');
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use serde_json::json;

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    #[serde(deny_unknown_fields)]
    struct Doc {
        schema: String,
        zeta: u64,
        alpha: String,
    }

    #[test]
    fn keys_are_sorted_and_compact() {
        let bytes = to_vec(&json!({"b": 1, "a": {"d": [1, 2], "c": "x\ny"}})).unwrap();
        assert_eq!(bytes, br#"{"a":{"c":"x\ny","d":[1,2]},"b":1}"#);
    }

    #[test]
    fn fractions_are_refused() {
        assert!(to_vec(&json!({"size": 1.5})).is_err());
    }

    #[test]
    fn round_trip_requires_canonical_bytes() {
        let doc = Doc {
            schema: "t/1".into(),
            zeta: 7,
            alpha: "é".into(),
        };
        let bytes = to_vec(&doc).unwrap();
        assert_eq!(from_slice::<Doc>(&bytes, 1024, "t/1").unwrap(), doc);
        let spaced = b"{\"alpha\":\"\\u00e9\", \"schema\":\"t/1\",\"zeta\":7}";
        assert!(matches!(
            from_slice::<Doc>(spaced, 1024, "t/1"),
            Err(Error::NotCanonical)
        ));
        assert_eq!(from_slice_relaxed::<Doc>(spaced, 1024, "t/1").unwrap(), doc);
    }

    #[test]
    fn schema_is_checked_first() {
        let bytes = br#"{"schema":"t/2","unexpected":true}"#;
        match from_slice::<Doc>(bytes, 1024, "t/1") {
            Err(Error::UnsupportedSchema { found, .. }) => assert_eq!(found, "t/2"),
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            from_slice::<Doc>(br#"{"zeta":1}"#, 1024, "t/1"),
            Err(Error::UnsupportedSchema { .. })
        ));
    }

    #[test]
    fn size_limit_is_checked_before_parsing() {
        assert!(matches!(
            from_slice::<Doc>(&[b' '; 2048], 1024, "t/1"),
            Err(Error::TooLarge { .. })
        ));
    }

    #[test]
    fn depth_is_bounded() {
        let mut v = json!(0);
        for _ in 0..40 {
            v = json!([v]);
        }
        assert!(to_vec(&v).is_err());
    }
}
