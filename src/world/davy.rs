//! Davy Jones's Locker is XML-owned (ghostShip.xml); these cover the generated layout's sight and recovered door openers.
#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::dungeon::davy::{self, BOSS, KEYS, OPENERS, UNLIT};

    const SEED: u64 = 12351;

    fn locker(seed: u64) -> (World, i32) {
        let content = Arc::new(crate::world::test_content());
        let map = Arc::new(
            crate::dungeon::generate_selected(&content, "davy-jones-locker", seed).unwrap(),
        );
        let mut w = World::in_domain(content, map, WorldDomain::NonRealm).unwrap();
        w.name = davy::NAME.into();
        w.xml.template = Some("davy-jones-locker".into());
        w.encounter = true;
        let character: Character = serde_json::from_value(serde_json::json!({
            "schema":1,"lifetime":crate::store::FameLifetime::complete(),
            "class":0x300,"base":[200,100,15,0,75,15,15,10],
            "id":1,"account":1,"name":"Diver1","level":20,"exp":0,
            "fame":0,"hp":200,"mp":100,"revision":0,"alive":true,
            "items":[0xa14,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1]
        }))
        .unwrap();
        let p = w.enter(character, None).unwrap();
        (w, p)
    }
    fn stand(w: &mut World, p: i32, at: Pos) {
        w.players.get_mut(&p).unwrap().pos = at;
        for _ in 0..10 {
            w.now += 50;
            w.xml_step().unwrap();
            crate::world::xml::tests::drain_events(w).unwrap();
        }
    }
    /// Collects the keys up to `color` and opens the pair through its recovered opener (ghostShip.xml:1861-1925).
    fn open(w: &mut World, p: i32, color: usize, pair: &[i32], _step: (f32, f32)) {
        let back = w.players[&p].pos;
        w.players.get_mut(&p).unwrap().lab_conditions =
            crate::condition::bit(crate::condition::INVINCIBLE);
        for key in &KEYS[..=color] {
            if let Some(at) = w.entities.values().find(|e| e.kind == *key).map(|e| e.pos) {
                stand(w, p, at);
            }
        }
        let door = w.entities[&pair[0]].pos;
        let opener = w
            .entities
            .values()
            .filter(|e| e.kind == OPENERS[color])
            .map(|e| e.pos)
            .min_by(|a, b| a.distance(door).total_cmp(&b.distance(door)))
            .unwrap();
        stand(w, p, opener);
        w.players.get_mut(&p).unwrap().pos = back;
        assert!(pair.iter().all(|id| !w.entities.contains_key(id)));
    }
    fn seen(w: &World, p: i32, at: Pos) -> bool {
        w.players[&p].tiles.contains(&(at.x as i16, at.y as i16))
    }

    const PRISM: u16 = 0xae3;

    fn trickster(w: &mut World) -> i32 {
        let mut ch = Character::fresh(w.content.get(0x324).unwrap()).unwrap();
        (ch.id, ch.account) = (7, 7);
        ch.items[1] = PRISM.into();
        w.enter(ch, None).unwrap()
    }
    /// The closed `color` door pair nearest `inside`, a hall tile three steps out from it, and that outward step.
    fn neck(w: &World, color: usize, inside: Pos) -> (Vec<i32>, Pos, (f32, f32)) {
        let doors: Vec<_> = w
            .entities
            .values()
            .filter_map(|e| davy::door_color(e.kind).map(|c| (e.id, c)))
            .collect();
        let near = |id: &i32| w.entities[id].pos.distance(inside);
        let first = doors
            .iter()
            .filter(|(_, c)| *c == color)
            .map(|(id, _)| *id)
            .min_by(|a, b| near(a).total_cmp(&near(b)))
            .unwrap();
        let at = w.entities[&first].pos;
        let pair: Vec<_> = doors
            .iter()
            .filter(|(id, c)| *c == color && w.entities[id].pos.distance(at) < 1.5)
            .map(|(id, _)| *id)
            .collect();
        assert_eq!(pair.len(), 2);
        // Side-by-side halves sit in a horizontal wall, so the hall lies above or below; stacked halves face sideways.
        let flat = w.entities[&pair[0]].pos.y == w.entities[&pair[1]].pos.y;
        let step = if flat {
            (0., (at.y - inside.y).signum())
        } else {
            ((at.x - inside.x).signum(), 0.)
        };
        let outside = Pos {
            x: at.x + 3. * step.0,
            y: at.y + 3. * step.1,
        };
        (pair, outside, step)
    }
    #[test]
    fn a_locked_key_room_stays_dark_and_closed_to_blinks_until_its_door_opens() {
        use super::super::sight::tests::{blink, refused};
        let (mut w, _) = locker(SEED);
        let key = w.entities.values().find(|e| e.kind == KEYS[1]).unwrap();
        let (key, inside) = (key.id, key.pos);
        let (pair, outside, step) = neck(&w, 0, inside);
        let p = trickster(&mut w);
        w.players.get_mut(&p).unwrap().pos = outside;
        w.updates(p, true).unwrap();
        assert!(!seen(&w, p, inside) && !w.players[&p].known.contains(&key));
        assert!(pair.iter().all(|id| w.players[&p].known.contains(id)));
        refused(&mut w, p, PRISM, inside);
        // A blink along the hall, in sight, still lands and costs the prism's 60 MP.
        let sight = w.room_sight((outside.x as i32, outside.y as i32));
        let along = [3., -3., 4., -4., 5., -5., 2., -2.].map(|k| Pos {
            x: outside.x + k * step.1.abs(),
            y: outside.y + k * step.0.abs(),
        });
        let hall = *along
            .iter()
            .find(|t| w.walkable(**t) && sight.contains(&(t.x as i16, t.y as i16)))
            .unwrap();
        blink(&mut w, p, PRISM, hall).unwrap();
        let caster = &w.players[&p];
        assert_eq!(
            (caster.pos.x, caster.pos.y, caster.character.mp),
            (hall.x, hall.y, 192)
        );
        open(&mut w, p, 0, &pair, step);
        w.updates(p, false).unwrap();
        assert!(seen(&w, p, inside) && w.players[&p].known.contains(&key));
        blink(&mut w, p, PRISM, inside).unwrap();
    }

    #[test]
    fn the_boss_room_stays_dark_and_closed_to_blinks_until_the_yellow_door_opens() {
        use super::super::sight::tests::{blink, refused};
        let (mut w, _) = locker(SEED);
        let home = w.entities.values().find(|e| e.kind == BOSS).unwrap().home;
        let (pair, outside, step) = neck(&w, 3, home);
        let lamps: Vec<_> = w
            .entities
            .values()
            .filter(|e| e.kind == UNLIT)
            .map(|e| e.id)
            .collect();
        let p = trickster(&mut w);
        w.players.get_mut(&p).unwrap().pos = outside;
        w.updates(p, true).unwrap();
        for id in &lamps {
            assert!(!seen(&w, p, w.entities[id].pos) && !w.players[&p].known.contains(id));
        }
        let near = |id: &i32| w.entities[id].pos.distance(outside);
        let lamp = *lamps
            .iter()
            .min_by(|a, b| near(a).total_cmp(&near(b)))
            .unwrap();
        let target = w.entities[&lamp].pos;
        assert!(target.distance(outside) <= 20.);
        refused(&mut w, p, PRISM, target);
        open(&mut w, p, 3, &pair, step);
        w.updates(p, false).unwrap();
        assert!(seen(&w, p, target) && w.players[&p].known.contains(&lamp));
        blink(&mut w, p, PRISM, target).unwrap();
    }
}
