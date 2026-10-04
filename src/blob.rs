//! A file's git blob id: the SHA-1 of `blob <length>\0` and its bytes, as
//! `git hash-object` prints it. A `.spitdag` names each file its pipeline
//! was read from by it, so `git log --find-object` finds the revision.
//! Written here rather than taken from a crate: SPIT depends on two.

/// The git blob id of `content`, in lowercase hexadecimal.
pub(crate) fn git_blob_id(content: &[u8]) -> String {
    let mut message = format!("blob {}\0", content.len()).into_bytes();
    message.extend_from_slice(content);
    let digest = sha1(&message);
    let mut hex = String::with_capacity(40);
    for byte in digest {
        hex.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
        hex.push(char::from(b"0123456789abcdef"[usize::from(byte & 0xf)]));
    }
    hex
}

/// SHA-1 of `message`, as FIPS 180-4 defines it.
fn sha1(message: &[u8]) -> [u8; 20] {
    let mut state: [u32; 5] = [
        0x6745_2301,
        0xEFCD_AB89,
        0x98BA_DCFE,
        0x1032_5476,
        0xC3D2_E1F0,
    ];
    let length = u64::try_from(message.len()).expect("a file shorter than 2^64 bytes") * 8;
    let mut padded = message.to_vec();
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&length.to_be_bytes());
    for block in padded.chunks_exact(64) {
        let mut words = [0u32; 80];
        for (word, bytes) in words.iter_mut().zip(block.chunks_exact(4)) {
            *word = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        }
        for index in 16..80 {
            words[index] =
                (words[index - 3] ^ words[index - 8] ^ words[index - 14] ^ words[index - 16])
                    .rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = state;
        for (index, word) in words.iter().enumerate() {
            let (mixed, constant) = match index {
                0..=19 => ((b & c) | (!b & d), 0x5A82_7999),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let next = a
                .rotate_left(5)
                .wrapping_add(mixed)
                .wrapping_add(e)
                .wrapping_add(constant)
                .wrapping_add(*word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = next;
        }
        for (value, add) in state.iter_mut().zip([a, b, c, d, e]) {
            *value = value.wrapping_add(add);
        }
    }
    let mut digest = [0u8; 20];
    for (bytes, value) in digest.chunks_exact_mut(4).zip(state) {
        bytes.copy_from_slice(&value.to_be_bytes());
    }
    digest
}

#[cfg(test)]
mod tests {
    use super::git_blob_id;

    #[test]
    fn ids_match_what_git_hash_object_prints() {
        // `git hash-object` on an empty file, and on one holding `hello\n`.
        assert_eq!(git_blob_id(b""), "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391");
        assert_eq!(
            git_blob_id(b"hello\n"),
            "ce013625030ba8dba906f756967f9e9ca394464a"
        );
        // Longer than one block, so the padding spans two.
        let long = "spit\n".repeat(30);
        assert_eq!(
            git_blob_id(long.as_bytes()),
            "c171d0f701835920bb9eeabf316a47b075b93dff"
        );
    }
}
