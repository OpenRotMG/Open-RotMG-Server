use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub(super) struct Cache {
    statuses: BTreeMap<i32, BTreeMap<u8, Vec<u8>>>,
    pub elided: u64,
}
impl Cache {
    pub fn encode(&mut self, id: i32, status: Vec<u8>) -> Vec<u8> {
        assert!(status.len() >= 14, "truncated object status for {id}");
        let count = i16::from_be_bytes(status[12..14].try_into().unwrap());
        assert!(count >= 0, "negative stat count for {id}");
        let previous = self.statuses.entry(id).or_default();
        let mut seen = [false; 256];
        let mut out = status[..14].to_vec();
        let mut cursor = 14;
        let mut fields = Vec::new();
        for _ in 0..count {
            let key = *status.get(cursor).expect("truncated internal stat key");
            assert!(!seen[key as usize], "duplicate stat {key} for {id}");
            seen[key as usize] = true;
            cursor += 1;
            let length = if matches!(key, 31 | 62) {
                let prefix = status
                    .get(cursor..cursor + 2)
                    .expect("truncated stat string length");
                2 + usize::from(u16::from_be_bytes(prefix.try_into().unwrap()))
            } else {
                4
            };
            let value = status
                .get(cursor..cursor + length)
                .expect("truncated internal stat value");
            let changed = previous.get(&key).is_none_or(|old| old != value);
            if changed {
                previous.insert(key, value.to_vec());
            }
            fields.push((key, value, changed));
            cursor += length;
        }
        assert_eq!(cursor, status.len(), "trailing stat bytes for {id}");
        // Flash caches the star badge in its name bitmap; stat 56 invalidates it.
        let stars_changed = fields
            .iter()
            .any(|(key, _, changed)| *key == 30 && *changed);
        let mut changed = 0i16;
        for (key, value, different) in fields {
            if different || key == 56 && stars_changed {
                out.push(key);
                out.extend(value);
                changed += 1;
            }
        }
        previous.retain(|key, _| seen[*key as usize]);
        out[12..14].copy_from_slice(&changed.to_be_bytes());
        if changed == 0 {
            self.elided += 1;
        }
        out
    }
    pub fn retain(&mut self, known: &BTreeSet<i32>) {
        self.statuses.retain(|id, _| known.contains(id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::Writer;

    fn status(x: f32, hp: i32) -> Vec<u8> {
        let mut w = Writer::new();
        w.i32(7).f32(x).f32(2.).i16(1).u8(1).i32(hp);
        w.0
    }
    #[test]
    fn movement_survives_unchanged_stats_and_health_changes_publish() {
        let mut cache = Cache::default();
        assert_eq!(cache.encode(7, status(1., 800)), status(1., 800));
        let moved = cache.encode(7, status(3., 800));
        assert_eq!(&moved[..12], &status(3., 800)[..12]);
        assert_eq!(&moved[12..], &[0, 0]);
        assert_eq!(cache.elided, 1);
        assert_eq!(cache.encode(7, status(3., 799)), status(3., 799));
    }
    #[test]
    fn health_changes_do_not_repeat_name_bitmap_or_guild_setters() {
        let rich = |hp| {
            let mut w = Writer::new();
            w.i32(7)
                .f32(1.)
                .f32(2.)
                .i16(4)
                .u8(1)
                .i32(hp)
                .u8(31)
                .utf("Raider")
                .u8(56)
                .i32(1)
                .u8(62)
                .utf("Guild");
            w.0
        };
        let mut cache = Cache::default();
        assert_eq!(cache.encode(7, rich(800)), rich(800));
        assert_eq!(cache.encode(7, rich(799)), status(1., 799));
    }
    #[test]
    fn star_changes_invalidate_the_name_bitmap_in_either_field_order() {
        for reversed in [false, true] {
            let fields = |stars| {
                if reversed {
                    [(56, 1), (30, stars)]
                } else {
                    [(30, stars), (56, 1)]
                }
            };
            let status = |stars, guild| {
                let mut w = Writer::new();
                w.i32(7).f32(1.).f32(2.).i16(if guild { 3 } else { 2 });
                for (key, value) in fields(stars) {
                    w.u8(key).i32(value);
                }
                if guild {
                    w.u8(62).utf("Guild");
                }
                w.0
            };
            let mut cache = Cache::default();
            cache.encode(7, status(1, true));
            assert_eq!(cache.encode(7, status(2, true)), status(2, false));
            assert_eq!(&cache.encode(7, status(2, true))[12..], &[0, 0]);
        }
    }
    #[test]
    fn conditions_inventory_and_utf8_strings_publish_only_on_change() {
        let rich = |flags, item, name: &str, guild: &str| {
            let mut w = Writer::new();
            w.i32(7)
                .f32(1.)
                .f32(2.)
                .i16(5)
                .u8(29)
                .i32(flags)
                .u8(31)
                .utf(name)
                .u8(56)
                .i32(1)
                .u8(62)
                .utf(guild)
                .u8(8)
                .i32(item);
            w.0
        };
        let mut cache = Cache::default();
        cache.encode(7, rich(1 << 26, 0xaf6, "Raider", "Guild"));
        let changed = cache.encode(7, rich(0, 0xc02, "戦士", "Temple"));
        let mut expected = Writer::new();
        expected
            .i32(7)
            .f32(1.)
            .f32(2.)
            .i16(4)
            .u8(29)
            .i32(0)
            .u8(31)
            .utf("戦士")
            .u8(62)
            .utf("Temple")
            .u8(8)
            .i32(0xc02);
        assert_eq!(changed, expected.0);
        assert_eq!(
            &cache.encode(7, rich(0, 0xc02, "戦士", "Temple"))[12..],
            &[0, 0]
        );
    }
    #[test]
    fn recipients_and_reintroduced_objects_have_independent_baselines() {
        let mut first = Cache::default();
        let mut second = Cache::default();
        first.encode(7, status(1., 800));
        assert_eq!(second.encode(7, status(1., 800)), status(1., 800));
        first.retain(&BTreeSet::new());
        assert_eq!(first.encode(7, status(1., 800)), status(1., 800));
    }
}
