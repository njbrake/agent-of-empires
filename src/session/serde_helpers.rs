//! Shared serde helpers for config deserialization.

use serde::de;

/// Deserialize a field as either a single string or a Vec of strings.
/// Allows `key = "value"` as shorthand for `key = ["value"]` in TOML.
pub(crate) fn string_or_vec<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: de::Deserializer<'de>,
{
    struct Visitor;

    impl<'de> de::Visitor<'de> for Visitor {
        type Value = Vec<String>;

        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a string or array of strings")
        }

        fn visit_str<E: de::Error>(self, value: &str) -> Result<Vec<String>, E> {
            Ok(vec![value.to_string()])
        }

        fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<String>, A::Error> {
            let mut vec = Vec::new();
            while let Some(val) = seq.next_element()? {
                vec.push(val);
            }
            Ok(vec)
        }
    }

    deserializer.deserialize_any(Visitor)
}

/// Like `string_or_vec` but wraps the result in `Some(...)`.
/// For `Option<Vec<String>>` fields: absent = `None`, present = `Some(vec)`.
pub(crate) fn option_string_or_vec<'de, D>(deserializer: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: de::Deserializer<'de>,
{
    string_or_vec(deserializer).map(Some)
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct TestRequired {
        #[serde(deserialize_with = "super::string_or_vec")]
        items: Vec<String>,
    }

    #[derive(Deserialize)]
    struct TestOptional {
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "super::option_string_or_vec"
        )]
        items: Option<Vec<String>>,
    }

    #[test]
    fn string_or_vec_with_string() {
        let t: TestRequired = toml::from_str(r#"items = "hello""#).unwrap();
        assert_eq!(t.items, vec!["hello"]);
    }

    #[test]
    fn string_or_vec_with_array() {
        let t: TestRequired = toml::from_str(r#"items = ["a", "b"]"#).unwrap();
        assert_eq!(t.items, vec!["a", "b"]);
    }

    #[test]
    fn option_string_or_vec_with_string() {
        let t: TestOptional = toml::from_str(r#"items = "hello""#).unwrap();
        assert_eq!(t.items, Some(vec!["hello".to_string()]));
    }

    #[test]
    fn option_string_or_vec_with_array() {
        let t: TestOptional = toml::from_str(r#"items = ["a", "b"]"#).unwrap();
        assert_eq!(t.items, Some(vec!["a".to_string(), "b".to_string()]));
    }

    #[test]
    fn option_string_or_vec_absent() {
        let t: TestOptional = toml::from_str("").unwrap();
        assert_eq!(t.items, None);
    }
}
