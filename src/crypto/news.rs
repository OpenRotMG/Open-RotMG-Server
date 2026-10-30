use anyhow::{Result, ensure};

const INITIALIZE: &[u8] = b"\x0a\x05\x0a\x0b\x93\x0b\x27\x26\xd7\x10\x08\x00\x00\x07\x94\x02\xc3\x03\x78\xad\xd7\x63\x04\xd0\x30\x62\x04\x2a\x12\x03\x00\x00\x29\xd1\x76\x11\x06\x02\x00\xd0\xd1\x61\xd5\x30";
const GUARD: &[u8] =
    b"\xd0\x30\xd0\x66\xd5\x30\x12\x01\x00\x00\x47\x27\x63\x04\x26\xd7\x02\x02\x02";

pub(super) fn patch(data: &mut [u8]) -> Result<usize> {
    let positions: Vec<_> = data
        .windows(INITIALIZE.len())
        .enumerate()
        .filter_map(|(i, bytes)| (bytes == INITIALIZE).then_some(i))
        .collect();
    ensure!(positions.len() <= 1, "ambiguous character/news initializer");
    if let Some(&offset) = positions.first() {
        // Fame's Continue reuses this view; initialize once like 7.0: https://github.com/moistosaurus/realm-cli/blob/a13c855dc5e09ee301fd0a66fcd303cfb5dff74e/src/com/company/assembleegameclient/screens/CharacterSelectionAndNewsScreen.as#L82-L91
        data[offset + 6..offset + 6 + GUARD.len()].copy_from_slice(GUARD);
    }
    Ok(positions.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn news_guard_requires_unique_initializer_and_preserves_branches_and_layout() {
        let original = [b"prefix".as_slice(), INITIALIZE, b"suffix".as_slice()].concat();
        let mut patched = original.clone();
        assert_eq!(patch(&mut patched).unwrap(), 1);
        assert_eq!(patched.len(), original.len());
        assert_eq!(&patched[..12], &original[..12]);
        assert_eq!(&patched[12..12 + GUARD.len()], GUARD);
        assert_eq!(&patched[12 + GUARD.len()..], &original[12 + GUARD.len()..]);
        assert_eq!(patch(&mut patched).unwrap(), 0);
        let mut duplicate = INITIALIZE.repeat(2);
        assert!(patch(&mut duplicate).is_err());
        assert_eq!(duplicate, INITIALIZE.repeat(2));
        for i in 0..INITIALIZE.len() {
            let mut changed = INITIALIZE.to_vec();
            changed[i] ^= 1;
            let before = changed.clone();
            assert_eq!(patch(&mut changed).unwrap(), 0);
            assert_eq!(changed, before);
        }
    }
}
