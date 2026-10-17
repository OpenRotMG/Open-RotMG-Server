use super::{Pos, Writer};

#[derive(Clone, Copy)]
pub(super) struct EnemyShot {
    pub bullet: u8,
    pub projectile: u8,
    pub start: Pos,
    pub angle: f32,
    pub damage: i32,
}

pub(super) fn dazed(shots: Vec<(u8, f32)>) -> Vec<(u8, f32)> {
    if shots.len() <= 1 {
        return shots;
    }
    let count = (shots.len() / 2).max(1);
    (0..count)
        .map(|i| shots[(2 * i + 1) * shots.len() / (2 * count)])
        .collect()
}

pub(super) fn packets(owner: i32, shots: &[EnemyShot]) -> Vec<Vec<u8>> {
    let mut packets = Vec::new();
    let mut offset = 0;
    while offset < shots.len() {
        let first = shots[offset];
        let mut count = 1;
        let mut gap = 0.;
        while count < usize::from(u8::MAX) {
            let Some(next) = shots.get(offset + count) else {
                break;
            };
            if next.bullet != first.bullet.wrapping_add(count as u8)
                || next.projectile != first.projectile
                || next.damage != first.damage
                || next.start.x != first.start.x
                || next.start.y != first.start.y
            {
                break;
            }
            if count == 1 {
                gap = next.angle - first.angle;
            } else {
                // Native count/gap reconstruction rounds complete rings in f32.
                let tolerance = 8. * f32::EPSILON;
                if (next.angle - (first.angle + gap * count as f32)).abs() > tolerance {
                    break;
                }
            }
            count += 1;
        }
        let mut out = Writer::new();
        out.u8(first.bullet)
            .i32(owner)
            .u8(first.projectile)
            .f32(first.start.x)
            .f32(first.start.y)
            .f32(first.angle)
            .i16(first.damage as i16);
        if count > 1 {
            out.u8(count as u8).f32(gap);
        }
        packets.push(out.0);
        offset += count;
    }
    packets
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::Reader;
    fn readback(owner: i32, packets: &[Vec<u8>]) -> Vec<EnemyShot> {
        let mut shots = Vec::new();
        for body in packets {
            let mut r = Reader::new(body);
            let bullet = r.u8().unwrap();
            assert_eq!(r.i32().unwrap(), owner);
            let projectile = r.u8().unwrap();
            let start = Pos {
                x: r.f32().unwrap(),
                y: r.f32().unwrap(),
            };
            let angle = r.f32().unwrap();
            let damage = i32::from(r.i16().unwrap());
            let (count, gap) = if body.len() > 20 {
                (r.u8().unwrap(), r.f32().unwrap())
            } else {
                (1, 0.)
            };
            r.done().unwrap();
            for i in 0..count {
                shots.push(EnemyShot {
                    bullet: bullet.wrapping_add(i),
                    projectile,
                    start,
                    angle: angle + gap * f32::from(i),
                    damage,
                });
            }
        }
        shots
    }
    fn compare(expected: &[EnemyShot], actual: &[EnemyShot]) {
        assert_eq!(expected.len(), actual.len());
        for (a, b) in expected.iter().zip(actual) {
            assert_eq!(
                (a.bullet, a.projectile, a.damage, a.start.x, a.start.y),
                (b.bullet, b.projectile, b.damage, b.start.x, b.start.y)
            );
            assert!((a.angle - b.angle).abs() < 0.00001);
        }
    }
    fn ring(count: usize) -> Vec<EnemyShot> {
        (0..count)
            .map(|i| EnemyShot {
                bullet: 254u8.wrapping_add(i as u8),
                projectile: 9,
                start: Pos { x: 13.5, y: 7.5 },
                angle: 0.37 + i as f32 * std::f32::consts::TAU / 12.,
                damage: 65,
            })
            .collect()
    }
    #[test]
    fn native_count_gap_preserves_ring_ids_geometry_and_fixed_damage() {
        let shots = ring(12);
        let encoded = packets(17, &shots);
        assert_eq!(encoded.len(), 1);
        assert_eq!(encoded[0].len(), 25);
        compare(&shots, &readback(17, &encoded));
        let encoded = packets(17, &shots[..1]);
        assert_eq!(encoded[0].len(), 20);
        compare(&shots[..1], &readback(17, &encoded));
    }
    #[test]
    fn incompatible_damage_type_position_angle_and_ids_remain_independent() {
        let mut shots = ring(12);
        shots[1].damage = 66;
        shots[3].projectile = 8;
        shots[5].start.x = 15.5;
        shots[7].angle += 0.4;
        shots[9].bullet = 99;
        let encoded = packets(17, &shots);
        assert!(encoded.len() > 1);
        compare(&shots, &readback(17, &encoded));
    }
    #[test]
    fn native_count_bound_splits_without_dropping_or_reordering() {
        let mut shots = ring(272);
        for shot in &mut shots {
            shot.angle = 0.;
        }
        let encoded = packets(17, &shots);
        assert_eq!(encoded.len(), 2);
        for (body, count) in encoded.iter().zip([255, 17]) {
            let mut r = Reader::new(body);
            r.take(20).unwrap();
            assert_eq!(r.u8().unwrap(), count);
        }
        compare(&shots, &readback(17, &encoded));
        assert!(packets(17, &[]).is_empty());
    }
}
