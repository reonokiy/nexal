//! Shared serde helpers for channel config deserialization.

/// Deserialize a `Vec<String>` that accepts strings, integers (e.g. Telegram
/// chat IDs), or comma-separated strings (for env-var compat). Each entry is
/// trimmed and has a leading `@` stripped; empty entries are dropped.
///
/// Used by `TelegramChannelConfig` (allow_from, allow_chats) and
/// `DiscordChannelConfig` (allow_guilds).
pub fn deserialize_string_or_int_vec<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de;

    fn normalize(raw: &str) -> impl Iterator<Item = String> + '_ {
        raw.split(',')
            .map(|s| s.trim().trim_start_matches('@').to_string())
            .filter(|s| !s.is_empty())
    }

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
            let mut out = Vec::new();
            while let Some(val) = seq.next_element::<serde_json::Value>()? {
                match val {
                    serde_json::Value::String(s) => out.extend(normalize(&s)),
                    serde_json::Value::Number(n) => out.extend(normalize(&n.to_string())),
                    other => out.extend(normalize(&other.to_string())),
                }
            }
            Ok(out)
        }

        fn visit_str<E: de::Error>(self, s: &str) -> Result<Vec<String>, E> {
            Ok(normalize(s).collect())
        }

        fn visit_i64<E: de::Error>(self, n: i64) -> Result<Vec<String>, E> {
            Ok(normalize(&n.to_string()).collect())
        }

        fn visit_u64<E: de::Error>(self, n: u64) -> Result<Vec<String>, E> {
            Ok(normalize(&n.to_string()).collect())
        }
    }

    deserializer.deserialize_any(StringOrIntVec)
}
