use super::*;

impl World {
    pub(super) fn room_sight(&self, center: (i32, i32)) -> BTreeSet<(i16, i16)> {
        // Reveal the enclosing wall face, not unopened rooms: https://www.youtube.com/watch?v=gWLn80rNsxU&t=30
        let mut seen = BTreeSet::from([(center.0 as i16, center.1 as i16)]);
        let mut queue = VecDeque::from([center]);
        while let Some((x, y)) = queue.pop_front() {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let (x, y) = (x + dx, y + dy);
                    if x < 0
                        || y < 0
                        || x >= self.map.width
                        || y >= self.map.height
                        || (x - center.0).pow(2) + (y - center.1).pow(2) > 20 * 20
                        || !seen.insert((x as i16, y as i16))
                    {
                        continue;
                    }
                    if self.map.cells[(y * self.map.width + x) as usize].ground != 0xff
                        && !self
                            .entities_at(Pos {
                                x: x as f32 + 0.5,
                                y: y as f32 + 0.5,
                            })
                            .any(|e| self.content.objects[&e.kind].blocks_sight)
                    {
                        queue.push_back((x, y));
                    }
                }
            }
        }
        seen
    }

    /// Worlds whose closed rooms stay undiscovered until a door, shelf or grate opens. In Davy Jones's Locker a room
    /// behind a closed gate stays black on the minimap until it opens: https://www.youtube.com/watch?v=2C8h5dswJmg&t=282
    /// (2012-10-02), https://www.youtube.com/watch?v=3BJGr4JLXu0&t=103 (2012-08-13). In the Mad Lab the minimap stays
    /// black north of Dr Terrible's room while its Lab Emergency Walls (BlocksSight) stand, though the Horrific
    /// Creation's room lies in sight range: https://www.youtube.com/watch?v=C4sKt3Se0T0&t=40 (2012-09-27).
    pub(super) fn room_sight_world(&self) -> bool {
        matches!(
            self.name.as_str(),
            "Snake Pit"
                | "Manor of the Immortals"
                | crate::dungeon::davy::NAME
                | crate::dungeon::lab::NAME
        )
    }

    pub(super) fn invalidate_sight(&mut self) {
        if self.room_sight_world() {
            for p in self.players.values_mut() {
                p.tile_scan = None;
            }
        }
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    /// Casts teleport item `kind` from `owner`'s ability slot at `target`, with 252 MP and the cooldown ready.
    pub(in crate::world) fn blink(w: &mut World, owner: i32, kind: u16, target: Pos) -> Result<()> {
        let p = w.players.get_mut(&owner).unwrap();
        p.character.items[1] = kind.into();
        p.character.mp = 252;
        p.ability_ready_ms = 0;
        let item = w.content.get(kind)?.clone();
        let slot = Slot {
            object: owner,
            slot: 1,
            kind: kind.into(),
        };
        w.activate_ability(owner, slot, &item, target)
    }
    /// Asserts a refused blink spent neither mana nor cooldown and left the caster in place.
    pub(in crate::world) fn refused(w: &mut World, owner: i32, kind: u16, target: Pos) {
        let from = w.players[&owner].pos;
        let error = blink(w, owner, kind, target).unwrap_err();
        assert!(error.to_string().contains("out of sight"), "{error}");
        let p = &w.players[&owner];
        assert_eq!((p.character.mp, p.ability_ready_ms), (252, 0));
        assert_eq!((p.pos.x, p.pos.y), (from.x, from.y));
    }
}
