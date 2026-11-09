//! A value as one path component, and back.

/// `value` as one path component: ASCII letters, digits and `-` as they
/// are, every other byte as `%XX`.
///
/// Keep in step with `is_value_character` in `inputs/pattern.rs`, which
/// holds that an encoded value has only these characters and `%`:
/// Also keep in step with the separator check in `entities.rs`, which
/// rejects formatting without a boundary outside these value characters.
/// Discovery binds a value without searching when the character after it
/// cannot be one of them, so a character added here must be added there.
pub(crate) fn encode_component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    push_encoded(&mut encoded, value);
    encoded
}

/// Add `value` to `text` as [`encode_component`] encodes it. Runs of
/// bytes kept as they are are added whole.
pub(crate) fn push_encoded(text: &mut String, value: &str) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut plain = 0;
    for (index, byte) in value.bytes().enumerate() {
        if byte.is_ascii_alphanumeric() || byte == b'-' {
            continue;
        }
        // A run kept as it is is ASCII, so it starts and ends between
        // characters; an empty one may not.
        if plain < index {
            text.push_str(&value[plain..index]);
        }
        text.push('%');
        text.push(char::from(HEX[usize::from(byte >> 4)]));
        text.push(char::from(HEX[usize::from(byte & 0xF)]));
        plain = index + 1;
    }
    text.push_str(&value[plain..]);
}

pub(crate) fn decode_component(encoded: &str) -> Option<String> {
    let bytes = encoded.as_bytes();
    let mut decoded = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = encoded.get(index + 1..index + 3)?;
            decoded.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

#[cfg(test)]
mod tests {
    use super::{decode_component, encode_component};

    #[test]
    fn components_keep_letters_digits_and_dashes_and_encode_the_rest() {
        assert_eq!(encode_component("sub-01"), "sub-01");
        assert_eq!(encode_component("a b/é"), "a%20b%2F%C3%A9");
        assert_eq!(encode_component("éa"), "%C3%A9a");
        assert_eq!(encode_component("__"), "%5F%5F");
        assert_eq!(encode_component(""), "");
        assert_eq!(decode_component("a%20b%2F%C3%A9").as_deref(), Some("a b/é"));
    }
}
