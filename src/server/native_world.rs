use super::*;

pub(super) fn accepts(id: u8) -> bool {
    matches!(
        id,
        wire::UPDATE_ACK | wire::MOVE | wire::PLAYER_SHOOT | 14 | 22 | 24 | 64
    )
}

pub(super) fn apply(
    w: &mut World,
    id: i32,
    packet: Packet,
    received: std::time::Instant,
    testing: bool,
) -> Result<()> {
    let p = w.players.get(&id).context("native player missing")?;
    if p.native_transferring()
        || matches!(packet.id, 7 | 38) && (p.death_pending || !p.character.alive)
    {
        return Ok(());
    }
    let mut r = NativeReader(Reader::new(&packet.body));
    match packet.id {
        wire::UPDATE_ACK => {
            r.done()?;
            if testing {
                w.players
                    .get_mut(&id)
                    .expect("native player")
                    .native_update_acked = true;
            }
        }
        wire::MOVE => {
            let tick = r.i32()?;
            let time = r.i32()?;
            let pos = Pos {
                x: r.f32()?,
                y: r.f32()?,
            };
            let count = r.i16()?;
            ensure!(
                (0..=128).contains(&count),
                lifecycle::protocol("movement history too long")
            );
            for _ in 0..count {
                r.i32()?;
                r.f32()?;
                r.f32()?;
            }
            r.done()?;
            w.acknowledge_tick(id, tick)?;
            match w.native_move_received(id, pos, (time, received)) {
                Ok(_) => {}
                Err(error) if lifecycle::rule_rejection(&error) => {
                    w.correct_position(id, time, &error.to_string())?
                }
                Err(error) => return Err(error),
            }
        }
        wire::PLAYER_SHOOT => {
            let time = r.i32()?;
            let bullet = r.u8()?;
            let kind = r.i16()? as u16;
            let pos = Pos {
                x: r.f32()?,
                y: r.f32()?,
            };
            let angle = r.f32()?;
            r.done()?;
            match w.native_shoot_received(id, (time, received), bullet, kind, pos, angle) {
                Ok(_) => {}
                Err(error)
                    if error
                        .downcast_ref::<crate::world::RejectedNativeShot>()
                        .is_some() =>
                {
                    w.correct_native_shot(id, time, bullet, kind, &format!("{error:#}"))?
                }
                Err(error) => return Err(error),
            }
        }
        14 | 22 => {
            let time = r.i32()?;
            r.done()?;
            if packet.id == 14 {
                w.acknowledge_goto(id, time)?;
            } else {
                w.acknowledge_shoot(id, time)?;
            }
        }
        24 => {
            let bullet = r.u8()?;
            let owner = r.i32()?;
            r.done()?;
            w.native_player_hit(id, owner, bullet)?;
        }
        64 => {
            let time = r.i32()?;
            let pos = Pos {
                x: r.f32()?,
                y: r.f32()?,
            };
            r.done()?;
            w.ground_damage(id, time, pos)?;
        }
        _ => unreachable!("unsupported native world packet"),
    }
    Ok(())
}
