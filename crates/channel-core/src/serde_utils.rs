//! Shared serde helpers for channel config deserialization.

/// Deserialize a `Vec<String>` that also accepts integers (e.g. Telegram chat IDs)
/// and comma-separated strings (for env-var compat).
///
/// Used by `TelegramChannelConfig` (allow_from, allow_chats) and
/// `DiscordChannelConfig` (allow_guilds).
pub fn deserialize_string_or_int_vec<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de;

    struct StringOrIntVec;

    impl<'de> de::Visitor<'de> for StringOrIntVec {
        type Value = Vec<String>;

        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a list of strings or integers, or a comma-separated string")
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Vec<String>, A::Error>
        where
            A: de::SeqAccess<'de>,
        {
            let mut v = Vec::new();
            while let Some(val) = seq.next_element::<serde_json::Value>()? {
                match val {
                    serde_json::Value::String(s) => v.push(s),
                    serde_json::Value::Number(n) => v.push(n.to_string()),
                    _ => v.push(val.to_string()),
                }
            }
            Ok(v)
        }

        fn visit_str<E: de::Error>(self, s: &str) -> Result<Vec<String>, E> {
            Ok(s.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect())
        }

        fn visit_i64<E: de::Error>(self, n: i64) -> Result<Vec<String>, E> {
            Ok(vec![n.to_string()])
        }

        fn visit_u64<E: de::Error>(self, n: u64) -> Result<Vec<String>, E> {
            Ok(vec![n.to_string()])
        }
    }

    deserializer.deserialize_any(StringOrIntVec)
}
