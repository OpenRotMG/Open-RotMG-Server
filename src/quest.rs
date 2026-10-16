use crate::encounter::{Motion, Profile};
use anyhow::{Result, ensure};

pub const ROOTS: [u16; 4] = [0x91f, 0x91b, 0x928, 0x675];
pub const ROSTER: &[u16] = &[
    0x91b, 0x91c, 0x91d, 0x91e, 0x91f, 0x920, 0x921, 0x922, 0x923, 0x924, 0x928, 0x929, 0x92a,
    0x92b, 0x92c, 0x92d, 0x675, 0x676, 0x677, 0x6c1, 0x6c2, 0x65f, 0x660, 0x661,
];
pub fn final_root(kind: u16) -> bool {
    matches!(kind, 0x920 | 0x91c | 0x92d | 0x6c1)
}
pub fn profile(kind: u16) -> Option<Profile> {
    ROSTER.contains(&kind).then_some(Profile {
        motion: Motion::Wander,
        speed: 3.,
        interval: 50,
        range: 15.,
    })
}
pub fn validate(content: &crate::content::Content, kind: u16) -> Result<()> {
    ensure!(
        ROSTER.contains(&kind),
        "unselected Realm quest object {kind:#x}"
    );
    let object = content.get(kind)?;
    ensure!(object.enemy && object.hp > 0, "invalid Realm quest enemy");
    let indexes: &[u8] = match kind {
        0x6c2 => &[],
        0x920 => &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9],
        0x91b | 0x91c | 0x91d | 0x6c1 => &[0, 1],
        _ => &[0],
    };
    for index in indexes {
        ensure!(
            object.projectiles.contains_key(index),
            "quest {kind:#x} lacks selected projectile {index}"
        );
    }
    Ok(())
}
pub fn ring(index: u8, count: usize, angle: f32) -> Vec<(u8, f32)> {
    (0..count)
        .map(|n| {
            (
                index,
                angle + n as f32 * std::f32::consts::TAU / count as f32,
            )
        })
        .collect()
}
pub fn fan(index: u8, count: usize, degrees: f32, heading: f32) -> Vec<(u8, f32)> {
    (0..count)
        .map(|n| {
            (
                index,
                heading + (n as f32 - (count - 1) as f32 / 2.) * degrees.to_radians(),
            )
        })
        .collect()
}
