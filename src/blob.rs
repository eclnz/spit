//! A file's git blob id: the SHA-1 of `blob <length>\0` and its bytes, as
//! `git hash-object` prints it. A `.spitdag` names each file its pipeline
//! was read from by it, so `git log --find-object` finds the revision.
//! Written here rather than taken from a crate: SPIT depends on two.

/// The git blob id of `content`, in lowercase hexadecimal.
pub(crate) fn git_blob_id(content: &[u8]) -> String {
    let header = format!("blob {}\0", content.len());
    let digest = sha1(&[header.as_bytes(), content]);
    let mut hex = String::with_capacity(40);
    for byte in digest {
        hex.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
        hex.push(char::from(b"0123456789abcdef"[usize::from(byte & 0xf)]));
    }
    hex
}

/// SHA-1, as FIPS 180-4 defines it, of `parts` one after another. Whole
/// blocks are hashed where they lie; only a block that spans two parts,
/// and the padded end, are gathered into a buffer.
fn sha1(parts: &[&[u8]]) -> [u8; 20] {
    let mut state: [u32; 5] = [
        0x6745_2301,
        0xEFCD_AB89,
        0x98BA_DCFE,
        0x1032_5476,
        0xC3D2_E1F0,
    ];
    let mut buffer = [0u8; 64];
    let mut filled = 0;
    let mut length: u64 = 0;
    for part in parts {
        length += u64::try_from(part.len()).expect("a file shorter than 2^61 bytes");
        let mut rest = *part;
        if filled > 0 {
            let take = rest.len().min(64 - filled);
            buffer[filled..filled + take].copy_from_slice(&rest[..take]);
            filled += take;
            rest = &rest[take..];
            if filled < 64 {
                continue;
            }
            compress(&mut state, &buffer);
        }
        let mut blocks = rest.chunks_exact(64);
        for block in &mut blocks {
            compress(&mut state, block);
        }
        let tail = blocks.remainder();
        buffer[..tail.len()].copy_from_slice(tail);
        filled = tail.len();
    }
    // The end: a 1 bit, zeros, then the length in bits, over one block or
    // two.
    buffer[filled] = 0x80;
    buffer[filled + 1..].fill(0);
    if filled >= 56 {
        compress(&mut state, &buffer);
        buffer.fill(0);
    }
    buffer[56..].copy_from_slice(&(length * 8).to_be_bytes());
    compress(&mut state, &buffer);
    let mut digest = [0u8; 20];
    for (bytes, value) in digest.chunks_exact_mut(4).zip(state) {
        bytes.copy_from_slice(&value.to_be_bytes());
    }
    digest
}

/// Fold one 64-byte block into `state`.
fn compress(state: &mut [u32; 5], block: &[u8]) {
    let mut words = [0u32; 80];
    for (word, bytes) in words.iter_mut().zip(block.chunks_exact(4)) {
        *word = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    }
    for index in 16..80 {
        words[index] =
            (words[index - 3] ^ words[index - 8] ^ words[index - 14] ^ words[index - 16])
                .rotate_left(1);
    }
    let [mut a, mut b, mut c, mut d, mut e] = *state;
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

#[cfg(test)]
mod tests {
    use super::{compress, git_blob_id};

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

    #[test]
    fn hashing_in_place_matches_hashing_one_padded_buffer() {
        // The header and content meet, and the padding falls, at every
        // offset in a block over these lengths.
        for length in 0..300 {
            let content: Vec<u8> = (0..length).map(|byte| (byte * 7 % 251) as u8).collect();
            let mut message = format!("blob {length}\0").into_bytes();
            message.extend_from_slice(&content);
            let bits = (message.len() as u64) * 8;
            message.push(0x80);
            while message.len() % 64 != 56 {
                message.push(0);
            }
            message.extend_from_slice(&bits.to_be_bytes());
            let mut state = [
                0x6745_2301,
                0xEFCD_AB89,
                0x98BA_DCFE,
                0x1032_5476,
                0xC3D2_E1F0,
            ];
            for block in message.chunks_exact(64) {
                compress(&mut state, block);
            }
            let expected: String = state.iter().map(|word| format!("{word:08x}")).collect();
            assert_eq!(git_blob_id(&content), expected, "{length} bytes");
        }
    }
}
