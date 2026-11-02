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

    pub(super) fn invalidate_sight(&mut self) {
        if self.name == "Snake Pit"
            || self.name == crate::dungeon::davy::NAME
            || self.name == "Manor of the Immortals"
        {
            for p in self.players.values_mut() {
                p.tile_scan = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dungeon::davy;

    fn locker(door: u16, name: &str) -> (World, i32, [i32; 2], i32) {
        let (fixture, player) = super::super::movement_tests::fixture(0x18, false);
        let mut map = (*fixture.map).clone();
        for y in 0..map.height {
            let kind = match y {
                8 => door,
                9 => door + 1,
                _ => 0x1826,
            };
            map.cells[(y * map.width + 10) as usize].object = Some(kind);
        }
        map.cells[8 * 32 + 12].object = Some(0x1867);
        map.cells[25 * 32 + 24].object = Some(davy::BOSS);
        for x in 20..28 {
            map.cells[24 * 32 + x].object = Some(davy::UNLIT);
        }
        let mut w = World::new(fixture.content.clone(), Arc::new(map)).unwrap();
        w.name = name.into();
        w.prepare_locker().unwrap();
        let doors = w
            .entities
            .values()
            .filter(|e| matches!(e.kind, k if k == door || k == door + 1))
            .map(|e| e.id)
            .collect::<Vec<_>>();
        let hidden = w.entities.values().find(|e| e.kind == 0x1867).unwrap().id;
        let player = w
            .enter(fixture.players[&player].character.clone(), None)
            .unwrap();
        (w, player, doors.try_into().unwrap(), hidden)
    }

    #[test]
    fn davy_locked_doors_hide_undiscovered_tiles_and_objects_on_initial_and_repeated_updates() {
        for door in davy::DOORS {
            let (mut w, player, doors, hidden) = locker(door, davy::NAME);
            let mut character = w.players[&player].character.clone();
            character.account = 2;
            let peer = w.enter(character, None).unwrap();
            w.players.get_mut(&peer).unwrap().pos = Pos { x: 13.5, y: 8.5 };
            for initial in [true, false] {
                w.updates(player, initial).unwrap();
                let p = &w.players[&player];
                assert!(
                    doors.iter().all(|id| p.known.contains(id)),
                    "door {door:#x}"
                );
                assert!(
                    !p.known.contains(&hidden),
                    "closed door {door:#x} reveals object"
                );
                assert!(
                    !p.tiles.contains(&(12, 8)),
                    "closed door {door:#x} reveals floor"
                );
                assert!(
                    !p.tiles.contains(&(13, 8)),
                    "peer marker revealed undiscovered floor"
                );
                assert!(p.known.contains(&peer));
                assert_eq!(p.tile_scan, Some((8, 8)));
            }
        }
    }

    #[test]
    fn davy_key_touch_opens_both_halves_and_reveals_room_without_moving_or_initial_update() {
        for (color, door) in davy::DOORS.into_iter().enumerate() {
            let (mut w, player, doors, hidden) = locker(door, davy::NAME);
            let pos = Pos { x: 9.5, y: 8.5 };
            w.players.get_mut(&player).unwrap().pos = pos;
            w.updates(player, true).unwrap();
            w.locker_step().unwrap();
            assert!(doors.iter().all(|id| w.entities.contains_key(id)));
            assert_eq!(w.locker.as_ref().unwrap().keys, 0);
            let key = w.spawn_entity(davy::KEYS[color], pos, vec![]).unwrap();
            w.locker_step().unwrap();
            assert!(!w.entities.contains_key(&key));
            assert!(doors.iter().all(|id| !w.entities.contains_key(id)));
            assert_eq!(w.locker.as_ref().unwrap().keys, 1 << color);
            assert_eq!(
                w.players[&player].tile_scan, None,
                "door removal left stale sight"
            );
            assert_eq!(w.players[&player].pos.distance(pos), 0.);
            w.updates(player, false).unwrap();
            assert!(w.players[&player].known.contains(&hidden));
            assert!(w.players[&player].tiles.contains(&(12, 8)));
        }
    }

    #[test]
    fn generated_davy_arrival_does_not_reveal_later_keys_through_locked_doors() {
        let (fixture, player) = super::super::movement_tests::fixture(0x18, false);
        let map = davy::generate(&fixture.content, 863).unwrap();
        let mut w = World::new(fixture.content.clone(), Arc::new(map)).unwrap();
        w.name = davy::NAME.into();
        w.prepare_locker().unwrap();
        let player = w
            .enter(fixture.players[&player].character.clone(), None)
            .unwrap();
        for initial in [true, false] {
            w.updates(player, initial).unwrap();
            let p = &w.players[&player];
            for e in w
                .entities
                .values()
                .filter(|e| davy::KEYS[1..].contains(&e.kind))
            {
                assert!(
                    !p.known.contains(&e.id),
                    "later key {} revealed on arrival",
                    e.kind
                );
                assert!(!p.tiles.contains(&(e.pos.x as i16, e.pos.y as i16)));
            }
            assert!(
                w.entities
                    .values()
                    .any(|e| e.kind == davy::DOORS[0] && p.known.contains(&e.id))
            );
        }
    }

    #[test]
    fn snake_pit_blocker_removal_still_invalidates_sight_but_other_worlds_keep_radius_visibility() {
        for name in ["Snake Pit", "Nexus"] {
            let (mut w, player, doors, hidden) = locker(davy::DOORS[0], name);
            w.updates(player, true).unwrap();
            assert_eq!(w.players[&player].known.contains(&hidden), name == "Nexus");
            assert_eq!(w.players[&player].tiles.contains(&(12, 8)), name == "Nexus");
            for id in doors {
                w.remove_entity(id);
            }
            w.updates(player, false).unwrap();
            assert!(w.players[&player].known.contains(&hidden));
            assert!(w.players[&player].tiles.contains(&(12, 8)));
        }
    }
}
