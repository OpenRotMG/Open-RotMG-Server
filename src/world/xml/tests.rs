use super::*;
use crate::assert_matches;
use crate::{
    behavior::World as _,
    content::{Cell, game},
};
use std::path::Path;
fn authored(name: &str, id: u16, body: &str) -> String {
    format!(
        r#"<Object type="{id}" id="{name}" disposition="active" provenance="authored" reason="native World regression" origin="independent fixture">{body}</Object>"#
    )
}
const LISTS: &str = r#"<StringLists><StringList id="Pirate"><Strings id="new"><String>Welcome.</String></Strings><Strings id="killed"><String>{KILLER} defeated the Pirate.</String></Strings><Strings id="death"><String>The Pirate is gone.</String></Strings></StringList></StringLists>"#;
fn fixture(body: &str) -> (World, i32, i32) {
    fixture_program(body, "<Behavior>RemainVisible</Behavior>", LISTS)
}
fn fixture_program(body: &str, child: &str, lists: &str) -> (World, i32, i32) {
    let mut content = Content::load(Path::new("data")).unwrap();
    let mut rules = authored("Pirate", 0x600, body) + &authored("Piratess", 0x601, child);
    for (kind, service) in [
        (6, "projectile"),
        (0x120, "scenery"),
        (0x100, "scenery"),
        (0x504, "container"),
        (0x307, "player"),
        (0xc2b, "inventory"),
        (0x1407, "companion"),
    ] {
        let name = &content.objects[&kind].name;
        rules += &format!(
            r#"<Object type="{kind}" id="{name}" disposition="native" nativeService="{service}" provenance="authored" origin="selected native fixture service" reason="explicit bounded World fixture dependency"/>"#
        );
    }
    let inputs=crate::content::game::test_inputs::selected([
        ("game/123.5.1/game.xml",br#"<Game schema="1"><Include file="rules.xml"/><Include file="strings.xml"/><Root object="Pirate"/><Geometry file="geometry.xml"/></Game>"#.to_vec()),
        ("game/123.5.1/rules.xml",format!("<Objects>{rules}</Objects>").into_bytes()),
        ("game/123.5.1/strings.xml",lists.as_bytes().to_vec()),
        ("game/123.5.1/geometry.xml",br#"<Geometry schema="1"><Template id="Stamp" provenance="authored" origin="native test" reason="transparent stamp regression" width="2" height="2" rotation="fixed" rooms="1"><Anchor x="0" y="0"/><CharMap char="." ground="Grass"/><CharMap char="c" clearObject="true"/><CharMap char="o" object="Piratess"/><Pattern><Row value=". "/><Row value="co"/></Pattern></Template></Geometry>"#.to_vec()),
    ]);
    content.game = Some(Arc::new(game::compile_fixture(&content, inputs).unwrap()));
    let mut map = flat_map(128);
    map.elevation = Some(vec![120; 128 * 128].into());
    let mut w = World::in_domain(Arc::new(content), Arc::new(map), WorldDomain::Realm).unwrap();
    let mut ch = Character::fresh(w.content.get(0x307).unwrap()).unwrap();
    ch.account = 1;
    ch.id = 1;
    ch.name = "Ada".into();
    let player = w.enter(ch, None).unwrap();
    w.players.get_mut(&player).unwrap().pos = Pos { x: 65.5, y: 64.5 };
    let id = spawn(&mut w, 0x600, 64.5, 64.5);
    (w, player, id)
}
fn spawn(w: &mut World, kind: u16, x: f32, y: f32) -> i32 {
    w.spawn_entity(kind, Pos { x, y }, vec![]).unwrap()
}
fn flat_map(size: usize) -> Map {
    Map {
        elevation: None,
        width: size as i32,
        height: size as i32,
        cells: vec![
            Cell {
                ground: 0x18,
                object: None,
                config: String::new(),
                terrain: 0,
                region: 0
            };
            size * size
        ],
        spawns: vec![(64.5, 64.5)],
    }
}
fn flat_nonrealm(content: Arc<Content>) -> World {
    World::in_domain(content, Arc::new(flat_map(128)), WorldDomain::NonRealm).unwrap()
}
fn frame(w: &mut World, at: u64) {
    w.now = at;
    w.xml_step().unwrap();
    if let Err(error) = drain_events(w) {
        eprintln!("journal drain retained event: {error:#}");
    }
}
fn scope(w: &World, id: i32) -> Scope {
    Scope {
        identity: w.xml_identity(id).unwrap(),
        issuer: id,
        order: None,
        listener: None,
        generation: 0,
    }
}
fn step(w: &World, id: i32, local: u32) -> Step {
    Step {
        token: Token {
            scope: scope(w, id),
            local,
        },
        active_ms: 0,
        age_ms: 0,
        delta_ms: 50,
        emissions: 0,
        text_choice: None,
    }
}
#[test]
fn xml_identity_of_kin_born_after_the_camp_leader_died_keeps_its_encounter() {
    // A golems native run panicked here: Metal Golem kin keep refilling the camp once its leader is dead.
    let (mut w, _) = crate::world::population::camp_fixture(0x62d);
    let leader = w.camps[0].leader.unwrap();
    let kin = |w: &World| -> Vec<i32> {
        (w.camp_members(0))
            .filter(|e| e.kind == 0x62d && e.id != leader)
            .map(|e| e.id)
            .collect()
    };
    while kin(&w).len() < 2 {
        w.now += 60_000;
        w.repopulate_families().unwrap();
    }
    w.remove_entity(leader);
    w.remove_entity(kin(&w)[0]);
    w.now += 60_000;
    w.repopulate_families().unwrap();
    let born = *kin(&w).last().unwrap();
    assert!(born > leader && kin(&w).len() == 2);
    assert_eq!(w.xml_identity(born).unwrap().encounter, Some(leader as u64));
}
#[test]
fn xml_runtime_world_missing_rules_refuses() {
    let c = Arc::new(Content::load(Path::new("data")).unwrap());
    let result = World::in_domain(c, Arc::new(flat_map(128)), WorldDomain::Realm);
    assert!(
        result
            .err()
            .unwrap()
            .to_string()
            .contains("retained validated XML")
    );
}
#[test]
fn xml_runtime_world_conditions_overlap_and_fractional_size() {
    let (mut w, _, id) = fixture(
        r#"<Behavior effect="Invulnerable">ConditionEffect</Behavior><Behavior rate="0.2" minSize="0" maxSize="200">ChangeSize</Behavior><State id="a"><Behavior effect="Invulnerable">ConditionEffect</Behavior><Transition afterTime="0.1">b</Transition></State><State id="b"/>"#,
    );
    frame(&mut w, 0);
    frame(&mut w, 50);
    frame(&mut w, 100);
    assert_ne!(
        w.entities[&id].condition & crate::condition::bit(crate::condition::INVULNERABLE),
        0
    );
    assert!((w.xml.sizes[&id] - 100.1).abs() < 0.00001);
}
#[test]
fn xml_runtime_world_projectiles_and_stun_stasis_dormancy() {
    let (mut w, p, id) = fixture(
        r#"<Behavior speed="2" angle="0" distance="10" cooldown="2">Move</Behavior><Behavior type="auto" projectileId="0" cooldown="0.1" numShots="1">Shoot</Behavior>"#,
    );
    frame(&mut w, 0);
    assert_eq!(w.bullets.len(), 1);
    frame(&mut w, 50);
    assert!((w.entities[&id].pos.x - 64.6).abs() < 0.0001);
    w.apply_condition(id, crate::condition::STUNNED, 200)
        .unwrap();
    frame(&mut w, 100);
    assert_eq!(w.bullets.len(), 1);
    assert!(w.entities[&id].pos.x > 64.6);
    let pos = w.entities[&id].pos;
    w.apply_condition(id, crate::condition::STASIS, 100)
        .unwrap();
    frame(&mut w, 150);
    assert_eq!(w.entities[&id].pos.x, pos.x);
    w.players.get_mut(&p).unwrap().pos = Pos { x: 1.5, y: 1.5 };
    frame(&mut w, 250);
    assert_eq!(w.entities[&id].pos.x, pos.x);
}
#[test]
fn xml_runtime_world_activation_is_first_non_dormant_tick() {
    let (mut w, p, id) =
        fixture(r#"<Behavior speed="2" angle="0" distance="10" cooldown="2">Move</Behavior>"#);
    w.players.get_mut(&p).unwrap().pos = Pos { x: 1.5, y: 1.5 };
    frame(&mut w, 0);
    assert_eq!(w.entities[&id].activated, None);
    w.players.get_mut(&p).unwrap().pos = Pos { x: 65.5, y: 64.5 };
    frame(&mut w, 100);
    frame(&mut w, 150);
    assert_eq!(w.entities[&id].activated, Some(100));
}
#[test]
fn xml_runtime_world_lifecycle_expires_dormant_and_stasis() {
    let (mut w, p, id) = fixture(r#"<Behavior time="0.1">Decay</Behavior>"#);
    w.players.get_mut(&p).unwrap().paused = true;
    w.apply_condition(id, crate::condition::STASIS, 1000)
        .unwrap();
    frame(&mut w, 0);
    frame(&mut w, 99);
    assert!(w.entities.contains_key(&id));
    frame(&mut w, 100);
    assert!(!w.entities.contains_key(&id));
    assert!(w.xml.rewards.is_empty());
}
#[test]
fn xml_runtime_world_chat_owning_world_and_expiry() {
    let (mut w, p, id) = fixture(
        r#"<Behavior>RemainVisible</Behavior><State id="a"><Transition playerSays="hello">b</Transition></State><State id="b"/>"#,
    );
    frame(&mut w, 0);
    w.xml_chat(p, "HELLO").unwrap();
    assert!(w.player_says(scope(&w, id), "hello").unwrap());
    frame(&mut w, 50);
    assert_eq!(
        w.xml.actors[&id].definition().states[w.xml.actors[&id].path()[1] as usize].name,
        "b"
    );
    w.now = 101;
    assert!(!w.player_says(scope(&w, id), "hello").unwrap());
    let (other, _, _) = fixture("<Behavior>RemainVisible</Behavior>");
    assert!(other.player_says(scope(&w, id), "hello").is_err());
}
#[test]
fn xml_runtime_world_target_ties_filters_and_scope() {
    let (mut w, p, id) = fixture("<Behavior>RemainVisible</Behavior>");
    let mut ch = Character::fresh(w.content.get(0x307).unwrap()).unwrap();
    ch.name = "Bea".into();
    ch.id = 2;
    ch.account = 2;
    let q = w.enter(ch, None).unwrap();
    w.players.get_mut(&q).unwrap().pos = Pos { x: 63.5, y: 64.5 };
    assert_eq!(
        w.xml_targets(scope(&w, id), 1., false)
            .unwrap()
            .iter()
            .map(|(id, _)| *id)
            .collect::<Vec<_>>(),
        vec![p, q]
    );
    w.apply_condition(p, crate::condition::INVISIBLE, 1000)
        .unwrap();
    assert_eq!(w.xml_targets(scope(&w, id), 1., false).unwrap().len(), 1);
}
#[test]
fn xml_runtime_world_terminal_actual_killer_and_pending_rewards() {
    let (mut w, p, id) = fixture(
        "<Behavior>OryxTaunt</Behavior><Behavior childId=\"Piratess\" prob=\"1\" posJitter=\"0\">MetamorphoseAtDeath</Behavior>",
    );
    frame(&mut w, 0);
    assert_eq!(w.xml.admissions.len(), 1);
    let hp = w.entities[&id].hp;
    assert_eq!(w.damage_enemy(p, id, 100000, true, &[], None).unwrap(), hp);
    if let Err(error) = drain_events(&mut w) {
        eprintln!("journal drain retained event: {error:#}");
    }
    assert!(!w.entities.contains_key(&id));
    let intent = &w.xml.rewards[&id];
    assert_eq!(intent.entity.damage.get(&1), Some(&hp));
    assert_eq!(
        intent.plan.announcements[0].text,
        "Ada defeated the Pirate."
    );
    assert_eq!(intent.replacements.len(), 1);
    let replacement = intent.replacements[0];
    assert_eq!(w.entities[&replacement].damage[&1], hp);
    assert_eq!(w.xml.identities[&replacement].parent, None);
    assert_eq!(w.xml.rewards.len(), 1);
}
#[test]
fn xml_runtime_world_stamp_transparency_clear_bounds_and_atomicity() {
    let (mut w, _, id) = fixture(r#"<Behavior map="Stamp">PlaceMap</Behavior>"#);
    let b = w.xml_game().definitions[&0x600].states[0].behaviors[0].clone();
    let st = step(&w, id, b.local);
    let left = spawn(&mut w, 0x120, 64.5, 65.5);
    let transparent = spawn(&mut w, 0x601, 65.5, 64.5);
    let mut rng = rand::rngs::StdRng::seed_from_u64(1);
    w.apply(Service::Map, &b, st, &mut rng).unwrap();
    assert!(!w.entities.contains_key(&left));
    assert!(w.entities.contains_key(&transparent));
    assert_eq!(w.entities_at(Pos { x: 65.5, y: 65.5 }).count(), 1);
    let before = w.entities.len();
    assert!(w.xml_prepare_stamp(0, Pos { x: 127.5, y: 127.5 }).is_err());
    assert_eq!(w.entities.len(), before);
}
#[test]
fn xml_runtime_world_stamp_over_a_reserved_interaction_waits_instead_of_failing() {
    let (mut w, _, id) = fixture(r#"<Behavior map="Stamp" cooldown="0">PlaceMap</Behavior>"#);
    let b = w.xml_game().definitions[&0x600].states[0].behaviors[0].clone();
    let held = spawn(&mut w, 0x120, 64.5, 65.5);
    w.entities.get_mut(&held).unwrap().reserved = true;
    assert!(!w.ready(Service::Map, &b, step(&w, id, b.local)).unwrap());
    frame(&mut w, 0);
    assert!(w.entities.contains_key(&held));
    assert!(!w.entities.values().any(|e| e.kind == 0x601));
    w.entities.get_mut(&held).unwrap().reserved = false;
    frame(&mut w, 50);
    assert!(!w.entities.contains_key(&held));
    assert_eq!(w.entities.values().filter(|e| e.kind == 0x601).count(), 1);
}
#[test]
fn xml_runtime_world_blocked_start_repeats_without_mutation_then_unblocks() {
    use rand::RngCore;
    let (mut w, _, id) = fixture(r#"<Behavior speed="20">Wander</Behavior>"#);
    let b = w.xml_game().definitions[&0x600].states[0].behaviors[0].clone();
    let st = step(&w, id, b.local);
    for y in 63..=65 {
        for x in 63..=65 {
            if (x, y) != (64, 64) {
                Arc::make_mut(&mut w.map).cells[y * 128 + x].ground = 255;
            }
        }
    }
    let mut rng = rand::rngs::StdRng::seed_from_u64(333);
    let expected = rng.clone().next_u64();
    for _ in 0..4 {
        assert_eq!(
            w.apply(Service::Movement, &b, st, &mut rng).unwrap(),
            ActionOutcome::Blocked
        );
        assert_eq!(rng.clone().next_u64(), expected);
        assert!(w.xml.motions.is_empty());
        assert_eq!(w.entities[&id].pos.x, 64.5);
        assert_eq!(w.entities[&id].pos.y, 64.5);
        assert!(w.bullets.is_empty() && w.xml.conditions.is_empty() && w.xml.claims.is_empty());
        assert_eq!(w.entities.len(), 1);
    }
    for y in 63..=65 {
        for x in 63..=65 {
            Arc::make_mut(&mut w.map).cells[y * 128 + x].ground = 0x18;
        }
    }
    assert_eq!(
        w.apply(Service::Movement, &b, st, &mut rng).unwrap(),
        ActionOutcome::Applied
    );
    assert!(w.entities[&id].pos.distance(Pos { x: 64.5, y: 64.5 }) > 0.9);
}
#[test]
fn xml_runtime_world_blocked_bucket_allows_later_real_movement() {
    let (mut w, _, id) = fixture(
        r#"<Behavior bucket="movement" angle="0" speed="2" distance="4">Move</Behavior><Behavior bucket="movement" angle="180" speed="2" distance="4">Move</Behavior>"#,
    );
    w.position_entity(id, Pos { x: 64.95, y: 64.5 }).unwrap();
    Arc::make_mut(&mut w.map).cells[64 * 128 + 65].ground = 255;
    frame(&mut w, 0);
    let locals: Vec<_> = w.xml.motions.keys().map(|t| t.local).collect();
    let bs = &w.xml_game().definitions[&0x600].states[0].behaviors;
    assert_eq!(locals, vec![bs[1].local]);
    frame(&mut w, 50);
    assert!((w.entities[&id].pos.x - 64.85).abs() < 0.0001);
    assert!(w.xml.actors.contains_key(&id));
}

fn output(w: &mut World, p: i32) -> tokio::sync::mpsc::Receiver<Packet> {
    let (tx, rx) = tokio::sync::mpsc::channel(256);
    let (lost, _) = tokio::sync::watch::channel(None);
    w.players.get_mut(&p).unwrap().tx = Some(Output {
        trace: None,
        packets: tx,
        lost,
    });
    rx
}
fn texts(rx: &mut tokio::sync::mpsc::Receiver<Packet>) -> Vec<String> {
    let mut out = vec![];
    while let Ok(p) = rx.try_recv() {
        if p.id == wire::TEXT {
            let mut r = wire::Reader::new(&p.body);
            r.utf().unwrap();
            r.i32().unwrap();
            r.i32().unwrap();
            r.u8().unwrap();
            r.utf().unwrap();
            out.push(r.utf().unwrap());
        }
    }
    out
}
#[test]
fn red_demon_taunt_without_eligible_player_preserves_actor_and_rewards() {
    let content = Arc::new(crate::world::test_content());
    let mut w = World::in_domain(content, Arc::new(flat_map(128)), WorldDomain::Realm).unwrap();
    let mut ch = Character::fresh(w.content.get(0x307).unwrap()).unwrap();
    (ch.name, ch.account, ch.id) = ("Ada".into(), 1, 1);
    let p = w.enter(ch, None).unwrap();
    let mut rx = output(&mut w, p);
    let pos = Pos { x: 64.5, y: 64.5 };
    w.players.get_mut(&p).unwrap().pos = pos;
    let id = w.spawn_entity(0x668, pos, vec![]).unwrap();
    let hp = w.entities[&id].hp;
    assert_eq!(w.damage_enemy(p, id, 1, true, &[], None).unwrap(), 1);
    let ledger = w.entities[&id].damage.clone();
    w.players.get_mut(&p).unwrap().paused = true;
    w.updates(p, true).unwrap();
    let mut actor = w.xml_actor(id).unwrap();
    // Drive active executor frames even though the normal interest scheduler would let it sleep.
    for now in [0, 20_000, 40_000, 60_000, 80_000, 100_000, 120_000, 140_000] {
        w.now = now;
        actor
            .tick(
                Frame {
                    delta_ms: 20_000,
                    dormant: false,
                    stasis: false,
                    stunned: false,
                },
                &mut w,
            )
            .unwrap();
        assert!(!actor.retired());
    }
    assert!(texts(&mut rx).is_empty());
    assert_eq!(w.entities[&id].hp, hp - 1);
    assert_eq!(w.entities[&id].damage, ledger);
    assert!(!w.xml.rewards.contains_key(&id));
    w.xml.actors.insert(id, actor);
    let ch = &mut w.players.get_mut(&p).unwrap().character;
    let (xp, kills) = (ch.exp, ch.kills);
    w.players.get_mut(&p).unwrap().paused = false;
    assert_eq!(w.damage_enemy(p, id, hp, true, &[], None).unwrap(), hp - 1);
    drain_events(&mut w).unwrap();
    assert!(!w.entities.contains_key(&id));
    let ch = &w.players[&p].character;
    assert!(ch.exp > xp);
    assert_eq!(ch.kills, kills + 1);
    assert!(w.xml.rewards.contains_key(&id));
}

#[test]
fn xml_runtime_world_announcements_retry_false_after_claim_is_once_and_world_local() {
    for fault in [Fault::AdmissionBefore, Fault::AdmissionAfter] {
        let (mut w, p, id) = fixture("<Behavior>OryxTaunt</Behavior>");
        let mut rx = output(&mut w, p);
        let (mut other, q, _) = fixture("<Behavior>RemainVisible</Behavior>");
        let mut unrelated = output(&mut other, q);
        w.players.get_mut(&p).unwrap().paused = true;
        w.xml.fault = Some(fault);
        w.xml_step().unwrap();
        if let Err(error) = drain_events(&mut w) {
            eprintln!("journal drain retained event: {error:#}");
        }
        assert!(w.xml.retry_admissions.contains(&id));
        let mut observed = texts(&mut rx);
        w.now = 50;
        w.xml_step().unwrap();
        if let Err(error) = drain_events(&mut w) {
            eprintln!("journal drain retained event: {error:#}");
        }
        observed.extend(texts(&mut rx));
        assert_eq!(observed, vec!["Welcome."]);
        assert!(texts(&mut unrelated).is_empty());
        assert_eq!(w.xml.admissions.len(), 1);
        assert!(w.xml.retry_admissions.is_empty());
    }
}
#[test]
fn xml_runtime_world_terminal_boost_captured_before_an_earlier_reward_reaches_level_20() {
    let (mut w, p, first) = fixture("<Behavior>RemainVisible</Behavior>");
    let second = spawn(&mut w, 0x600, 63.5, 64.5);
    let ch = &mut w.players.get_mut(&p).unwrap().character;
    ch.level = 19;
    ch.exp = crate::store::level_start(20).unwrap() - 1;
    ch.xp_boost.active = true;
    frame(&mut w, 0);
    for id in [first, second] {
        let hp = w.entities[&id].hp;
        assert_eq!(w.damage_enemy(p, id, 100000, true, &[], None).unwrap(), hp);
    }
    for at in [50, 100, 150] {
        frame(&mut w, at);
    }
    assert!(w.xml.retry_terminals.is_empty());
    assert!(w.xml.rewards.contains_key(&first) && w.xml.rewards.contains_key(&second));
    let ch = &w.players[&p].character;
    assert_eq!((ch.level, ch.xp_boost.active, ch.kills), (20, false, 2));
}
#[test]
fn xml_runtime_world_metamorphose_jitter_stays_in_the_parent_tile_beside_walls() {
    let (mut w, p, first) = fixture(
        r#"<Behavior>RemainVisible</Behavior><Behavior childId="Piratess" posJitter="0.5">MetamorphoseAtDeath</Behavior>"#,
    );
    w.players.get_mut(&p).unwrap().pos = Pos { x: 60.5, y: 64.5 };
    for i in (0..9).filter(|i| *i != 4) {
        let (x, y) = (63.5 + (i % 3) as f32, 63.5 + (i / 3) as f32);
        w.spawn_entity(0x120, Pos { x, y }, vec![]).unwrap();
    }
    w.remove_entity(first);
    let corner = Pos { x: 64.95, y: 64.95 };
    let parents: Vec<_> = (0..6)
        .map(|_| w.spawn_entity(0x600, corner, vec![]).unwrap())
        .collect();
    frame(&mut w, 0);
    for id in &parents {
        let hp = w.entities[id].hp;
        assert_eq!(w.damage_enemy(p, *id, 100000, true, &[], None).unwrap(), hp);
    }
    for at in 1..=10 {
        frame(&mut w, at * 50);
    }
    assert!(w.xml.retry_terminals.is_empty());
    let children: Vec<_> = w.entities.values().filter(|e| e.kind == 0x601).collect();
    assert_eq!(children.len(), parents.len());
    assert!(children.iter().all(|e| e.pos.x < 65. && e.pos.y < 65.));
}
#[test]
fn xml_runtime_world_pause_settlement_retries_a_captured_terminal_without_ticks() {
    let (mut w, p, id) = fixture("<Behavior>RemainVisible</Behavior>");
    frame(&mut w, 0);
    w.players.get_mut(&p).unwrap().pending = true;
    let hp = w.entities[&id].hp;
    assert_eq!(w.damage_enemy(p, id, 100000, true, &[], None).unwrap(), hp);
    assert!(w.xml.facts.contains_key(&id) && w.xml.retry_terminals.contains(&id));
    // The recipient's earlier progress commits while the native clock is paused; no World tick follows.
    w.players.get_mut(&p).unwrap().pending = false;
    drain_events(&mut w).unwrap();
    assert!(!w.unsettled().unwrap().is_empty());
    w.xml_settle_captured_terminals().unwrap();
    drain_events(&mut w).unwrap();
    assert!(w.unsettled().unwrap().is_empty(), "{:?}", w.unsettled());
    assert!(w.xml.rewards.contains_key(&id) && !w.entities.contains_key(&id));
}
#[test]
fn xml_runtime_world_death_births_ignore_player_blocking_scenery() {
    let (mut w, p, id) = fixture(
        r#"<Behavior min="2" max="2" childId="Piratess" range="0.4">ReleaseSpawnAtDeath</Behavior>"#,
    );
    // Tree A (OccupySquare only) blocks players and bags, not enemies (Large Ghost Imps, actors.xml:4073).
    spawn(&mut w, 0x100, 64.5, 64.5);
    frame(&mut w, 0);
    let hp = w.entities[&id].hp;
    assert_eq!(w.damage_enemy(p, id, 100000, true, &[], None).unwrap(), hp);
    frame(&mut w, 50);
    assert!(w.xml.retry_terminals.is_empty() && w.xml.rewards.contains_key(&id));
    assert_eq!(w.entities.values().filter(|e| e.kind == 0x601).count(), 2);
}
#[test]
fn xml_runtime_world_release_spawn_jitter_into_walls_or_void_births_at_the_host() {
    // Large Ghost Imps (actors.xml:4093) beside a wall; Medium/Small Ghost births beside void (skull-shrine take 4d48bcd6).
    for void in [false, true] {
        let (mut w, p, first) = fixture(
            r#"<Behavior min="3" max="3" childId="Piratess">ReleaseSpawnAtDeath</Behavior>"#,
        );
        w.players.get_mut(&p).unwrap().pos = Pos { x: 60.5, y: 64.5 };
        let ground = *w.content.void_grounds.iter().next().unwrap();
        for i in (0..9).filter(|i| *i != 4) {
            let (x, y) = (63 + i % 3, 63 + i / 3);
            if void {
                Arc::make_mut(&mut w.map).cells[y * 128 + x].ground = ground;
            } else {
                let pos = Pos {
                    x: x as f32 + 0.5,
                    y: y as f32 + 0.5,
                };
                w.spawn_entity(0x120, pos, vec![]).unwrap();
            }
        }
        w.remove_entity(first);
        let corner = Pos { x: 64.95, y: 64.95 };
        let parents: Vec<_> = (0..6)
            .map(|_| w.spawn_entity(0x600, corner, vec![]).unwrap())
            .collect();
        frame(&mut w, 0);
        for id in &parents {
            let hp = w.entities[id].hp;
            assert_eq!(w.damage_enemy(p, *id, 100000, true, &[], None).unwrap(), hp);
        }
        for at in 1..=10 {
            frame(&mut w, at * 50);
        }
        assert!(w.xml.retry_terminals.is_empty() && w.xml.abandon_physical.is_empty());
        let children: Vec<_> = w.entities.values().filter(|e| e.kind == 0x601).collect();
        assert_eq!(children.len(), 3 * parents.len(), "void={void}");
        assert!(children.iter().all(|e| e.pos.x < 65. && e.pos.y < 65.));
        assert!(
            children.iter().any(|e| e.pos.x != corner.x),
            "unblocked jitter keeps its offset"
        );
    }
}
#[test]
fn xml_runtime_world_terminal_retry_same_plan_no_duplicate_birth_message_or_ledger() {
    for fault in [Fault::TerminalBefore, Fault::TerminalAfter] {
        let (mut w, p, id) = fixture(
            r#"<Behavior>OryxTaunt</Behavior><Behavior childId="Piratess" posJitter="0">MetamorphoseAtDeath</Behavior>"#,
        );
        let mut rx = output(&mut w, p);
        frame(&mut w, 0);
        texts(&mut rx);
        let hp = w.entities[&id].hp;
        w.xml.fault = Some(fault);
        assert_eq!(w.damage_enemy(p, id, 999999, true, &[], None).unwrap(), hp);
        if let Err(error) = drain_events(&mut w) {
            eprintln!("journal drain retained event: {error:#}");
        }
        assert!(w.xml.retry_terminals.contains(&id));
        let mut observed = texts(&mut rx);
        w.now = 50;
        w.xml_step().unwrap();
        if let Err(error) = drain_events(&mut w) {
            eprintln!("journal drain retained event: {error:#}");
        }
        observed.extend(texts(&mut rx));
        assert_eq!(observed, vec!["Ada defeated the Pirate."]);
        let plan = w.xml.rewards[&id].plan.clone();
        let child = w.xml.rewards[&id].replacements[0];
        assert_eq!(w.entities[&child].damage[&1], hp);
        assert_eq!(w.entities.values().filter(|e| e.kind == 0x601).count(), 1);
        assert!(!w.entities.contains_key(&id));
        w.terminal(plan.origin, &plan).unwrap();
        w.xml_defeated(id, TerminalContext::NonPlayer).unwrap();
        if let Err(error) = drain_events(&mut w) {
            eprintln!("journal drain retained event: {error:#}");
        }
        assert_eq!(w.entities.values().filter(|e| e.kind == 0x601).count(), 1);
        assert!(texts(&mut rx).is_empty());
        assert!(w.xml.retry_terminals.is_empty());
    }
}
#[test]
fn xml_runtime_world_nonplayer_birthonly_and_state_membership() {
    let (mut w, p, id) = fixture(
        r#"<State id="active"><Behavior>OryxTaunt</Behavior><Transition afterTime="0.05">silent</Transition></State><State id="silent"/>"#,
    );
    let mut rx = output(&mut w, p);
    frame(&mut w, 0);
    assert_eq!(texts(&mut rx), vec!["Welcome."]);
    frame(&mut w, 50);
    w.xml_defeated(id, TerminalContext::NonPlayer).unwrap();
    if let Err(error) = drain_events(&mut w) {
        eprintln!("journal drain retained event: {error:#}");
    }
    assert!(texts(&mut rx).is_empty());
    let only = r#"<StringLists><StringList id="Pirate"><Strings id="new"><String>Birth only.</String></Strings></StringList></StringLists>"#;
    let (mut w, p, id) = fixture_program(
        "<Behavior>OryxTaunt</Behavior>",
        "<Behavior>RemainVisible</Behavior>",
        only,
    );
    let mut rx = output(&mut w, p);
    frame(&mut w, 0);
    assert_eq!(texts(&mut rx), vec!["Birth only."]);
    w.xml_defeated(id, TerminalContext::NonPlayer).unwrap();
    if let Err(error) = drain_events(&mut w) {
        eprintln!("journal drain retained event: {error:#}");
    }
    assert!(texts(&mut rx).is_empty());
    let (mut w, p, id) = fixture("<Behavior>OryxTaunt</Behavior>");
    let mut rx = output(&mut w, p);
    frame(&mut w, 0);
    texts(&mut rx);
    w.xml_defeated(id, TerminalContext::NonPlayer).unwrap();
    if let Err(error) = drain_events(&mut w) {
        eprintln!("journal drain retained event: {error:#}");
    }
    assert_eq!(texts(&mut rx), vec!["The Pirate is gone."]);
}
#[test]
fn xml_runtime_world_ordered_lifecycle_and_independent_condition_ownership() {
    let root = r#"<Behavior>RemainVisible</Behavior><Order targetIds="Piratess"><Behavior>OryxTaunt</Behavior><Behavior effect="Invulnerable">ConditionEffect</Behavior></Order>"#;
    let child = r#"<Behavior listenIds="Pirate">FollowOrders</Behavior><Behavior listenIds="Pirate">FollowOrders</Behavior>"#;
    let (mut w, p, issuer) = fixture_program(root, child, LISTS);
    let listener = spawn(&mut w, 0x601, 63.5, 64.5);
    let mut rx = output(&mut w, p);
    frame(&mut w, 0);
    assert_eq!(texts(&mut rx), vec!["Welcome.", "Welcome."]);
    assert_eq!(
        w.xml
            .conditions
            .keys()
            .filter(|t| t.scope.identity.actor == listener)
            .count(),
        2
    );
    w.remove_entity(issuer);
    frame(&mut w, 50);
    assert_eq!(
        w.entities[&listener].condition & crate::condition::bit(crate::condition::INVULNERABLE),
        0
    );
    assert!(w.xml.orders.is_empty());
}
#[test]
fn xml_runtime_world_actual_projectile_origin_survives_owner_departure_and_rejects_missing() {
    let (mut w, p, id) = fixture("<Behavior>OryxTaunt</Behavior>");
    frame(&mut w, 0);
    let origin = w.damage_origin(p).unwrap();
    let hp = w.entities[&id].hp;
    w.players.remove(&p);
    assert!(w.damage_enemy(999, id, 1, true, &[], None).is_err());
    if let Err(error) = drain_events(&mut w) {
        eprintln!("journal drain retained event: {error:#}");
    }
    assert_eq!(w.entities[&id].hp, hp);
    w.damage_enemy_from(&origin, id, 999999, true, &[], Some(1))
        .unwrap();
    if let Err(error) = drain_events(&mut w) {
        eprintln!("journal drain retained event: {error:#}");
    }
    assert_eq!(
        w.xml.rewards[&id].plan.announcements[0].text,
        "Ada defeated the Pirate."
    );
    assert_eq!(w.xml.rewards[&id].entity.damage[&1], hp);
}

#[test]
fn xml_runtime_world_children_bounds_release_group_and_parent_encounter_distinction() {
    let (mut w, _, id) = fixture(
        r#"<Behavior childIds="Piratess" maxChildren="2" cooldown="0.1">MakeMinions</Behavior>"#,
    );
    frame(&mut w, 0);
    let children: Vec<_> = w
        .entities
        .values()
        .filter(|e| e.parent == Some(id))
        .map(|e| e.id)
        .collect();
    assert_eq!(children.len(), 2);
    for child in &children {
        assert_eq!(
            w.xml_identity(*child).unwrap().encounter,
            w.xml_identity(id).unwrap().encounter
        );
    }
    w.xml_defeated(children[0], TerminalContext::NonPlayer)
        .unwrap();
    if let Err(error) = drain_events(&mut w) {
        eprintln!("journal drain retained event: {error:#}");
    }
    frame(&mut w, 99);
    assert_eq!(
        w.entities.values().filter(|e| e.parent == Some(id)).count(),
        1
    );
    frame(&mut w, 100);
    assert_eq!(
        w.entities.values().filter(|e| e.parent == Some(id)).count(),
        2
    );
    frame(&mut w, 500);
    assert_eq!(
        w.entities.values().filter(|e| e.parent == Some(id)).count(),
        2
    );
    let root2 = spawn(&mut w, 0x600, 64.5, 65.5);
    let b = Behavior {
        kind: K::Protect,
        fields: BTreeMap::from([
            (F::ProtecteeId, Value::Objects(vec![0x600])),
            (F::AcquireRange, Value::Number(20.)),
        ]),
        ..w.xml_game().definitions[&0x600].states[0].behaviors[0].clone()
    };
    let child = *w
        .entities
        .values()
        .find(|e| e.parent == Some(id))
        .map(|e| &e.id)
        .unwrap();
    assert_eq!(w.xml_anchor(&b, scope(&w, child)).unwrap(), Some(id));
    w.remove_entity(id);
    assert_eq!(w.xml_anchor(&b, scope(&w, child)).unwrap(), None);
    assert!(w.exists(scope(&w, child), &[0x600]).unwrap());
    assert!(w.entities.contains_key(&root2));
}
#[test]
fn xml_runtime_world_object_toss_is_world_clock_reserved_birth_not_immediate() {
    let (mut w, p, id) = fixture(
        r#"<Behavior childId="Piratess" type="auto" defaultAngle="0" range="3" rangeJitter="0" centerObject="true" cooldown="10">ObjectToss</Behavior>"#,
    );
    frame(&mut w, 0);
    assert_eq!(w.entities.len(), 1);
    assert_eq!(w.xml.births.len(), 1);
    let birth = w.xml.births[0].id;
    w.players.get_mut(&p).unwrap().paused = true;
    frame(&mut w, 1499);
    assert!(!w.entities.contains_key(&birth));
    frame(&mut w, 1500);
    assert_eq!(w.entities[&birth].pos.x, 67.5);
    assert_eq!(w.entities[&birth].parent, Some(id));
    assert_eq!(
        w.xml_identity(birth).unwrap().encounter,
        w.xml_identity(id).unwrap().encounter
    );
}
#[test]
fn xml_runtime_world_object_toss_past_the_map_edge_is_blocked_not_fatal() {
    let (mut w, _, _) = fixture(
        r#"<Behavior childId="Piratess" type="auto" defaultAngle="0" range="100" cooldown="10">ObjectToss</Behavior>"#,
    );
    frame(&mut w, 0);
    frame(&mut w, 1500);
    assert!(w.xml.births.is_empty());
    assert_eq!(w.entities.len(), 1);
}
/// Production Pentaract revealed through the actual Realm event director, with an invincible
/// player beyond the host's 20-tile dormancy radius but beside slot 0.
fn revealed_pentaract() -> (Directory, i32, i32) {
    let content = Arc::new(crate::world::test_content());
    let mut d = Directory::test_with_seed(Path::new("data"), content.clone(), 85, 1344).unwrap();
    let w = d.worlds.get_mut(&-3).unwrap();
    assert!(w.entities.values().all(|e| e.kind != 0xd5e));
    let source = w.actors().find(|e| *e.quest == Some(e.id)).unwrap().id;
    (w.event_waiting, w.event_choice) = (Some(source), Some(0xd5f));
    let realm_events::EventAdmission::Spawned(host) = w.earn_realm_event(source).unwrap() else {
        panic!("Pentaract reveal refused")
    };
    let mut ch = Character::fresh(content.get(crate::content::WIZARD).unwrap()).unwrap();
    (ch.name, ch.id, ch.account, ch.level) = ("Razer".into(), 1, 1, 20);
    let p = w.enter(ch, None).unwrap();
    let at = w.entities[&host].pos;
    let player = w.players.get_mut(&p).unwrap();
    player.lab_conditions = crate::condition::bit(crate::condition::INVINCIBLE);
    player.pos = Pos {
        x: at.x + 27.,
        y: at.y,
    };
    // Publish the prepared arrival before staging rewards; later effects can otherwise reveal tiles mid-receipt.
    w.updates(p, false).unwrap();
    (d, p, host)
}
fn towers(w: &World, host: i32) -> Vec<i32> {
    let mut ids: Vec<_> = w
        .actors()
        .filter(|e| e.parent == Some(host) && e.kind == 0xd5e && !e.reserved)
        .map(|e| e.id)
        .collect();
    ids.sort_by(|a, b| {
        let angle = |id: &i32| {
            let (e, h) = (w.entities[id].pos, w.entities[&host].pos);
            (f64::from(e.y - h.y))
                .atan2(f64::from(e.x - h.x))
                .rem_euclid(std::f64::consts::TAU)
        };
        angle(a).total_cmp(&angle(b))
    });
    ids
}
fn kill_tower(w: &mut World, p: i32, tower: i32, damage: i32) {
    let e = w.entities.get_mut(&tower).unwrap();
    (e.hp, e.damage) = (1, BTreeMap::from([(1, damage - 1)]));
    w.damage_enemy(p, tower, 1000, true, &[], None).unwrap();
}
fn capture(w: &mut World, at: u64) {
    w.now = at;
    w.xml_step().unwrap();
    drain_events(w).unwrap();
}
/// Realm-wide Decay waves (e.g. 150 Golem Satellites at 16 s) queue ahead of the corpses at 8 pending terminals.
fn settle(w: &mut World, now: &mut u64, host: i32) {
    for _ in 0..1000 {
        if !w.entities.contains_key(&host) && w.entities.values().all(|e| e.kind != 0xd60) {
            return;
        }
        *now += 50;
        capture(w, *now);
    }
    panic!("Pentaract completion did not settle");
}
fn journaled(w: &World) -> Vec<crate::store::world_events::TerminalPayload> {
    let path = std::path::PathBuf::from(std::env::var("QA_RESULTS_DIR").unwrap())
        .join(format!("world-terminal-{}.db", w.xml.instance));
    let db = rusqlite::Connection::open(path).unwrap();
    let mut rows = db
        .prepare("SELECT payload FROM world_events WHERE kind='terminal'")
        .unwrap();
    rows.query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .map(|payload| {
            let payload: serde_json::Value = serde_json::from_str(&payload.unwrap()).unwrap();
            serde_json::from_value(payload["payload"]["data"].clone()).unwrap()
        })
        .collect()
}
#[test]
fn xml_runtime_pentaract_tower_contributors_qualify_for_the_never_damaged_root_quest() {
    let (mut d, p, host) = revealed_pentaract();
    let w = d.worlds.get_mut(&-3).unwrap();
    let mut now = w.now;
    capture(w, now);
    let at = w.entities[&host].pos;
    let player = w.players.get_mut(&p).unwrap();
    (player.quest, player.pos) = (
        Some(host),
        Pos {
            x: at.x + 22.,
            y: at.y,
        },
    );
    for id in towers(w, host) {
        kill_tower(w, p, id, 1000);
    }
    settle(w, &mut now, host);
    let root = journaled(w)
        .into_iter()
        .find(|t| t.defeat.kind == 0xd5f)
        .unwrap();
    assert_eq!(
        root.ledger[0].effective_damage, 5000,
        "every tower ledger joins the root"
    );
    assert_eq!(w.players[&p].character.lifetime.qualified_quests, 1);
}
#[test]
fn xml_runtime_pentaract_completes_once_with_each_corpse_carrying_its_tower_ledger() {
    let (mut d, p, host) = revealed_pentaract();
    let w = d.worlds.get_mut(&-3).unwrap();
    let mut now = w.now;
    capture(w, now);
    // behavior.cc builds the protectors in the constructor: no player is within 20 tiles of the host.
    let slots = towers(w, host);
    assert_eq!(slots.len(), 5);
    assert_eq!(w.entities.values().filter(|e| e.kind == 0xd5e).count(), 5);
    let center = w.entities[&host].pos;
    for (slot, id) in slots.iter().enumerate() {
        let angle = std::f64::consts::TAU * slot as f64 / 5.;
        let e = w.entities[id].pos;
        assert!((f64::from(e.x - center.x) - 15. * angle.cos()).abs() < 1e-3);
        assert!((f64::from(e.y - center.y) - 15. * angle.sin()).abs() < 1e-3);
    }
    let damage = [100, 600, 1000, 300, 5000];
    for (slot, id) in slots[..4].iter().enumerate() {
        kill_tower(w, p, *id, damage[slot]);
        now += 3000;
        capture(w, now);
    }
    kill_tower(w, p, slots[4], damage[4]);
    // Undrained retries while the host's terminal awaits its durable receipt must not repeat completion.
    for _ in 0..4 {
        now += 50;
        w.now = now;
        w.xml_step().unwrap();
        assert!(w.entities.contains_key(&host));
        assert_eq!(w.entities.values().filter(|e| e.kind == 0xd60).count(), 5);
    }
    assert_eq!(
        w.pending_terminal_events()
            .iter()
            .filter(|e| e.terminal_payload().unwrap().defeat.kind == 0xd5f)
            .count(),
        1
    );
    settle(w, &mut now, host);
    assert!(w.entities.values().all(|e| e.kind != 0xd5e));
    assert_eq!(w.critical_deaths, vec![host]);
    let payloads = journaled(w);
    let hosts: Vec<_> = payloads.iter().filter(|t| t.defeat.kind == 0xd5f).collect();
    assert_eq!(hosts.len(), 1);
    assert!(hosts[0].allocations().is_empty());
    let rows = &w.xml_game().definitions[&0xd60].loot;
    let mut ledgers: Vec<_> = payloads
        .iter()
        .filter(|t| t.defeat.kind == 0xd60)
        .map(|t| {
            assert_eq!(t.defeat.encounter, hosts[0].defeat.encounter);
            assert_eq!(t.ledger.len(), 1);
            let ledger = t.ledger[0].effective_damage;
            for a in t.allocations() {
                assert_eq!(a.owner(), crate::loot::evaluator::Owner::Character(1));
                assert!(ledger as f64 / 5000. >= rows[a.row()].threshold);
            }
            if ledger == 100 {
                assert!(t.allocations().is_empty(), "below every corpse threshold");
            }
            ledger
        })
        .collect();
    ledgers.sort();
    assert_eq!(ledgers, vec![100, 300, 600, 1000, 5000]);
    for _ in 0..5 {
        now += 1000;
        capture(w, now);
    }
    assert_eq!(
        journaled(w)
            .iter()
            .filter(|t| t.defeat.kind == 0xd5f)
            .count(),
        1
    );
    assert_eq!(w.critical_deaths, vec![host]);
}
#[test]
fn xml_runtime_pentaract_respawns_slow_kills_and_corpses_use_latest_tower_ledgers() {
    let (mut d, p, host) = revealed_pentaract();
    let w = d.worlds.get_mut(&-3).unwrap();
    let mut rx = output(w, p);
    w.players.get_mut(&p).unwrap().known.insert(host);
    let streams = |rx: &mut tokio::sync::mpsc::Receiver<Packet>| {
        drain(rx)
            .into_iter()
            .filter(|packet| packet.id == wire::SHOW_EFFECT && packet.body[0] == 3)
            .map(|packet| u32::from_be_bytes(packet.body[21..25].try_into().unwrap()))
            .collect::<Vec<_>>()
    };
    let mut now = w.now.next_multiple_of(200);
    capture(w, now);
    // P2-C (behavior.cc:1724-1737): live towers stream red while uncontested, then yellow once respawn is armed.
    let calm = streams(&mut rx);
    assert!(
        !calm.is_empty() && calm.iter().all(|c| *c == 0xFF0000),
        "{calm:x?}"
    );
    let first = towers(w, host);
    for id in &first[..4] {
        kill_tower(w, p, *id, 4000);
        now += 1000;
        capture(w, now);
    }
    let armed_colors = streams(&mut rx);
    assert!(armed_colors.contains(&0xFFFF00), "{armed_colors:x?}");
    // Armed on the first observed death: 15 s later the dead slots return, not before.
    let armed = now - 3000;
    capture(w, armed + 14_950);
    assert_eq!(towers(w, host), first[4..]);
    capture(w, armed + 15_000);
    let second = towers(w, host);
    assert_eq!(second.len(), 5);
    assert_eq!(second[4], first[4]);
    assert!(second[..4].iter().all(|id| !first.contains(id)));
    assert!(w.entities.contains_key(&host));
    now = armed + 15_000;
    for (slot, id) in second.iter().enumerate() {
        kill_tower(w, p, *id, 10 + slot as i32);
        now += 1000;
        capture(w, now);
    }
    settle(w, &mut now, host);
    assert_eq!(w.critical_deaths, vec![host]);
    let mut ledgers: Vec<_> = journaled(w)
        .iter()
        .filter(|t| t.defeat.kind == 0xd60)
        .map(|t| t.ledger.iter().map(|c| c.effective_damage).sum::<u64>())
        .collect();
    ledgers.sort();
    // Respawned slots carry only the replacement tower's damage, never the earlier 4000.
    assert_eq!(ledgers, vec![10, 11, 12, 13, 14]);
}
#[test]
fn xml_runtime_world_grenade_source_seconds_conditions_and_zap_poison() {
    let (mut w, p, id) = fixture(
        r#"<Behavior radius="2" damage="10" duration="0.2" effect="Slowed" range="8" cooldown="10">GrenadeToss</Behavior>"#,
    );
    let hp = w.players[&p].character.hp;
    frame(&mut w, 0);
    assert_eq!(w.xml.blasts.len(), 1);
    frame(&mut w, 1499);
    assert_eq!(w.players[&p].character.hp, hp);
    frame(&mut w, 1500);
    assert!(w.players[&p].character.hp < hp);
    assert!(
        w.players[&p]
            .statuses
            .contains_key(&crate::condition::SLOWED)
    );
    assert_eq!(w.players[&p].statuses[&crate::condition::SLOWED].1, 1700);
    let mut b = w.xml_game().definitions[&0x600].states[0].behaviors[0].clone();
    b.kind = K::Zap;
    b.fields = BTreeMap::from([
        (F::Range, Value::Number(8.)),
        (F::Damage, Value::Integer(10)),
        (F::PoisonTotalDamage, Value::Integer(20)),
        (F::PoisonDuration, Value::Millis(200)),
    ]);
    let st = step(&w, id, b.local);
    let hp = w.players[&p].character.hp;
    w.apply(
        Service::Attack,
        &b,
        st,
        &mut rand::rngs::StdRng::seed_from_u64(1),
    )
    .unwrap();
    let after = w.players[&p].character.hp;
    assert!(after < hp);
    w.now = 1600;
    w.xml_combat_deadlines().unwrap();
    assert_eq!(w.players[&p].character.hp, after - 10);
    w.now = 1700;
    w.xml_combat_deadlines().unwrap();
    assert_eq!(w.players[&p].character.hp, after - 20);
    assert!(w.xml.poisons.is_empty());
}
#[test]
fn xml_runtime_world_aura_exit_cleanup_and_heal_targets_breath_fraction() {
    let (mut w, p, id) = fixture(
        r#"<State id="a"><Behavior effect="Speedy" range="3" cooldown="0.2">ConditionEffectAura</Behavior><Transition afterTime="0.1">b</Transition></State><State id="b"/>"#,
    );
    frame(&mut w, 0);
    assert_ne!(
        w.players[&p].conditions(0) & crate::condition::bit(crate::condition::SPEEDY),
        0
    );
    frame(&mut w, 100);
    assert_eq!(
        w.players[&p].conditions(100) & crate::condition::bit(crate::condition::SPEEDY),
        0
    );
    let mut b = w.xml_game().definitions[&0x600].states[1].behaviors[0].clone();
    b.kind = K::HealZap;
    b.fields = BTreeMap::from([
        (F::Range, Value::Number(8.)),
        (F::Healing, Value::Integer(7)),
    ]);
    w.players.get_mut(&p).unwrap().character.hp -= 20;
    let hp = w.players[&p].character.hp;
    w.apply(
        Service::Heal,
        &b,
        step(&w, id, b.local),
        &mut rand::rngs::StdRng::seed_from_u64(1),
    )
    .unwrap();
    assert_eq!(w.players[&p].character.hp, hp + 7);
    b.kind = K::RestoresBreath;
    b.fields = BTreeMap::from([
        (F::Radius, Value::Number(2.)),
        (F::Speed, Value::Number(5.)),
    ]);
    w.players.get_mut(&p).unwrap().oxygen = 0;
    for _ in 0..4 {
        w.apply(
            Service::Breath,
            &b,
            step(&w, id, b.local),
            &mut rand::rngs::StdRng::seed_from_u64(1),
        )
        .unwrap();
    }
    assert_eq!(w.players[&p].oxygen, 1);
}

#[test]
fn xml_runtime_world_all_production_stamps_preserve_selected_layers() {
    use crate::content::game::geometry::Geometry;
    let content = Arc::new(crate::world::test_content());
    let game = content.game.as_ref().unwrap();
    let mut stamps = BTreeMap::new();
    let mut occurrences = 0;
    for d in game.definitions.values() {
        for b in d
            .states
            .iter()
            .flat_map(|s| &s.behaviors)
            .filter(|b| b.kind == K::PlaceMap)
        {
            let Some(Value::Map(id)) = value(b, F::Map) else {
                panic!("linked map required")
            };
            stamps.entry(*id).or_insert((d.kind, b.clone()));
            occurrences += 1;
        }
    }
    assert_eq!(occurrences, 29);
    assert_eq!(stamps.len(), 22);
    let mut portal_count = 0;
    for (map, (kind, b)) in stamps {
        let mut geometry = flat_map(128);
        geometry.elevation = Some(vec![200; 128 * 128].into());
        let mut w =
            World::in_domain(content.clone(), Arc::new(geometry), WorldDomain::Realm).unwrap();
        let id = spawn(&mut w, kind, 64.5, 64.5);
        let st = step(&w, id, b.local);
        let mut rng = rand::rngs::StdRng::seed_from_u64(99);
        let portal = w
            .xml_prepare_stamp(map, w.entities[&id].pos)
            .unwrap()
            .has_portals(&w);
        if portal {
            portal_count += 1;
            assert_eq!(
                w.apply(Service::Map, &b, st, &mut rng).unwrap(),
                ActionOutcome::Pending
            );
            drain_events(&mut w).unwrap();
        }
        assert_eq!(
            w.apply(Service::Map, &b, st, &mut rng).unwrap(),
            ActionOutcome::Applied
        );
        if let Some(controller) = w.entities.values().find(|e| e.kind == 0xe3c).cloned() {
            assert!(!w.content.objects[&controller.kind].flying);
            assert_eq!(
                w.map
                    .cell(controller.pos.x, controller.pos.y)
                    .unwrap()
                    .ground,
                0xbc
            );
            assert!(
                !w.xml_npc_walkable(scope(&w, controller.id), controller.pos)
                    .unwrap()
            );
            assert!(w.content.objects[&0xe3d].flying);
            let child = w.spawn_entity(0xe3d, controller.pos, vec![]).unwrap();
            assert!(
                w.xml_npc_walkable(scope(&w, child), controller.pos)
                    .unwrap()
            );
        }
        let Geometry::Template(t) = &game.maps[map as usize].geometry else {
            panic!("selected PlaceMap must be Template")
        };
        let anchor = t.anchor.unwrap();
        for (i, cell) in t.cells.iter().enumerate() {
            let x = 64 - anchor[0] as i32 + (i as u32 % t.dimensions[0]) as i32;
            let y = 64 - anchor[1] as i32 + (i as u32 / t.dimensions[0]) as i32;
            let host = w.map.cell(x as f32 + 0.5, y as f32 + 0.5).unwrap();
            match cell {
                None => assert_eq!(host.ground, 0x18),
                Some(index) => {
                    let tile = &t.palette[*index as usize].tile;
                    assert_eq!(host.ground, tile.ground.unwrap_or(0x18));
                    if let Some(kind) = tile.object {
                        let e = w
                            .entities_at(Pos {
                                x: x as f32 + 0.5,
                                y: y as f32 + 0.5,
                            })
                            .find(|e| e.kind == kind)
                            .unwrap_or_else(|| {
                                panic!("{} cell{x},{y}", game.maps[map as usize].name)
                            });
                        if !portal {
                            assert_eq!(e.parent, Some(id));
                            assert_eq!(w.xml_identity(e.id).unwrap().encounter, Some(id as u64));
                        }
                    }
                }
            }
        }
        assert!(
            w.entities.contains_key(&id),
            "stamp must not delete its active caster"
        );
    }
    assert_eq!(portal_count, 1);
}

#[test]
fn xml_runtime_world_stamp_blocked_at_map_edge_changes_nothing_and_max_place_is_real() {
    let (mut w, _, id) =
        fixture(r#"<Behavior map="Stamp" maxPlace="1" cooldown="0">PlaceMap</Behavior>"#);
    let source = w.xml_game().definitions[&0x600].states[0].behaviors[0].clone();
    let ground = w.map.cells.iter().map(|c| c.ground).collect::<Vec<_>>();
    let original_ids = w.ids.load(Ordering::Relaxed);
    w.position_entity(id, Pos { x: 127.5, y: 127.5 }).unwrap();
    let Value::Map(map) = source.fields[&F::Map] else {
        panic!("fixture did not link stamp")
    };
    let error = w.xml_prepare_stamp(map, w.entities[&id].pos).unwrap_err();
    assert!(error.is::<maps::StampBlocked>());
    assert!(error.to_string().contains("outside owning map"));
    assert!(
        !w.ready(Service::Map, &source, step(&w, id, source.local))
            .unwrap()
    );
    assert_eq!(
        w.map.cells.iter().map(|c| c.ground).collect::<Vec<_>>(),
        ground
    );
    assert_eq!(w.ids.load(Ordering::Relaxed), original_ids);
    assert_eq!(w.entities.len(), 1);
    w.position_entity(id, Pos { x: 64.5, y: 64.5 }).unwrap();
    frame(&mut w, 0);
    let child = w.entities.values().find(|e| e.kind == 0x601).unwrap().id;
    frame(&mut w, 50);
    assert_eq!(w.entities.values().filter(|e| e.kind == 0x601).count(), 1);
    assert!(w.entities.contains_key(&child));
}
#[test]
fn xml_runtime_world_xml_only_cooldown_state_and_birth_mutations_change_observed_services() {
    fn candidate(cooldown: &str, state: &str, children: usize) -> World {
        let (mut w, _, _) = fixture(&format!(
            r#"<Behavior childIds="Piratess" maxChildren="{children}" cooldown="10">MakeMinions</Behavior><Behavior type="auto" projectileId="0" cooldown="{cooldown}">Shoot</Behavior><State id="shield"><Behavior effect="Invulnerable">ConditionEffect</Behavior><Transition afterTime="{state}">open</Transition></State><State id="open"/>"#
        ));
        for at in [0, 50, 100, 150] {
            frame(&mut w, at);
        }
        w
    }
    let a = candidate("0.1", "0.1", 1);
    let b = candidate("0.2", "0.2", 2);
    assert_eq!(a.bullets.len(), 2);
    assert_eq!(b.bullets.len(), 1);
    assert_eq!(a.entities.values().filter(|e| e.kind == 0x601).count(), 1);
    assert_eq!(b.entities.values().filter(|e| e.kind == 0x601).count(), 2);
    let root = |w: &World| {
        w.entities
            .values()
            .find(|e| e.kind == 0x600)
            .unwrap()
            .condition
            & crate::condition::bit(crate::condition::INVULNERABLE)
    };
    assert_eq!(root(&a), 0);
    assert_ne!(root(&b), 0);
    assert_ne!(a.xml_game().fingerprint, b.xml_game().fingerprint);
}

#[test]
fn xml_runtime_world_selected_flying_and_initial_water_rules_do_not_change_player_collision() {
    let content = Arc::new(crate::world::test_content());
    assert!(content.objects[&0xe37].flying);
    assert!(!content.objects[&0xd61].flying);
    let mut map = flat_map(128);
    for c in &mut map.cells {
        c.ground = 0xbc;
    }
    map.elevation = Some(vec![200; 128 * 128].into());
    let mut w = World::in_domain(content, Arc::new(map), WorldDomain::Realm).unwrap();
    let pos = Pos { x: 64.5, y: 64.5 };
    assert!(!w.walkable(pos));
    assert!(w.xml_placeable(0xe37, pos, &[], None).unwrap());
    assert!(w.xml_placeable(0xd61, pos, &[], None).unwrap());
    assert!(w.xml_placeable(0x601, pos, &[], None).unwrap());
    let grounded = w.spawn_entity(0x601, pos, vec![]).unwrap();
    assert!(
        !w.xml_npc_walkable(scope(&w, grounded), Pos { x: 65.5, y: 64.5 })
            .unwrap()
    );
    let ghost = w.spawn_entity(0xe37, pos, vec![]).unwrap();
    let hermit = spawn(&mut w, 0xd61, 68.5, 64.5);
    frame(&mut w, 0);
    assert!(
        !w.xml_npc_walkable(scope(&w, hermit), Pos { x: 69.5, y: 64.5 })
            .unwrap()
    );
    assert!(
        !w.xml_npc_walkable(scope(&w, hermit), Pos { x: 69.5, y: 64.5 })
            .unwrap()
    );
    let grants: Vec<_> = w.xml_game().definitions[&0xd61]
        .states
        .iter()
        .filter(|s| s.behaviors.iter().any(|b| b.kind == K::StayInTerrain))
        .map(|s| (s.id, s.name.clone()))
        .collect();
    assert_eq!(
        grants
            .iter()
            .map(|(_, s)| s.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["Base", "AttackPools", "Wait"])
    );
    for (state, _) in &grants {
        // This prepared service boundary verifies grant membership, not earned boss progression.
        w.effect(scope(&w, hermit), Effect::Enter(*state)).unwrap();
        assert!(
            w.xml_npc_walkable(scope(&w, hermit), Pos { x: 69.5, y: 64.5 })
                .unwrap()
        );
        w.effect(scope(&w, hermit), Effect::Exit(*state)).unwrap();
        assert!(
            !w.xml_npc_walkable(scope(&w, hermit), Pos { x: 69.5, y: 64.5 })
                .unwrap()
        );
    }
    w.effect(scope(&w, hermit), Effect::Enter(grants[0].0))
        .unwrap();
    let source = w.xml_game().definitions[&0xd61]
        .states
        .iter()
        .flat_map(|s| &s.behaviors)
        .find(|b| b.kind == K::Wander)
        .unwrap()
        .clone();
    let b = Behavior {
        kind: K::Move,
        fields: BTreeMap::from([
            (F::Speed, Value::Number(2.)),
            (F::Angle, Value::Radians(0.)),
            (F::Distance, Value::Number(2.)),
        ]),
        ..source
    };
    let mut rng = rand::rngs::StdRng::seed_from_u64(1);
    assert_eq!(
        w.apply(Service::Movement, &b, step(&w, hermit, b.local), &mut rng)
            .unwrap(),
        ActionOutcome::Applied
    );
    assert!(w.entities[&hermit].pos.x > 68.5);
    assert!(
        w.xml_npc_walkable(scope(&w, ghost), Pos { x: 65.5, y: 64.5 })
            .unwrap()
    );
    assert!(!w.walkable(pos));
    for ground in [0xfe, 0xff] {
        Arc::make_mut(&mut w.map).cells[64 * 128 + 65].ground = ground;
        assert!(
            !w.xml_npc_walkable(scope(&w, ghost), Pos { x: 65.5, y: 64.5 })
                .unwrap()
        );
    }
    Arc::make_mut(&mut w.map).cells[64 * 128 + 65].ground = 0;
    assert!(
        w.xml_npc_walkable(scope(&w, ghost), Pos { x: 65.5, y: 64.5 })
            .unwrap()
    );
    assert!(!w.walkable(Pos { x: 65.5, y: 64.5 }));
    assert!(
        !w.xml_npc_walkable(scope(&w, ghost), Pos { x: -1., y: 64.5 })
            .unwrap()
    );
    assert!(
        !w.xml_npc_walkable(
            scope(&w, ghost),
            Pos {
                x: f32::NAN,
                y: 64.5
            }
        )
        .unwrap()
    );
    Arc::make_mut(&mut w.map).cells[64 * 128 + 65].ground = 0x18;
    let wall = spawn(&mut w, 0x120, 65.5, 64.5);
    assert!(
        !w.xml_npc_walkable(scope(&w, ghost), Pos { x: 65.5, y: 64.5 })
            .unwrap()
    );
    assert!(
        !w.xml_npc_walkable(scope(&w, hermit), Pos { x: 65.5, y: 64.5 })
            .unwrap()
    );
    assert!(w.entities.contains_key(&wall));
}
#[test]
fn xml_runtime_world_active_terrain_grants_are_scoped_not_future_or_biome_guesses() {
    let (mut w, _, id) = fixture(
        r#"<Behavior>RemainVisible</Behavior><State id="a"><Behavior terrain="Dark Water" speed="0">StayInTerrain</Behavior><Transition afterTime="0.05">b</Transition></State><State id="b"><Transition afterTime="0.05">a</Transition></State>"#,
    );
    let probe = Pos { x: 65.5, y: 64.5 };
    Arc::make_mut(&mut w.map).cells[64 * 128 + 65].ground = 0xbc;
    assert!(w.xml_placeable(0x600, probe, &[], None).unwrap());
    frame(&mut w, 0);
    assert!(w.xml_npc_walkable(scope(&w, id), probe).unwrap());
    frame(&mut w, 50);
    assert!(!w.xml_npc_walkable(scope(&w, id), probe).unwrap());
    frame(&mut w, 100);
    assert!(w.xml_npc_walkable(scope(&w, id), probe).unwrap());
    let (mut other, _, actor) = fixture(
        r#"<Behavior>RemainVisible</Behavior><State id="initial"/><State id="future"><Behavior terrain="Dark Water">StayInTerrain</Behavior></State>"#,
    );
    Arc::make_mut(&mut other.map).cells[64 * 128 + 65].ground = 0xbc;
    assert!(other.xml_placeable(0x600, probe, &[], None).unwrap());
    frame(&mut other, 0);
    assert!(!other.xml_npc_walkable(scope(&other, actor), probe).unwrap());
    assert!(other.xml_npc_walkable(scope(&w, id), probe).is_err());
    let (mut ancestor, _, actor) = fixture(
        r#"<Behavior terrain="Dark Water" speed="0">StayInTerrain</Behavior><State id="a"><Behavior terrain="Dark Water" speed="0">StayInTerrain</Behavior><Transition afterTime="0.05">b</Transition></State><State id="b"/>"#,
    );
    Arc::make_mut(&mut ancestor.map).cells[64 * 128 + 65].ground = 0xbc;
    frame(&mut ancestor, 0);
    frame(&mut ancestor, 50);
    assert!(
        ancestor
            .xml_npc_walkable(scope(&ancestor, actor), probe)
            .unwrap()
    );
}
#[test]
fn xml_runtime_world_order_terrain_grants_revoke_on_issuer_retirement() {
    let root = r#"<Behavior>RemainVisible</Behavior><Order targetIds="Piratess"><Behavior terrain="Dark Water" speed="0">StayInTerrain</Behavior></Order>"#;
    let child = r#"<Behavior listenIds="Pirate">FollowOrders</Behavior><Behavior listenIds="Pirate">FollowOrders</Behavior>"#;
    let (mut w, _, issuer) = fixture_program(root, child, LISTS);
    let listener = spawn(&mut w, 0x601, 63.5, 64.5);
    let probe = Pos { x: 62.5, y: 64.5 };
    Arc::make_mut(&mut w.map).cells[64 * 128 + 62].ground = 0xbc;
    assert!(!w.xml_npc_walkable(scope(&w, listener), probe).unwrap());
    frame(&mut w, 0);
    assert!(w.xml_npc_walkable(scope(&w, listener), probe).unwrap());
    assert_eq!(
        w.xml
            .listening
            .keys()
            .filter(|s| s.identity.actor == listener)
            .count(),
        2
    );
    w.remove_entity(issuer);
    assert!(!w.xml_npc_walkable(scope(&w, listener), probe).unwrap());
}

fn update_ids(packet: &Packet) -> (Vec<i32>, Vec<i32>) {
    let mut r = wire::Reader::new(&packet.body);
    let tiles = r.i16().unwrap() as usize;
    r.take(tiles * 5).unwrap();
    let n = r.i16().unwrap();
    let mut added = vec![];
    for _ in 0..n {
        r.i16().unwrap();
        added.push(r.i32().unwrap());
        r.f32().unwrap();
        r.f32().unwrap();
        let stats = r.i16().unwrap();
        for _ in 0..stats {
            let stat = r.u8().unwrap();
            if matches!(stat, 31 | 62) {
                r.utf().unwrap();
            } else {
                r.i32().unwrap();
            }
        }
    }
    let n = r.i16().unwrap();
    let dropped = (0..n).map(|_| r.i32().unwrap()).collect();
    (added, dropped)
}
#[test]
fn xml_runtime_world_remain_visible_actual_sources_native_interest_add_drop_not_terrain_or_waking()
{
    let content = Arc::new(crate::world::test_content());
    let kinds: Vec<_> = content
        .game
        .as_ref()
        .unwrap()
        .definitions
        .values()
        .filter(|d| {
            d.states
                .iter()
                .any(|s| s.behaviors.iter().any(|b| b.kind == K::RemainVisible))
        })
        .map(|d| d.kind)
        .collect();
    assert_eq!(kinds.len(), 13);
    let mut map = flat_map(128);
    map.elevation = Some(vec![200; 128 * 128].into());
    let mut w = World::in_domain(content, Arc::new(map), WorldDomain::Realm).unwrap();
    let mut ch = Character::fresh(w.content.get(0x307).unwrap()).unwrap();
    ch.name = "Ada".into();
    ch.id = 1;
    ch.account = 1;
    let p = w.enter(ch, None).unwrap();
    let mut rx = output(&mut w, p);
    let mut visible = vec![];
    for kind in kinds {
        visible.push(spawn(&mut w, kind, 105.5, 105.5));
    }
    let normal = spawn(&mut w, 0x601, 105.5, 105.5);
    assert_eq!(w.xml.visible.len(), 13);
    assert!(!w.visible_with(p, None).contains(&normal));
    assert!(
        visible
            .iter()
            .all(|id| w.visible_with(p, None).contains(id))
    );
    assert!(!w.players[&p].tiles.contains(&(105, 105)));
    w.updates(p, true).unwrap();
    let mut added = vec![];
    while let Ok(packet) = rx.try_recv() {
        if packet.id == wire::UPDATE {
            added.extend(update_ids(&packet).0);
        }
    }
    assert!(visible.iter().all(|id| added.contains(id)));
    assert!(!added.contains(&normal));
    assert!(!w.players[&p].tiles.contains(&(105, 105)));
    frame(&mut w, 0);
    assert!(w.bullets.is_empty());
    assert!(visible.iter().all(|id| w.xml.actors[id].active_ms() == 0));
    w.apply_condition(visible[0], crate::condition::INVISIBLE, 1000)
        .unwrap();
    assert!(w.visible_with(p, None).contains(&visible[0]));
    assert_ne!(
        w.entities[&visible[0]].conditions(0) & crate::condition::bit(crate::condition::INVISIBLE),
        0
    );
    w.players.get_mut(&p).unwrap().sight_tiles = Some(BTreeSet::from([(64, 64)]));
    assert!(!w.visible_with(p, None).contains(&visible[0]));
    w.players.get_mut(&p).unwrap().sight_tiles = None;
    w.entities.get_mut(&visible[1]).unwrap().published = false;
    assert!(!w.visible_with(p, None).contains(&visible[1]));
    w.entities.get_mut(&visible[1]).unwrap().published = true;
    w.remove_entity(visible[0]);
    w.updates(p, false).unwrap();
    let mut dropped = vec![];
    while let Ok(packet) = rx.try_recv() {
        if packet.id == wire::UPDATE {
            dropped.extend(update_ids(&packet).1);
        }
    }
    assert!(dropped.contains(&visible[0]));
    assert!(!w.players[&p].known.contains(&visible[0]));
    let mut ch = Character::fresh(w.content.get(0x307).unwrap()).unwrap();
    ch.name = "Bea".into();
    ch.id = 2;
    ch.account = 2;
    let q = w.enter(ch, None).unwrap();
    assert!(w.players[&q].known.contains(&visible[2]));
    assert!(!w.players[&q].tiles.contains(&(105, 105)));
}
#[test]
fn xml_runtime_world_remain_visible_scoped_ancestor_reentry_order_and_pending_terminal() {
    let (mut w, p, id) = fixture(
        r#"<Behavior>RemainVisible</Behavior><Behavior>OryxTaunt</Behavior><State id="a"><Behavior>RemainVisible</Behavior><Transition afterTime="0.05">b</Transition></State><State id="b"/>"#,
    );
    w.position_entity(id, Pos { x: 105.5, y: 105.5 }).unwrap();
    assert!(w.visible_with(p, None).contains(&id));
    w.players.get_mut(&p).unwrap().pos = Pos { x: 104.5, y: 105.5 };
    frame(&mut w, 0);
    frame(&mut w, 50);
    assert!(w.xml.visible.contains(&id));
    w.xml.fault = Some(Fault::TerminalBefore);
    assert!(w.xml_defeated(id, TerminalContext::NonPlayer).is_err());
    assert!(w.xml.visible.contains(&id));
    assert!(w.visible_with(p, None).contains(&id));
    frame(&mut w, 100);
    assert!(!w.xml.visible.contains(&id));
    let root = r#"<Behavior text="issuer">Say</Behavior><Order targetIds="Piratess"><Behavior>RemainVisible</Behavior></Order>"#;
    let (mut w, p, issuer) = fixture_program(
        root,
        r#"<Behavior listenIds="Pirate">FollowOrders</Behavior>"#,
        LISTS,
    );
    let listener = spawn(&mut w, 0x601, 63.5, 64.5);
    frame(&mut w, 0);
    w.position_entity(listener, Pos { x: 105.5, y: 105.5 })
        .unwrap();
    assert!(w.visible_with(p, None).contains(&listener));
    w.remove_entity(issuer);
    assert!(!w.visible_with(p, None).contains(&listener));
}

fn effect_fields(p: &Packet) -> (u8, i32, Pos, Pos, u32) {
    assert_eq!(p.id, wire::SHOW_EFFECT);
    assert_eq!(p.body.len(), 25);
    let mut r = wire::Reader::new(&p.body);
    (
        r.u8().unwrap(),
        r.i32().unwrap(),
        Pos {
            x: r.f32().unwrap(),
            y: r.f32().unwrap(),
        },
        Pos {
            x: r.f32().unwrap(),
            y: r.f32().unwrap(),
        },
        r.i32().unwrap() as u32,
    )
}
fn drain(rx: &mut tokio::sync::mpsc::Receiver<Packet>) -> Vec<Packet> {
    let mut packets = vec![];
    while let Ok(p) = rx.try_recv() {
        packets.push(p);
    }
    packets
}
#[test]
fn xml_runtime_world_selected_wire_blast_flag_mutation_same_binary_only_one_cosmetic_pulse() {
    use sha2::{Digest, Sha256};
    let executable = std::env::current_exe().unwrap();
    let hash = format!("{:x}", Sha256::digest(std::fs::read(&executable).unwrap()));
    let mut runs = vec![];
    let mut projectile_vectors = vec![];
    for flag in [false, true] {
        let root = format!(
            r#"<Behavior blastEffect="{}" range="10" numShots="3" angle="10" cooldown="1">Shoot</Behavior>"#,
            u8::from(flag)
        );
        let (mut w, p, id) = fixture(&root);
        w.rng = rand::rngs::StdRng::seed_from_u64(42);
        w.prepared_seed = Some(17);
        let mut rx = output(&mut w, p);
        assert!(!w.players[&p].known.contains(&id));
        frame(&mut w, 0);
        let packets = drain(&mut rx);
        let add = packets
            .iter()
            .position(|x| x.id == wire::UPDATE && update_ids(x).0.contains(&id))
            .unwrap();
        let shot = packets.iter().position(|x| x.id == 19).unwrap();
        assert!(add < shot);
        assert_eq!(
            packets
                .iter()
                .filter(|x| x.id == 19)
                .map(|x| if x.body.len() == 25 {
                    x.body[20] as usize
                } else {
                    1
                })
                .sum::<usize>(),
            3
        );
        let effects: Vec<_> = packets
            .iter()
            .filter(|x| x.id == wire::SHOW_EFFECT)
            .collect();
        assert_eq!(effects.len(), usize::from(flag));
        if flag {
            let (kind, owner, a, b, color) = effect_fields(effects[0]);
            assert_eq!((kind, owner, color), (5, id, 0xffffffff));
            assert_eq!([a.x, a.y, b.x, b.y], [1., 0., 0., 0.]);
            assert!(
                shot < packets
                    .iter()
                    .position(|x| x.id == wire::SHOW_EFFECT)
                    .unwrap()
            );
        }
        projectile_vectors.push(
            w.bullets
                .iter()
                .map(|b| {
                    (
                        b.angle.to_bits(),
                        b.damage,
                        b.spec.speed.to_bits(),
                        b.start.x.to_bits(),
                        b.start.y.to_bits(),
                    )
                })
                .collect::<Vec<_>>(),
        );
        let hp = w.players[&p].character.hp;
        frame(&mut w, 50);
        assert!(drain(&mut rx).is_empty());
        w.apply_condition(id, crate::condition::STUNNED, 2000)
            .unwrap();
        frame(&mut w, 1000);
        assert_eq!(w.bullets.len(), 3);
        assert!(drain(&mut rx).is_empty());
        w.entities
            .get_mut(&id)
            .unwrap()
            .statuses
            .remove(&crate::condition::STUNNED);
        frame(&mut w, 1050);
        assert_eq!(
            drain(&mut rx)
                .iter()
                .filter(|x| x.id == wire::SHOW_EFFECT)
                .count(),
            usize::from(flag)
        );
        assert_eq!(w.players[&p].character.hp, hp);
        let saturated = w.bullets[0].clone();
        w.bullets.resize(2048, saturated);
        frame(&mut w, 2050);
        assert_eq!(w.bullets.len(), 2048);
        assert!(drain(&mut rx).is_empty());
        w.bullets.clear();
        frame(&mut w, 2100);
        assert_eq!(w.bullets.len(), 3);
        assert_eq!(
            drain(&mut rx)
                .iter()
                .filter(|x| x.id == wire::SHOW_EFFECT)
                .count(),
            usize::from(flag)
        );
        let mut b = w.xml_game().definitions[&0x600].states[0].behaviors[0].clone();
        b.fields.insert(F::NumShots, Value::Integer(256));
        assert!(
            w.apply(
                Service::Attack,
                &b,
                step(&w, id, b.local),
                &mut rand::rngs::StdRng::seed_from_u64(1)
            )
            .is_err()
        );
        assert!(drain(&mut rx).is_empty());
        runs.push(serde_json::json!({"xml":root,"fingerprint":w.xml_game().fingerprint,"first_shots":3,"first_pulses":usize::from(flag),"same_cooling_stun_cap_retry_and_damage":true}));
    }
    assert_eq!(projectile_vectors[0], projectile_vectors[1]);
    assert_eq!(
        hash,
        format!("{:x}", Sha256::digest(std::fs::read(executable).unwrap()))
    );
    std::fs::write(std::path::Path::new(&std::env::var("QA_RESULTS_DIR").unwrap()).join("xml-runtime-world-visual-mutation.json"),serde_json::to_vec_pretty(&serde_json::json!({"kind":"real native packet unit evidence, not rendered gameplay","authored_binding":"blastEffect=AreaBlast5/radius1; selected client has no later ConeBlast","executable_sha256":hash,"runs":runs})).unwrap()).unwrap();
}
#[test]
fn xml_runtime_world_native_terminal_projectiles_precede_drop_and_keep_owner_known() {
    let (mut w, p, id) =
        fixture(r#"<Behavior range="2" numShots="3" projectileId="0">Explode</Behavior>"#);
    w.players.get_mut(&p).unwrap().controller = Controller::Human;
    let mut rx = output(&mut w, p);
    frame(&mut w, 0);
    assert!(!w.entities.contains_key(&id));
    assert_eq!(w.bullets.len(), 3);
    let packets = drain(&mut rx);
    let add = packets
        .iter()
        .position(|x| x.id == wire::UPDATE && update_ids(x).0.contains(&id))
        .unwrap();
    let shot = packets.iter().position(|x| x.id == 19).unwrap();
    assert!(add < shot);
    w.updates(p, false).unwrap();
    assert!(
        drain(&mut rx)
            .iter()
            .any(|x| x.id == wire::UPDATE && update_ids(x).1.contains(&id))
    );
    assert_eq!(
        w.players[&p]
            .shoot_acks
            .iter()
            .map(|(_, b)| b.len())
            .sum::<usize>(),
        3
    );
}
#[test]
fn xml_runtime_world_native_lightning_diffuse_sound_and_nova_geometry_not_world_origin() {
    let (mut w, p, id) = fixture("<Behavior>RemainVisible</Behavior>");
    w.position_entity(id, Pos { x: 105.5, y: 100.5 }).unwrap();
    w.players.get_mut(&p).unwrap().pos = Pos { x: 107.5, y: 100.5 };
    let mut rx = output(&mut w, p);
    let mut b = w.xml_game().definitions[&0x600].states[0].behaviors[0].clone();
    b.kind = K::Zap;
    b.fields = BTreeMap::from([
        (F::Range, Value::Number(8.)),
        (F::Color, Value::Colors(vec![0x80ff00aa])),
        (F::Damage, Value::Number(0.)),
        (F::PoisonTotalDamage, Value::Number(0.)),
        (F::PoisonDuration, Value::Millis(0)),
        (F::ConditionDuration, Value::Millis(1000)),
    ]);
    w.apply(
        Service::Attack,
        &b,
        step(&w, id, b.local),
        &mut rand::rngs::StdRng::seed_from_u64(1),
    )
    .unwrap();
    let packets = drain(&mut rx);
    let add = packets
        .iter()
        .position(|x| x.id == wire::UPDATE && update_ids(x).0.contains(&id))
        .unwrap();
    let fx = packets
        .iter()
        .position(|x| x.id == wire::SHOW_EFFECT)
        .unwrap();
    assert!(add < fx);
    let (kind, owner, a, c, color) = effect_fields(&packets[fx]);
    assert_eq!((kind, owner, color), (11, id, 0x80ff00aa));
    assert_eq!([a.x, a.y, c.x, c.y], [107.5, 100.5, 100., 0.]);
    b.kind = K::TrapTrigger;
    b.fields = BTreeMap::from([
        (F::Radius, Value::Number(2.5)),
        (F::MinDamage, Value::Integer(0)),
        (F::MaxDamage, Value::Integer(0)),
        (F::PosJitter, Value::Number(0.5)),
        (F::RadiusVariability, Value::Number(0.)),
        (F::Color, Value::Colors(vec![0x80ff00aa])),
    ]);
    w.apply(
        Service::Attack,
        &b,
        step(&w, id, b.local),
        &mut rand::rngs::StdRng::seed_from_u64(7),
    )
    .unwrap();
    let packets = drain(&mut rx);
    let (kind, owner, center, edge, color) =
        effect_fields(packets.iter().find(|x| x.id == wire::SHOW_EFFECT).unwrap());
    assert_eq!((kind, owner, color), (8, id, 0x80ff00aa));
    assert!((center.distance(edge) - 2.5).abs() < 0.0001);
    assert!(center.distance(w.entities[&id].pos) <= 0.5);
    w.players.get_mut(&p).unwrap().character.items[4] = 0xc2b;
    let plan = w
        .use_item(
            p,
            Slot {
                object: p,
                slot: 4,
                kind: 0xc2b,
            },
            w.players[&p].pos,
        )
        .unwrap()
        .unwrap();
    let sound_actor = w.entities.values().find(|e| e.kind == 0x1407).unwrap().id;
    let revision = plan.revision + 1;
    w.finish_inventory(plan, Ok(revision)).unwrap();
    w.xml_sound(sound_actor, 0).unwrap();
    let packets = drain(&mut rx);
    let sound = packets.iter().find(|x| x.id == wire::PLAY_SOUND).unwrap();
    assert_eq!(sound.body.len(), 5);
    assert_eq!(&sound.body[..4], &sound_actor.to_be_bytes());
    assert_eq!(sound.body[4], 0);
    assert!(w.xml_sound(id, 255).is_err());
    assert!(drain(&mut rx).is_empty());
    w.entities.get_mut(&id).unwrap().published = false;
    w.entities.get_mut(&sound_actor).unwrap().published = false;
    w.xml_diffuse(id, center, 2.5, color).unwrap();
    w.xml_sound(sound_actor, 0).unwrap();
    assert!(drain(&mut rx).is_empty());
    w.entities.get_mut(&id).unwrap().published = true;
    w.entities.get_mut(&id).unwrap().bag_owners = vec![9];
    w.xml_diffuse(id, center, 2.5, color).unwrap();
    assert!(drain(&mut rx).is_empty());
    let (mut w, p, id) =
        fixture(r#"<Behavior time="0" particleEffect="Nova" color="0x80ff00aa">Decay</Behavior>"#);
    w.position_entity(id, Pos { x: 105.5, y: 100.5 }).unwrap();
    w.players.get_mut(&p).unwrap().pos = Pos { x: 107.5, y: 100.5 };
    let mut rx = output(&mut w, p);
    frame(&mut w, 0);
    let packets = drain(&mut rx);
    let add = packets
        .iter()
        .position(|x| x.id == wire::UPDATE && update_ids(x).0.contains(&id))
        .unwrap();
    let fx = packets
        .iter()
        .position(|x| x.id == wire::SHOW_EFFECT)
        .unwrap();
    assert!(add < fx);
    let (kind, owner, center, edge, color) = effect_fields(&packets[fx]);
    assert_eq!((kind, owner, color), (8, id, 0x80ff00aa));
    assert_eq!(
        [center.x, center.y, edge.x, edge.y],
        [105.5, 100.5, 106.5, 100.5]
    );
    assert!(!w.entities.contains_key(&id));
}
#[test]
fn xml_runtime_world_zero_shot_initial_delay_range_zero_stun_stasis_and_dormancy() {
    let root = r#"<Behavior range="0" numShots="0" initialDelay="0.25">Explode</Behavior><Behavior childId="Piratess" min="1" max="1">ReleaseSpawnAtDeath</Behavior>"#;
    let (mut w, _, id) = fixture(root);
    w.apply_condition(id, crate::condition::STUNNED, 5000)
        .unwrap();
    frame(&mut w, 0);
    frame(&mut w, 249);
    assert!(w.entities.contains_key(&id));
    frame(&mut w, 250);
    assert!(!w.entities.contains_key(&id));
    assert_eq!(w.entities.len(), 1);
    frame(&mut w, 300);
    assert_eq!(w.entities.len(), 1);
    let (mut w, _, id) = fixture(root);
    w.apply_condition(id, crate::condition::STASIS, 5000)
        .unwrap();
    frame(&mut w, 0);
    frame(&mut w, 500);
    assert_eq!(w.xml.actors[&id].active_ms(), 0);
    w.entities.get_mut(&id).unwrap().statuses.clear();
    frame(&mut w, 550);
    assert!(w.entities.contains_key(&id));
    frame(&mut w, 750);
    assert!(!w.entities.contains_key(&id));
    let (mut w, p, id) = fixture(root);
    w.players.get_mut(&p).unwrap().pos.x = 100.5;
    frame(&mut w, 0);
    frame(&mut w, 500);
    assert_eq!(w.xml.actors[&id].active_ms(), 0);
    w.players.get_mut(&p).unwrap().pos.x = 80.5;
    frame(&mut w, 550);
    assert!(w.entities.contains_key(&id));
    frame(&mut w, 750);
    assert!(!w.entities.contains_key(&id));
}
#[test]
fn xml_runtime_world_zero_shot_explode_actual_spider_egg_source_activation_range() {
    let content = Arc::new(crate::world::test_content());
    let mut observed = vec![];
    let mut failures = vec![];
    for (distance, expected_terminal) in [(20., false), (2.001, false), (2., true), (1.999, true)] {
        let mut map = flat_map(128);
        map.elevation = Some(vec![200; 128 * 128].into());
        let mut w = World::in_domain(content.clone(), Arc::new(map), WorldDomain::Realm).unwrap();
        w.prepared_seed = Some(123);
        let mut ch = Character::fresh(content.get(0x307).unwrap()).unwrap();
        ch.id = 1;
        ch.account = 1;
        let p = w.enter(ch, None).unwrap();
        w.players.get_mut(&p).unwrap().pos = Pos {
            x: 64.5 + distance,
            y: 64.5,
        };
        let egg = spawn(&mut w, 0x21b, 64.5, 64.5);
        let rule = w.xml_game().definitions[&0x21b]
            .states
            .iter()
            .flat_map(|s| &s.behaviors)
            .find(|b| b.kind == K::Explode)
            .unwrap()
            .clone();
        assert_eq!(n(&rule, F::Range), 2.);
        assert_eq!(n(&rule, F::NumShots), 0.);
        frame(&mut w, 0);
        let terminal = !w.entities.contains_key(&egg);
        let children = w
            .entities
            .values()
            .filter(|e| e.parent == Some(egg))
            .count();
        observed.push(serde_json::json!({"distance":distance,"expected_terminal":expected_terminal,"actual_terminal":terminal,"children":children,"terminal_attempts":w.xml.terminal_attempts.len(),"claim":w.xml.claims.contains(&egg),"source":rule.source}));
        if terminal != expected_terminal
            || (!expected_terminal && children != 0)
            || (expected_terminal && !(2..=7).contains(&children))
        {
            failures.push(format!("distance={distance} expected_terminal={expected_terminal} actual_terminal={terminal} children={children}"));
        }
        frame(&mut w, 50);
        if children
            != w.entities
                .values()
                .filter(|e| e.parent == Some(egg))
                .count()
        {
            failures.push("once-only births changed on next frame".into());
        }
    }
    std::fs::write(std::path::Path::new(&std::env::var("QA_RESULTS_DIR").unwrap()).join("xml-runtime-world-zero-shot-range.json"),serde_json::to_vec_pretty(&serde_json::json!({"scope":"actual selected0x21b World regression; readiness must precede sampling in retained executor","vectors":observed,"failures":failures})).unwrap()).unwrap();
    assert!(failures.is_empty(), "{}", failures.join("; "));
}

#[test]
#[should_panic(expected = "selected sound capability absent object=0x600 soundId=0")]
fn xml_runtime_world_unlinked_selected_sound_refuses_at_startup() {
    fixture(r#"<Behavior soundId="0">TrapTrigger</Behavior>"#);
}
#[test]
fn xml_runtime_world_avoid_water_is_exact_selected_sink_policy_not_names_or_movement_permission() {
    let content = Arc::new(crate::world::test_content());
    let raw = std::fs::read_to_string("data/grounds.xml").unwrap();
    let doc = roxmltree::Document::parse(&raw).unwrap();
    let flags: BTreeSet<_> = doc
        .descendants()
        .filter(|n| n.has_tag_name("Ground") && n.children().any(|c| c.has_tag_name("Sink")))
        .map(|n| {
            u8::from_str_radix(n.attribute("type").unwrap().trim_start_matches("0x"), 16).unwrap()
        })
        .collect();
    assert_eq!(flags.len(), 25);
    assert_eq!(flags, content.sinking_grounds);
    assert!(!flags.contains(&0));
    assert!(content.objects[&0xe37].flying);
    let mut map = flat_map(128);
    map.elevation = Some(vec![200; 128 * 128].into());
    let mut w = World::in_domain(content, Arc::new(map), WorldDomain::Realm).unwrap();
    let id = spawn(&mut w, 0xe37, 64.5, 64.5);
    let mut b = w.xml_game().definitions[&0xe37]
        .states
        .iter()
        .flat_map(|s| &s.behaviors)
        .find(|b| b.kind == K::Wander)
        .unwrap()
        .clone();
    b.fields.insert(F::Speed, Value::Number(2.));
    for ground in [
        0x17, 0xc9, 0x72, 0x73, 0xbc, 0xfa, 0xb1, 0xb2, 0xb3, 0x70, 0x82, 0xa9, 0x18, 0,
    ] {
        for c in &mut Arc::make_mut(&mut w.map).cells {
            c.ground = ground;
        }
        w.position_entity(id, Pos { x: 64.5, y: 64.5 }).unwrap();
        w.xml.motions.clear();
        b.fields.insert(F::AvoidWater, Value::Bool(true));
        let result = w
            .apply(
                Service::Movement,
                &b,
                step(&w, id, b.local),
                &mut rand::rngs::StdRng::seed_from_u64(1),
            )
            .unwrap();
        if flags.contains(&ground) {
            assert_matches!(result, ActionOutcome::Blocked);
            assert_eq!(w.entities[&id].pos.x, 64.5);
            assert!(w.xml.motions.is_empty());
        } else {
            assert_matches!(result, ActionOutcome::Applied);
        }
        b.fields.insert(F::AvoidWater, Value::Bool(false));
        assert_matches!(
            w.apply(
                Service::Movement,
                &b,
                step(&w, id, b.local),
                &mut rand::rngs::StdRng::seed_from_u64(1)
            )
            .unwrap(),
            ActionOutcome::Applied
        );
    }
    let grounded = spawn(&mut w, 0x601, 70.5, 64.5);
    assert!(
        !w.xml_npc_walkable(scope(&w, grounded), Pos { x: 71.5, y: 64.5 })
            .unwrap()
    );
    assert!(!w.walkable(Pos { x: 71.5, y: 64.5 }));
}
#[test]
fn xml_runtime_world_motion_numeric_refusal_is_bounded_and_finite_bounds_are_blocked() {
    use rand::RngCore;
    let (mut w, p, id) = fixture("<Behavior>RemainVisible</Behavior>");
    let mut rx = output(&mut w, p);
    let mut b = w.xml_game().definitions[&0x600].states[0].behaviors[0].clone();
    b.kind = K::Move;
    b.fields = BTreeMap::from([
        (F::Speed, Value::Number(1e25)),
        (F::Distance, Value::Number(1e25)),
        (F::Angle, Value::Radians(0.)),
    ]);
    let mut rng = rand::rngs::StdRng::seed_from_u64(31);
    let before = rng.clone().next_u64();
    assert_matches!(
        w.apply(Service::Movement, &b, step(&w, id, b.local), &mut rng)
            .unwrap(),
        ActionOutcome::Blocked
    );
    assert_eq!(rng.clone().next_u64(), before);
    assert!(w.xml.motions.is_empty());
    assert_eq!(w.entities[&id].pos.x, 64.5);
    assert!(drain(&mut rx).is_empty());
    b.fields.insert(F::Speed, Value::Number(f64::MAX));
    let mut st = step(&w, id, b.local);
    st.delta_ms = u64::MAX;
    let error = w.apply(Service::Movement, &b, st, &mut rng).unwrap_err();
    let error = format!("{error:#}");
    assert!(error.contains("cannot be represented by native scalar coordinates"));
    assert!(error.contains("object=Pirate") && error.contains("rules.xml:1"));
    assert!(w.xml.motions.is_empty());
    assert_eq!(w.entities[&id].pos.x, 64.5);
    b.fields.insert(F::Distance, Value::Number(f64::MAX));
    assert!(
        w.apply(Service::Movement, &b, step(&w, id, b.local), &mut rng)
            .is_err()
    );
    assert!(w.xml.motions.is_empty());
    assert!(drain(&mut rx).is_empty());
    w.position_entity(id, Pos { x: 125.5, y: 64.5 }).unwrap();
    b.fields.insert(F::Speed, Value::Number(50.));
    b.fields.insert(F::Distance, Value::Number(2.5));
    assert_matches!(
        w.apply(Service::Movement, &b, step(&w, id, b.local), &mut rng)
            .unwrap(),
        ActionOutcome::Blocked
    );
    assert_eq!(w.entities[&id].pos.x, 125.5);
    assert!(
        !w.xml_npc_walkable(scope(&w, id), Pos { x: 128., y: 64.5 })
            .unwrap()
    );
    assert!(
        w.xml_npc_walkable(
            scope(&w, id),
            Pos {
                x: 127.99999,
                y: 64.5
            }
        )
        .unwrap()
    );
    b.fields.insert(F::Distance, Value::Number(2.));
    Arc::make_mut(&mut w.map).cells[64 * 128 + 126].ground = 255;
    assert_matches!(
        w.apply(Service::Movement, &b, step(&w, id, b.local), &mut rng)
            .unwrap(),
        ActionOutcome::Blocked
    );
    assert_eq!(w.entities[&id].pos.x, 125.5);
    Arc::make_mut(&mut w.map).cells[64 * 128 + 126].ground = 0x18;
    assert_matches!(
        w.apply(Service::Movement, &b, step(&w, id, b.local), &mut rng)
            .unwrap(),
        ActionOutcome::Applied
    );
    assert_eq!(w.entities[&id].pos.x, 127.5);
}
#[test]
fn xml_runtime_world_predictive_shoot_arc_boundaries_and_actual_motion_facing() {
    let (mut w, p, id) =
        fixture(r#"<Behavior range="10" predictive="1" arcDegrees="90">Shoot</Behavior>"#);
    let b = w.xml_game().definitions[&0x600].states[0].behaviors[0].clone();
    w.players.get_mut(&p).unwrap().pos = Pos { x: 69.5, y: 65.5 };
    w.players.get_mut(&p).unwrap().motion.velocity = Pos { x: 0., y: 2. };
    w.players.get_mut(&p).unwrap().motion.at = 0;
    let mut rng = rand::rngs::StdRng::seed_from_u64(1);
    assert_matches!(
        w.apply(Service::Attack, &b, step(&w, id, b.local), &mut rng)
            .unwrap(),
        ActionOutcome::Applied
    );
    let travel =
        (26f64).sqrt() / (f64::from(w.content.objects[&0x600].projectiles[&0].speed) / 10.);
    let expected = (1. + 2. * travel).atan2(5.);
    assert!((f64::from(w.bullets[0].angle) - expected).abs() < 0.00001);
    let mut b = b.clone();
    b.fields.insert(F::Predictive, Value::Number(0.));
    for (y, applied) in [(69.499, true), (69.5, true), (69.501, false)] {
        w.players.get_mut(&p).unwrap().pos = Pos { x: 69.5, y };
        let outcome = w
            .apply(Service::Attack, &b, step(&w, id, b.local), &mut rng)
            .unwrap();
        assert_eq!(matches!(outcome, ActionOutcome::Applied), applied);
    }
    w.players.get_mut(&p).unwrap().pos = Pos { x: 59.5, y: 64.5 };
    b.fields.insert(F::ArcDegrees, Value::Radians(0.));
    assert_matches!(
        w.apply(Service::Attack, &b, step(&w, id, b.local), &mut rng)
            .unwrap(),
        ActionOutcome::Applied
    );
    b.fields
        .insert(F::ArcDegrees, Value::Radians(std::f64::consts::FRAC_PI_2));
    w.players.get_mut(&p).unwrap().pos = Pos { x: 64.5, y: 69.5 };
    assert_matches!(
        w.apply(Service::Attack, &b, step(&w, id, b.local), &mut rng)
            .unwrap(),
        ActionOutcome::Blocked
    );
    let mut movement = b.clone();
    movement.kind = K::Move;
    movement.fields = BTreeMap::from([
        (F::Speed, Value::Number(1.)),
        (F::Distance, Value::Number(1.)),
        (F::Angle, Value::Radians(std::f64::consts::FRAC_PI_2)),
    ]);
    w.apply(
        Service::Movement,
        &movement,
        step(&w, id, movement.local),
        &mut rng,
    )
    .unwrap();
    assert_matches!(
        w.apply(Service::Attack, &b, step(&w, id, b.local), &mut rng)
            .unwrap(),
        ActionOutcome::Applied
    );
}
#[test]
fn xml_runtime_world_remain_visible_child_exit_reentry_and_dormant_membership() {
    let (mut w, p, id) = fixture(
        r#"<State id="a"><Behavior>RemainVisible</Behavior><Transition afterTime="0.05">b</Transition></State><State id="b"><Transition afterTime="0.05">a</Transition></State>"#,
    );
    w.position_entity(id, Pos { x: 105.5, y: 105.5 }).unwrap();
    assert!(w.visible_with(p, None).contains(&id));
    w.players.get_mut(&p).unwrap().pos = Pos { x: 104.5, y: 105.5 };
    frame(&mut w, 0);
    frame(&mut w, 50);
    w.players.get_mut(&p).unwrap().pos = Pos { x: 64.5, y: 64.5 };
    assert!(!w.visible_with(p, None).contains(&id));
    w.players.get_mut(&p).unwrap().pos = Pos { x: 104.5, y: 105.5 };
    frame(&mut w, 100);
    w.players.get_mut(&p).unwrap().pos = Pos { x: 64.5, y: 64.5 };
    assert!(w.visible_with(p, None).contains(&id));
    frame(&mut w, 1000);
    assert_eq!(w.xml.actors[&id].active_ms(), 100);
    assert!(w.visible_with(p, None).contains(&id));
}

#[test]
fn xml_runtime_world_order_native_scope_withdrawal_and_retirement_revoke_only_owner_immediately() {
    for retire in [false, true] {
        let (mut w, _, issuer) = fixture_program(
            r#"<Order targetIds="Piratess"><Behavior effect="Armored">ConditionEffect</Behavior><Behavior>RemainVisible</Behavior></Order>"#,
            r#"<Behavior effect="Invulnerable">ConditionEffect</Behavior><Behavior listenIds="Pirate">FollowOrders</Behavior>"#,
            LISTS,
        );
        let listener = spawn(&mut w, 0x601, 63.5, 64.5);
        frame(&mut w, 0);
        let armored = crate::condition::bit(crate::condition::ARMORED);
        let invulnerable = crate::condition::bit(crate::condition::INVULNERABLE);
        assert_ne!(w.entities[&listener].conditions(0) & armored, 0);
        assert!(w.xml.visible.contains(&listener));
        if retire {
            w.remove_entity(issuer);
        } else {
            let local = w.xml.orders.keys().find(|(id, _)| *id == issuer).unwrap().1;
            w.effect(scope(&w, issuer), Effect::WithdrawOrder(local))
                .unwrap();
        }
        assert_eq!(w.entities[&listener].conditions(0) & armored, 0);
        assert_ne!(w.entities[&listener].conditions(0) & invulnerable, 0);
        assert!(!w.xml.visible.contains(&listener));
        frame(&mut w, 50);
        assert_eq!(w.entities[&listener].conditions(50) & armored, 0);
        assert_ne!(w.entities[&listener].conditions(50) & invulnerable, 0);
    }
}
/// Ghost King's Overly_long_combat orders Ghost Master (actors.xml:3936-3938) to Decay; the merged
/// listener terminal keeps its order scope as origin and must still be claimed by the retiring actor.
#[test]
fn xml_runtime_world_ordered_decay_and_explode_retire_the_listening_actor() {
    for order in [
        r#"<Behavior time="0.1">Decay</Behavior>"#,
        r#"<Behavior numShots="0">Explode</Behavior>"#,
    ] {
        let (mut w, _, issuer) = fixture_program(
            &format!(
                r#"<Behavior>RemainVisible</Behavior><Order targetIds="Piratess">{order}</Order>"#
            ),
            r#"<Behavior listenIds="Pirate">FollowOrders</Behavior>"#,
            LISTS,
        );
        let listener = spawn(&mut w, 0x601, 63.5, 64.5);
        for at in [0, 50, 100, 150] {
            frame(&mut w, at);
        }
        assert!(!w.entities.contains_key(&listener), "{order}");
        assert!(w.entities.contains_key(&issuer));
        assert!(w.xml.retry_terminals.is_empty());
    }
}
#[test]
fn xml_runtime_world_faulted_actor_is_quarantined_and_other_actors_keep_ticking() {
    let (mut w, _, id) = fixture(
        r#"<Behavior>RemainVisible</Behavior><State id="a"><Transition hitpointsLessThan="0.5">b</Transition></State><State id="b"/>"#,
    );
    let other = spawn(&mut w, 0x601, 63.5, 64.5);
    frame(&mut w, 0);
    w.xml.identities.get_mut(&id).unwrap().encounter = Some(999);
    w.now = 50;
    let error = w.xml_step().unwrap_err();
    assert!(error.is::<ActorsQuarantined>(), "{error:#}");
    assert!(format!("{error:#}").contains("XML full identity mismatch"));
    assert!(!w.entities.contains_key(&id));
    assert!(w.xml.actors.contains_key(&other));
    frame(&mut w, 100);
    assert!(w.entities.contains_key(&other));
}
#[test]
fn xml_runtime_world_full_parent_encounter_identity_is_not_only_actor_number() {
    let (mut w, _, id) = fixture("<Behavior>RemainVisible</Behavior>");
    let mut b = w.xml_game().definitions[&0x600].states[0].behaviors[0].clone();
    b.kind = K::Move;
    b.fields = BTreeMap::from([
        (F::Speed, Value::Number(1.)),
        (F::Distance, Value::Number(1.)),
        (F::Angle, Value::Radians(0.)),
    ]);
    let mut bad = step(&w, id, b.local);
    bad.token.scope.identity.encounter = Some(999);
    let error = w
        .apply(
            Service::Movement,
            &b,
            bad,
            &mut rand::rngs::StdRng::seed_from_u64(1),
        )
        .unwrap_err();
    assert!(format!("{error:#}").contains("full identity mismatch"));
    assert!(w.xml.motions.is_empty());
    assert_eq!(w.entities[&id].pos.x, 64.5);
}

#[test]
fn xml_runtime_world_actual_spider_egg_retry_plan_survives_competing_killer_and_releases_parent_cap()
 {
    let content = Arc::new(crate::world::test_content());
    let mut map = flat_map(128);
    map.elevation = Some(vec![200; 128 * 128].into());
    let mut w = World::in_domain(content.clone(), Arc::new(map), WorldDomain::Realm).unwrap();
    w.prepared_seed = Some(123);
    let mut ch = Character::fresh(content.get(0x307).unwrap()).unwrap();
    ch.id = 1;
    ch.account = 1;
    ch.name = "Ada".into();
    let p = w.enter(ch, None).unwrap();
    w.players.get_mut(&p).unwrap().pos.x = 65.5;
    let id = spawn(&mut w, 0x21b, 64.5, 64.5);
    w.xml.fault = Some(Fault::TerminalBefore);
    frame(&mut w, 0);
    assert!(w.entities.contains_key(&id));
    assert!(w.xml.retry_terminals.contains(&id));
    assert_eq!(w.xml.terminal_attempts.len(), 1);
    assert_eq!(
        w.entities.values().filter(|e| e.parent == Some(id)).count(),
        0
    );
    assert_matches!(w.xml.terminal_attempts[0].reason, TerminalReason::Explode);
    assert_matches!(
        w.xml.terminal_attempts[0].context,
        Some(TerminalContext::NonPlayer)
    );
    let plan = format!("{:?}", w.xml.terminal_attempts[0]);
    let origin = w.damage_origin(p).unwrap();
    w.damage_enemy_from(&origin, id, 10000, false, &[], None)
        .unwrap();
    if let Err(error) = drain_events(&mut w) {
        eprintln!("journal drain retained event: {error:#}");
    }
    assert_eq!(w.xml.terminal_attempts.len(), 2);
    assert_eq!(plan, format!("{:?}", w.xml.terminal_attempts[1]));
    assert!(!w.entities.contains_key(&id));
    let children = w
        .entities
        .values()
        .filter(|e| e.parent == Some(id))
        .map(|e| e.id)
        .collect::<Vec<_>>();
    assert!((2..=7).contains(&children.len()));
    assert_eq!(w.xml.claims.len(), 1);
    assert_eq!(w.actor_ids.len(), children.len());
    let positions = children
        .iter()
        .map(|id| {
            (
                w.entities[id].pos.x.to_bits(),
                w.entities[id].pos.y.to_bits(),
            )
        })
        .collect::<Vec<_>>();
    w.xml_defeated(id, TerminalContext::NonPlayer).unwrap();
    if let Err(error) = drain_events(&mut w) {
        eprintln!("journal drain retained event: {error:#}");
    }
    assert_eq!(
        positions,
        children
            .iter()
            .map(|id| (
                w.entities[id].pos.x.to_bits(),
                w.entities[id].pos.y.to_bits()
            ))
            .collect::<Vec<_>>()
    );
    frame(&mut w, 50);
    assert_eq!(
        children.iter().copied().collect::<BTreeSet<_>>(),
        w.entities
            .values()
            .filter(|e| e.parent == Some(id))
            .map(|e| e.id)
            .collect()
    );
    assert_eq!(w.xml.terminal_attempts.len(), 2);
    for child in children {
        w.remove_entity(child);
    }
    assert!(w.actor_ids.is_empty());
    assert!(w.xml.births.is_empty());
    assert!(w.xml.rewards.is_empty());
}

#[test]
fn xml_runtime_world_stamp_native_tiles_add_drop_ownership_and_reserved_dynamic_refusal() {
    let (mut w, p, id) = fixture(r#"<Behavior map="Stamp">PlaceMap</Behavior>"#);
    let b = w.xml_game().definitions[&0x600].states[0].behaviors[0].clone();
    Arc::make_mut(&mut w.map).cells[64 * 128 + 64].ground = 0x73;
    let wall = spawn(&mut w, 0x120, 64.5, 65.5);
    let mut rx = output(&mut w, p);
    w.updates(p, true).unwrap();
    drain(&mut rx);
    w.apply(
        Service::Map,
        &b,
        step(&w, id, b.local),
        &mut rand::rngs::StdRng::seed_from_u64(1),
    )
    .unwrap();
    let child = w.entities.values().find(|e| e.kind == 0x601).unwrap().id;
    assert_eq!(w.entities[&child].parent, Some(id));
    assert_eq!(
        w.xml.identities[&child].encounter,
        scope(&w, id).identity.encounter
    );
    w.updates(p, false).unwrap();
    let packets = drain(&mut rx);
    let mut observed_tile = false;
    let mut added = false;
    let mut dropped = false;
    for packet in packets.iter().filter(|x| x.id == wire::UPDATE) {
        let mut r = wire::Reader::new(&packet.body);
        let n = r.i16().unwrap();
        for _ in 0..n {
            let x = r.i16().unwrap();
            let y = r.i16().unwrap();
            let ground = r.u8().unwrap();
            if (x, y) == (64, 64) {
                assert_eq!(ground, 0x48);
                observed_tile = true;
            }
        }
        let (a, d) = update_ids(packet);
        added |= a.contains(&child);
        dropped |= d.contains(&wall);
    }
    assert!(observed_tile && added && dropped);
    let (mut w, _, id) = fixture(r#"<Behavior map="Stamp">PlaceMap</Behavior>"#);
    let bag = w
        .insert_native_entity(
            0x504,
            NativeService::Container,
            Pos { x: 64.5, y: 65.5 },
            vec![-1; 8],
            "reserved-container stamp fixture",
        )
        .unwrap();
    w.entities.get_mut(&bag).unwrap().reserved = true;
    let before = serde_json::to_value(&w.map.cells).unwrap();
    let b = w.xml_game().definitions[&0x600].states[0].behaviors[0].clone();
    // A reserved interaction only delays the stamp (maps.rs StampBlocked); it is not a world fault.
    assert_matches!(
        w.apply(
            Service::Map,
            &b,
            step(&w, id, b.local),
            &mut rand::rngs::StdRng::seed_from_u64(1)
        ),
        Ok(ActionOutcome::Blocked)
    );
    assert_eq!(serde_json::to_value(&w.map.cells).unwrap(), before);
    assert!(w.entities.contains_key(&bag));
    assert_eq!(w.entities.len(), 2);
}
#[test]
fn xml_runtime_world_stamp_parent_cap_is_blocked_then_releases_without_spending_max_place() {
    let (mut w, _, id) = fixture(r#"<Behavior map="Stamp" maxPlace="1">PlaceMap</Behavior>"#);
    let mut children = vec![];
    for _ in 0..CHILD_LIMIT {
        let child = spawn(&mut w, 0x601, 70.5, 70.5);
        w.xml_own(child, Some(id), scope(&w, id).identity.encounter)
            .unwrap();
        children.push(child);
    }
    frame(&mut w, 0);
    assert_eq!(w.xml_owned_count(id), CHILD_LIMIT);
    assert!(
        !w.entities
            .values()
            .any(|e| e.kind == 0x601 && e.pos.x == 65.5)
    );
    w.remove_entity(children[0]);
    frame(&mut w, 50);
    assert_eq!(w.xml_owned_count(id), CHILD_LIMIT);
    assert!(
        w.entities
            .values()
            .any(|e| e.kind == 0x601 && e.pos.x == 65.5 && e.pos.y == 65.5)
    );
    frame(&mut w, 100);
    assert_eq!(w.xml_owned_count(id), CHILD_LIMIT);
}
#[test]
fn xml_runtime_world_copy_damage_record_default_forty_range_and_missing_target_is_blocked() {
    let (mut w, _, id) = fixture(r#"<Behavior targetId="Piratess">CopyDamageRecord</Behavior>"#);
    let b = w.xml_game().definitions[&0x600].states[0].behaviors[0].clone();
    let mut rng = rand::rngs::StdRng::seed_from_u64(1);
    assert!(
        !w.ready(Service::DamageRecord, &b, step(&w, id, b.local))
            .unwrap()
    );
    assert_matches!(
        w.apply(Service::DamageRecord, &b, step(&w, id, b.local), &mut rng)
            .unwrap(),
        ActionOutcome::Blocked
    );
    let target = spawn(&mut w, 0x601, 104.5, 64.5);
    w.entities.get_mut(&target).unwrap().damage.insert(7, 40);
    assert!(
        w.ready(Service::DamageRecord, &b, step(&w, id, b.local))
            .unwrap()
    );
    assert_matches!(
        w.apply(Service::DamageRecord, &b, step(&w, id, b.local), &mut rng)
            .unwrap(),
        ActionOutcome::Applied
    );
    assert_eq!(w.entities[&id].damage.get(&7), Some(&40));
    w.position_entity(
        target,
        Pos {
            x: 104.501,
            y: 64.5,
        },
    )
    .unwrap();
    assert!(
        !w.ready(Service::DamageRecord, &b, step(&w, id, b.local))
            .unwrap()
    );
}

#[test]
fn xml_runtime_world_child_shoot_blast_is_one_native_pulse_and_uses_documented_auto_angle() {
    let (mut w, p, id) = fixture(
        r#"<Behavior childId="Piratess" type="auto" range="1" numShots="3" angle="10" blastEffect="1">Shoot</Behavior>"#,
    );
    let mut rx = output(&mut w, p);
    frame(&mut w, 0);
    let children: Vec<_> = w
        .entities
        .values()
        .filter(|e| e.parent == Some(id))
        .collect();
    assert_eq!(children.len(), 3);
    for (child, degrees) in children.iter().zip([80f64, 90., 100.]) {
        let angle = (f64::from(child.pos.y) - 64.5).atan2(f64::from(child.pos.x) - 64.5);
        assert!(
            (angle - degrees.to_radians()).abs() < 0.0001,
            "child auto angle {angle}"
        );
    }
    let packets = drain(&mut rx);
    assert!(!packets.iter().any(|x| x.id == 19));
    let add = packets
        .iter()
        .position(|x| x.id == wire::UPDATE && update_ids(x).0.contains(&id))
        .unwrap();
    let effects: Vec<_> = packets
        .iter()
        .filter(|x| x.id == wire::SHOW_EFFECT)
        .collect();
    assert_eq!(effects.len(), 1);
    assert_eq!(effect_fields(effects[0]).0, 5);
    assert!(
        add < packets
            .iter()
            .position(|x| x.id == wire::SHOW_EFFECT)
            .unwrap()
    );
    frame(&mut w, 50);
    assert!(!drain(&mut rx).iter().any(|x| x.id == wire::SHOW_EFFECT));
}

#[path = "admission_tests.rs"]
mod admission_tests;
#[path = "movement_tests.rs"]
mod movement_tests;

#[test]
fn xml_runtime_world_stamp_refuses_a_changed_catalog_before_mutation() {
    let (mut w, _, id) = fixture(r#"<Behavior map="Stamp">PlaceMap</Behavior>"#);
    let stamp = w.xml_prepare_stamp(0, w.entities[&id].pos).unwrap();
    let ids = w.ids.load(Ordering::Relaxed);
    let before = serde_json::to_vec(&w.map.cells).unwrap();
    let mut game = w.xml_game().clone();
    game.definitions.remove(&0x601);
    Arc::make_mut(&mut w.content).game = Some(Arc::new(game));
    assert!(
        w.xml_apply_stamp(stamp, w.xml_identity(id).unwrap())
            .is_err()
    );
    assert_eq!(w.ids.load(Ordering::Relaxed), ids);
    assert_eq!(serde_json::to_vec(&w.map.cells).unwrap(), before);
}

#[test]
fn xml_runtime_world_change_ground_updates_only_observed_native_tiles() {
    let (mut w, p, id) = fixture(r#"<Behavior groundId="Grass">ChangeGround</Behavior>"#);
    let q = w
        .enter(
            {
                let mut c = w.players[&p].character.clone();
                c.account = 2;
                c.id = 2;
                c
            },
            None,
        )
        .unwrap();
    w.players.get_mut(&p).unwrap().tiles.insert((64, 64));
    w.players.get_mut(&q).unwrap().tiles.clear();
    let b = w.xml_game().definitions[&0x600].states[0].behaviors[0].clone();
    w.apply(
        Service::Ground,
        &b,
        step(&w, id, b.local),
        &mut rand::rngs::StdRng::seed_from_u64(1),
    )
    .unwrap();
    assert_eq!(w.map.cells[64 * 128 + 64].ground, 0x48);
    assert_eq!(w.players[&p].tile_updates, [(64, 64, 0x48)]);
    assert!(w.players[&q].tile_updates.is_empty() && w.players[&q].tiles.is_empty());
}

#[test]
fn xml_runtime_world_breath_numeric_preflight_is_atomic_not_a_saturating_cast() {
    let (mut w, p, id) = fixture(
        r#"<Behavior radius="4" speed="100000000000000000000000000000000000000">RestoresBreath</Behavior>"#,
    );
    let q = w
        .enter(
            {
                let mut c = w.players[&p].character.clone();
                c.account = 2;
                c.id = 2;
                c
            },
            None,
        )
        .unwrap();
    w.players.get_mut(&p).unwrap().oxygen = 50;
    w.players.get_mut(&q).unwrap().oxygen = 80;
    let b = w.xml_game().definitions[&0x600].states[0].behaviors[0].clone();
    let error = format!(
        "{:#}",
        w.apply(
            Service::Breath,
            &b,
            step(&w, id, b.local),
            &mut rand::rngs::StdRng::seed_from_u64(1)
        )
        .unwrap_err()
    );
    assert!(error.contains("breath credit exceeds integer status capacity"));
    assert_eq!((w.players[&p].oxygen, w.players[&q].oxygen), (50, 80));
    assert!(w.xml.breath.is_empty());
}

#[cfg(test)]
#[path = "context_tests.rs"]
mod context_tests;

pub(crate) fn drain_events(w: &mut World) -> Result<()> {
    drain_events_plan(w, None)
}
pub(super) fn drain_events_plan(w: &mut World, plan: Option<&TerminalPlan>) -> Result<()> {
    if w.pending_terminal_events().is_empty() && w.delivery_events().is_empty() {
        return Ok(());
    }
    std::thread::scope(|scope| {
        scope.spawn(||->Result<()> {
        let runtime=tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        runtime.block_on(async {
            let path=std::path::PathBuf::from(std::env::var("QA_RESULTS_DIR")?).join(format!("world-terminal-{}.db",w.xml.instance));
            let store=crate::store::Store::open(&path,&w.content)?;
            let db=rusqlite::Connection::open(&path)?;
            for p in w.players.values() {
                let ch=&p.character;
                db.execute("INSERT OR IGNORE INTO accounts(id,guid,name) VALUES(?1,?2,?3)",rusqlite::params![ch.account,format!("world-component-{}",ch.account),ch.name])?;
                db.execute("INSERT OR IGNORE INTO account_fame VALUES(?1,0,0)",[ch.account])?;
                db.execute("INSERT OR IGNORE INTO vaults(account,chest,items) VALUES(?1,0,'[-1,-1,-1,-1,-1,-1,-1,-1]')",[ch.account])?;
                db.execute("INSERT INTO characters(id,account,stats,items,revision,alive) SELECT ?1,?2,?3,?4,?5,?6 WHERE NOT EXISTS(SELECT 1 FROM characters WHERE id=?1)",rusqlite::params![ch.id,ch.account,serde_json::to_string(ch)?,serde_json::to_string(&ch.items)?,ch.revision,ch.alive])?;
            }
            drop(db);
            for event in w.pending_terminal_events() {
                let record=store.coordinate_terminal(event.clone()).await?;
                if let Some(plan)=plan {
                    ensure!(w.xml.terminals[&plan.origin.identity.actor].event==record.event,"service-plan receipt mismatch");
                    w.xml.terminals.get_mut(&plan.origin.identity.actor).unwrap().receipt=true;
                    w.xml_terminal(plan)?;
                }
                w.finish_terminal_event(record)?;
                store.publish_world_event(event.clone()).await?;
                w.acknowledge_terminal_publication(event.id);
            }
            for event in w.delivery_events() {
                let expiry=w.entitlement_expiries(&event);
                if !expiry.is_empty(){store.expire_world_entitlements(event.clone(),expiry).await?;}
                let rows=store.unpicked_entitlements(event.clone()).await?;
                w.publish_entitlements(event,rows)?;
            }
            Ok(())
        })
    }).join().expect("shared real-journal test drain panicked")
    })
}

#[test]
fn xml_rewards_terminal_only_dragon_egg_runs_without_idle_executor_while_trees_stay_scenery() {
    let content = Arc::new(crate::world::test_content());
    let definitions = &content.game.as_ref().unwrap().definitions;
    // Dragon Egg is a destructible (Enemy) terminal-only kind; Tree A's death hook has no hit box to reach it.
    assert!(admission::terminal_only(&definitions[&0x206]) && content.objects[&0x206].destructible);
    assert!(
        admission::terminal_only(&definitions[&0x100]) && !content.objects[&0x100].destructible
    );
    assert!(!admission::terminal_only(&definitions[&0x600]));
    let mut observations = Vec::new();
    for eager in [false, true] {
        let mut map = flat_map(64);
        map.spawns = vec![(16.5, 16.5)];
        map.cells[32 * 64 + 32].object = Some(0x206);
        map.cells[40 * 64 + 40].object = Some(0x100);
        let mut w = World::new(content.clone(), Arc::new(map)).unwrap();
        w.xml.world = 12351;
        w.prepared_seed = Some(52);
        w.rng = rand::rngs::StdRng::seed_from_u64(52);
        let tree = w.entities.values().find(|e| e.kind == 0x100).unwrap().id;
        assert!(!w.actor_ids.contains(&tree));
        assert_matches!(
            w.entities[&tree].state,
            entity_store::EntityState::Shared(_)
        );
        let id = w.entities.values().find(|e| e.kind == 0x206).unwrap().id;
        assert!(w.actor_ids.contains(&id));
        assert!(!w.xml.runnable.contains(&id));
        let ch = Character::fresh(content.get(0x307).unwrap()).unwrap();
        let p = w
            .enter(
                Character {
                    id: 1,
                    account: 1,
                    name: "Ada".into(),
                    ..ch
                },
                None,
            )
            .unwrap();
        let before = serde_json::json!({"entity":w.entities[&id],"actors":w.actor_ids,"cell":w.cells.ids(32*64+32)});
        if eager {
            let mut actor = w.xml_actor(id).unwrap();
            actor
                .tick(
                    Frame {
                        delta_ms: 0,
                        dormant: true,
                        stasis: false,
                        stunned: false,
                    },
                    &mut w,
                )
                .unwrap();
            w.xml.actors.insert(id, actor);
        }
        frame(&mut w, 6000);
        if !eager {
            assert!(!w.xml.actors.contains_key(&id));
        }
        let hp = w.entities[&id].hp;
        assert!(hp > 0);
        w.damage_enemy(p, id, hp, true, &[], None).unwrap();
        assert!(w.entities.contains_key(&id));
        assert!(!w.xml.claims.contains(&id));
        let event = w.pending_terminal_events()[0].clone();
        drain_events(&mut w).unwrap();
        drain_events(&mut w).unwrap();
        assert!(!w.entities.contains_key(&id));
        assert_eq!(w.xml.rewards[&id].entity.damage[&1], hp);
        observations.push(serde_json::json!({"before":before,"entities":w.entities.values().collect::<Vec<_>>(),"plan":format!("{:?}",w.xml.rewards[&id].plan),"ledger":w.xml.rewards[&id].entity.damage,"next_rng":w.rng.r#gen::<u64>(),"allocation":event.terminal_payload().unwrap().allocation.as_ref().unwrap().allocations()}));
    }
    assert_eq!(observations[0], observations[1]);
}

#[path = "journal_tests.rs"]
mod journal_tests;

#[path = "batching_tests.rs"]
mod batching_tests;

#[path = "review_tests.rs"]
mod review_tests;

#[path = "publication_tests.rs"]
mod publication_tests;

#[path = "elevation_tests.rs"]
mod elevation_tests;

#[path = "ab_review_tests.rs"]
mod ab_review_tests;
#[path = "cutover_tests.rs"]
mod cutover_tests;

#[test]
fn xml_runtime_world_permanently_blocked_birth_is_abandoned_while_the_defeat_still_commits() {
    let (mut w, p, id) = fixture(
        r#"<Behavior childId="Piratess" prob="1" posJitter="0">MetamorphoseAtDeath</Behavior>"#,
    );
    frame(&mut w, 0);
    let void = *w.content.void_grounds.iter().next().unwrap();
    Arc::make_mut(&mut w.map).cells[64 * 128 + 64].ground = void;
    let (hp, exp) = (w.entities[&id].hp, w.players[&p].character.exp);
    let log = std::path::PathBuf::from(std::env::var("QA_RESULTS_DIR").unwrap())
        .join(format!("blocked-birth-{}.log", w.xml.instance));
    let errors = tracing::subscriber::set_default(
        tracing_subscriber::fmt()
            .with_ansi(false)
            .with_max_level(tracing::Level::ERROR)
            .with_writer(std::sync::Mutex::new(std::fs::File::create(&log).unwrap()))
            .finish(),
    );
    assert_eq!(w.damage_enemy(p, id, 100000, true, &[], None).unwrap(), hp);
    let mut now = 0;
    while now < terminal::PREFLIGHT_RETRY_MS {
        now += 1000;
        frame(&mut w, now);
        assert!(w.entities.contains_key(&id) && w.xml.rewards.is_empty());
        assert_eq!(w.xml.terminal_capacity.load(Ordering::Acquire), 1);
    }
    for _ in 0..4 {
        now += 1000;
        frame(&mut w, now);
    }
    drop(errors);
    // A bounded, expected retry logs one ERROR when it sticks and one when the births are abandoned.
    let logged = std::fs::read_to_string(&log).unwrap();
    assert_eq!(logged.lines().count(), 2, "{logged}");
    assert!(
        logged.contains("remains pending")
            && logged.contains("abandon only impossible physical effects")
    );
    assert!(!w.entities.contains_key(&id));
    assert_eq!(w.xml.terminal_capacity.load(Ordering::Acquire), 0);
    assert!(w.xml.rewards[&id].replacements.is_empty());
    assert!(!w.entities.values().any(|e| e.kind == 0x601));
    assert!(
        w.players[&p].character.exp > exp,
        "XP must commit despite the abandoned birth"
    );
    let db = rusqlite::Connection::open(
        std::path::PathBuf::from(std::env::var("QA_RESULTS_DIR").unwrap())
            .join(format!("world-terminal-{}.db", w.xml.instance)),
    )
    .unwrap();
    let (status, payload): (i32, String) = db
        .query_row(
            "SELECT status,payload FROM world_events WHERE kind='terminal'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    let payload: serde_json::Value = serde_json::from_str(&payload).unwrap();
    let publication = &payload["payload"]["data"]["actions"]["publication"];
    assert!(status >= 1 && publication["births"].as_array().unwrap().is_empty());
    let abandoned = &publication["abandoned_births"][0];
    assert_eq!(abandoned["disposition"], "physical_effect_abandoned");
    assert!(
        abandoned["preflight"]
            .as_str()
            .unwrap()
            .contains("geometry blocked"),
        "{abandoned}"
    );
}

#[test]
fn xml_rewards_realm_cycle_credits_a_burst_preparation_form_once_after_failed_stages() {
    let content = Arc::new(crate::world::test_content());
    let mut d = Directory::test_with_seed(Path::new("data"), content.clone(), 85, 1344).unwrap();
    let w = d.worlds.get_mut(&-3).unwrap();
    let mut ch = Character::fresh(content.get(crate::content::WIZARD).unwrap()).unwrap();
    (ch.name, ch.id, ch.account, ch.level) = ("Burst".into(), 1, 1, 20);
    let p = w.enter(ch, None).unwrap();
    w.players.get_mut(&p).unwrap().lab_conditions =
        crate::condition::bit(crate::condition::INVINCIBLE);
    // Ent Ancient's preparation form can die before its XML program metamorphoses it (actors.xml 0x91f).
    let prep = w
        .actors()
        .find(|e| e.kind == 0x91f && *e.quest == Some(e.id))
        .unwrap()
        .id;
    w.players.get_mut(&p).unwrap().pos = w.entities[&prep].pos;
    w.entities.get_mut(&prep).unwrap().hp = 1;
    w.xml.fault = Some(Fault::TerminalBefore);
    let _ = w.damage_enemy(p, prep, 1000, true, &[], None);
    assert!(
        w.critical_deaths.is_empty(),
        "a failed stage must not credit"
    );
    w.xml.fault = Some(Fault::TerminalAfter);
    let mut now = w.now;
    for _ in 0..20 {
        now += 50;
        frame(w, now);
    }
    assert!(!w.entities.contains_key(&prep));
    // Retries and repeated publication after the claim credit the quest line exactly once.
    assert_eq!(w.critical_deaths, vec![prep]);
}

#[test]
fn xml_rewards_realm_cycle_credit_rule_follows_the_last_quest_carrier() {
    use crate::endgame::critical_credit;
    let content = crate::world::test_content();
    let game = content.game.as_ref().unwrap();
    let production = Runtime::new(&content, None).unwrap();
    let carriers = production.quest_carriers;
    // Structural: roots, final forms and heroes plus Phoenix Lord's MetamorphoseAtDeath Egg; every one is announceable.
    assert_eq!(
        carriers
            .iter()
            .map(|k| format!("{k:#x}"))
            .collect::<Vec<_>>(),
        [
            "0x668", "0x675", "0x678", "0x67d", "0x6c1", "0x6c2", "0x91b", "0x91c", "0x91f",
            "0x920", "0x928", "0x92d", "0xd50", "0xd54", "0xd56", "0xd59", "0xd5f", "0xd61",
            "0xe37"
        ]
    );
    assert!(
        carriers
            .iter()
            .all(|k| super::super::realm_voice::family_name(*k).is_some())
    );
    let root = Some(7);
    let credit = |kind: u16, successor| {
        critical_credit(
            game.site_role(kind) == Some(crate::content::game::SiteRole::Hero),
            9,
            root,
            successor,
            carriers.contains(&kind),
        )
    };
    for prep in [0x91f, 0x91b, 0x928, 0x675] {
        assert_eq!(credit(prep, false), root, "burst {prep:#x}");
        assert_eq!(credit(prep, true), None, "metamorphosed {prep:#x}");
    }
    for actual in [0x920, 0x91c, 0x92d, 0x6c1] {
        assert_eq!(credit(actual, false), root, "{actual:#x}");
    }
    assert_eq!(
        credit(0x6c2, true),
        None,
        "a hatching egg passes the quest on"
    );
    assert_eq!(
        credit(0x6c2, false),
        root,
        "an abandoned Reborn leaves the egg last"
    );
    for event in [0xd50, 0xd54, 0xd56, 0xd59, 0xd5f, 0xd61, 0xe37, 0x678] {
        assert_eq!(credit(event, false), root, "event boss {event:#x}");
        assert_eq!(
            credit(event, true),
            None,
            "transformed event boss {event:#x}"
        );
    }
    assert_eq!(
        critical_credit(true, 9, None, false, true),
        Some(9),
        "heroes credit themselves"
    );
    // Followers carry the root's tag (landmark_ai.rs, realm_events.rs, ocean_setpiece.rs) but never end its line.
    for follower in [0x91d, 0x679, 0xd57, 0xd65] {
        assert_eq!(credit(follower, false), None, "follower {follower:#x}");
    }
}

#[path = "migration_regression_tests.rs"]
mod migration_regression_tests;

#[test]
fn xml_cutover_named_sites_add_change_remove_and_refuse_duplicate_ownership() {
    use crate::content::game::SitePhase;
    let data = Path::new("data");
    let source = crate::world::test_content();
    let manifest = "game/123.5.1/game.xml";
    let inputs: BTreeMap<_, _> = game::test_inputs::production(source.game.as_ref().unwrap(), data);
    let doc = std::str::from_utf8(&inputs[manifest]).unwrap();
    let parsed = roxmltree::Document::parse(doc).unwrap();
    let baseline = |name: &str| {
        &doc[parsed
            .descendants()
            .find(|node| node.has_tag_name("NamedSite") && node.attribute("defender") == Some(name))
            .unwrap()
            .range()]
    };
    for (kind, name, terrain, min, max, changed_terrain, changed_count) in [
        (0xdc2, "Great Coil Snake", 6, 2, 2, "MidForest", 3),
        (0x668, "Red Demon", 1, 3, 4, "HighPlains", 2),
        (0x67d, "Cyclops God", 4, 4, 6, "HighPlains", 1),
    ] {
        let row = baseline(name);
        let mut only = doc.to_owned();
        for node in parsed
            .descendants()
            .filter(|node| {
                node.has_tag_name("NamedSite") && node.attribute("defender") != Some(name)
            })
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
        {
            only.replace_range(node.range(), "");
        }
        let mut changed = format!(
            "    <NamedSite defender=\"{name}\" terrains=\"{changed_terrain}\" min=\"{changed_count}\" max=\"{changed_count}\" anchorsReplaceRoll=\"false\" role=\"{}\" provenance=\"authored\" origin=\"mutation test\" reason=\"change count and habitat\" />",
            if kind == 0xdc2 { "quest" } else { "hero" }
        );
        if let Some(children) = row.find('>').filter(|_| !row.ends_with("/>")) {
            changed.truncate(changed.len() - 3);
            changed.push_str(&row[children..]);
        }
        let compile = |text: String| {
            let mut inputs = inputs.clone();
            inputs.insert(manifest.into(), text.into_bytes());
            crate::content::game::compile_fixture_at(&source, inputs, data)
        };
        let duplicate = compile(only.replace(row, &format!("{row}\n{changed}"))).unwrap_err();
        assert!(format!("{duplicate:#}").contains("duplicate NamedSite"));
        for (tag, xml, habitat, expected) in [
            ("source", only.clone(), terrain, min..=max),
            ("removed", only.replace(row, ""), terrain, 0..=0),
            (
                "added",
                only.replace(row, &changed),
                if kind == 0xdc2 { 7 } else { 3 },
                changed_count..=changed_count,
            ),
        ] {
            let mut content = source.clone();
            content.game = Some(Arc::new(compile(xml).unwrap()));
            let mut map = flat_map(400);
            map.spawns = vec![(5.5, 5.5)];
            map.elevation = Some(vec![200; 400 * 400].into());
            for cell in &mut map.cells {
                cell.terrain = habitat;
            }
            let mut w =
                World::in_domain(Arc::new(content), Arc::new(map), WorldDomain::Realm).unwrap();
            w.populate_named_sites(&[], SitePhase::Landmarks).unwrap();
            let sites: Vec<_> = w.entities.values().filter(|e| e.kind == kind).collect();
            assert!(
                expected.contains(&sites.len()),
                "{name} {tag}: {} sites",
                sites.len()
            );
            assert!(
                sites
                    .iter()
                    .all(|e| w.map.cell(e.pos.x, e.pos.y).unwrap().terrain == habitat)
            );
            if kind != 0xdc2 {
                assert_eq!(w.hero_identities().len(), sites.len());
            }
        }
    }
}

#[test]
fn xml_cutover_scripted_clear_portal_add_change_remove_never_selects_a_fallback() {
    let data = Path::new("data");
    let source = crate::world::test_content();
    let actors = "game/123.5.1/actors.xml";
    let inputs: BTreeMap<_, _> = game::test_inputs::production(source.game.as_ref().unwrap(), data);
    let text = std::str::from_utf8(&inputs[actors]).unwrap();
    let start = text.find("<Object type=\"0x0927\"").unwrap();
    let after = start + text[start..].find("</Object>").unwrap();
    let row = r#"<Behavior prob="1" childId="Realm Portal">MetamorphoseAtDeath</Behavior>"#;
    assert_eq!(text[start..after].matches(row).count(), 1);
    let changed = row.replace("Realm Portal", "Portal to Nexus");
    for (rule, expected) in [(row, vec![0x704]), ("", vec![]), (&changed, vec![0x71d])] {
        let mut inputs = inputs.clone();
        inputs.insert(
            actors.into(),
            format!(
                "{}{}{}",
                &text[..start],
                text[start..after].replace(row, rule),
                &text[after..]
            )
            .into_bytes(),
        );
        let mut content = source.clone();
        content.game = Some(Arc::new(
            crate::content::game::compile_fixture_at(&content, inputs, data).unwrap(),
        ));
        let mut w = flat_nonrealm(Arc::new(content));
        w.xml.template = Some("pirate-cave".into());
        w.encounter = true;
        let boss = spawn(&mut w, 0x927, 64.5, 64.5);
        frame(&mut w, 0);
        w.xml_defeated(boss, TerminalContext::NonPlayer).unwrap();
        drain_events(&mut w).unwrap();
        assert!(w.cleared);
        assert_eq!(
            w.entities
                .values()
                .filter(|e| e.published && w.xml_is_portal(e.kind))
                .map(|e| e.kind)
                .collect::<Vec<_>>(),
            expected
        );
    }
}

#[test]
fn xml_cutover_generated_locker_uses_only_the_authored_clear_exit() {
    let data = Path::new("data");
    let source = crate::world::test_content();
    let game = "game/123.5.1/game.xml";
    let inputs: BTreeMap<_, _> = game::test_inputs::production(source.game.as_ref().unwrap(), data);
    let text = std::str::from_utf8(&inputs[game]).unwrap();
    let route = r#"boss="Davy Jones" route="realm-exit" portal="Realm Portal""#;
    assert_eq!(text.matches(route).count(), 1);
    for (replacement, expected) in [
        (route, vec![0x704]),
        (
            r#"boss="Davy Jones" route="realm-exit" portal="Glowing Realm Portal""#,
            vec![0x70e],
        ),
        (r#"boss="Davy Jones" route="none""#, vec![]),
    ] {
        let mut content = source.clone();
        if replacement != route {
            let mut inputs = inputs.clone();
            inputs.insert(game.into(), text.replace(route, replacement).into_bytes());
            content.game = Some(Arc::new(
                crate::content::game::compile_fixture_at(&content, inputs, data).unwrap(),
            ));
        }
        let mut directory = admission_tests::isolated_directory(Arc::new(content));
        let instance = directory
            .instance("davy-jones-locker", None, Some(DungeonReturn::Random))
            .unwrap();
        let w = directory.worlds.get_mut(&instance).unwrap();
        assert!(!w.entities.values().any(|e| e.kind == 0xe2d));
        let boss = w.entities.values().find(|e| e.kind == 0xe32).unwrap().id;
        assert!(w.xml_identity(boss).unwrap().parent.is_none());
        frame(w, 0);
        w.xml_defeated(boss, TerminalContext::NonPlayer).unwrap();
        drain_events(w).unwrap();
        assert!(w.cleared);
        assert_eq!(
            w.entities
                .values()
                .filter(|e| e.published && w.xml_is_portal(e.kind))
                .map(|e| e.kind)
                .collect::<Vec<_>>(),
            expected
        );
    }
}

fn mutate_cutover_manifest(
    element: &str,
    key: (&str, &str),
    edit: impl FnOnce(&str) -> String,
) -> Result<Content> {
    let data = Path::new("data");
    let mut content = crate::world::test_content();
    let mut inputs: BTreeMap<_, _> =
        game::test_inputs::production(content.game.as_ref().unwrap(), data);
    let manifest = inputs.get_mut("game/123.5.1/game.xml").unwrap();
    let xml = std::str::from_utf8(manifest).unwrap();
    let doc = roxmltree::Document::parse(xml).unwrap();
    let nodes = doc
        .descendants()
        .filter(|n| n.has_tag_name(element) && n.attribute(key.0) == Some(key.1))
        .collect::<Vec<_>>();
    assert_eq!(nodes.len(), 1, "{element} {key:?}");
    let range = nodes[0].range();
    let mut changed = xml.to_owned();
    changed.replace_range(range.clone(), &edit(&xml[range]));
    *manifest = changed.into_bytes();
    content.game = Some(Arc::new(game::compile_fixture_at(&content, inputs, data)?));
    Ok(content)
}

#[test]
fn xml_cutover_directory_constructs_and_enters_linked_world_map() {
    let fixed = Arc::new(crate::world::test_content());
    let source = Directory::fixture(fixed, 1, 12351, Some(12351)).unwrap();
    let content = mutate_cutover_manifest("World", ("id", "tutorial"), |row| {
        row.replace("map=\"tutorial\"", "map=\"kitchen\"")
    })
    .unwrap();
    let mut changed = Directory::fixture(Arc::new(content), 1, 12351, Some(12351)).unwrap();
    let kitchen = changed.make("kitchen", "Kitchen", false, None).unwrap();
    let tutorial = changed.worlds.get_mut(&-1).unwrap();
    assert_ne!(tutorial.map.spawns, source.worlds[&-1].map.spawns);
    assert_eq!((tutorial.map.width, tutorial.map.height), (33, 145));
    assert_eq!(tutorial.map.spawns, kitchen.map.spawns);
    let character =
        Character::fresh(tutorial.content.get(crate::content::WIZARD).unwrap()).unwrap();
    let player = tutorial.enter(character, None).unwrap();
    let pos = tutorial.players[&player].pos;
    assert_eq!((pos.x, pos.y), kitchen.map.spawns[0]);
    assert!(tutorial.walkable(pos));

    let incompatible = mutate_cutover_manifest("World", ("id", "nexus"), |row| {
        row.replace("map=\"nexus\"", "map=\"vault\"")
    })
    .unwrap();
    let error = Directory::with_seed(Arc::new(incompatible), 1, 12351, 1)
        .err()
        .unwrap();
    let error = format!("{error:#}");
    assert!(error.contains("World nexus map=vault"), "{error}");
    assert!(
        error.contains("entrance packing has 0 positions"),
        "{error}"
    );
}

#[test]
fn xml_cutover_invalid_owned_key_route_does_not_reserve_or_spend() {
    let mut content = crate::world::test_content();
    let game = Arc::make_mut(content.game.as_mut().unwrap());
    let route = game.portal_routes.remove(&0x70c).unwrap();
    for bad in [
        None,
        Some(game::PortalDestination::Nexus),
        Some(game::PortalDestination::Instance("pirate-cave".into())),
    ] {
        let mut content = content.clone();
        if let Some(destination) = bad {
            let mut route = route.clone();
            route[0].destination = destination;
            Arc::make_mut(content.game.as_mut().unwrap())
                .portal_routes
                .insert(0x70c, route);
        }
        let mut world = flat_nonrealm(Arc::new(content));
        world.xml.template = Some("nexus".into());
        let mut ch = Character::fresh(world.content.get(crate::content::WIZARD).unwrap()).unwrap();
        ch.account = 1;
        ch.id = 1;
        ch.items[4] = 0x71f;
        let player = world.enter(ch, None).unwrap();
        let p = &world.players[&player];
        let (items, revision) = (p.character.items.clone(), p.character.revision);
        let (ids, entities) = (world.ids.load(Ordering::Relaxed), world.entities.len());
        let error = world
            .use_item(
                player,
                Slot {
                    object: player,
                    slot: 4,
                    kind: 0x71f,
                },
                p.pos,
            )
            .err()
            .unwrap();
        let error = format!("{error:#}");
        assert!(
            error.contains("owned portal preflight before inventory reservation")
                && error.contains("kind=0x70c"),
            "{error}"
        );
        let p = &world.players[&player];
        assert!(!p.pending);
        assert_eq!(
            (p.character.items.clone(), p.character.revision),
            (items, revision)
        );
        assert_eq!(
            (world.ids.load(Ordering::Relaxed), world.entities.len()),
            (ids, entities)
        );
    }
}

#[test]
fn xml_cutover_linked_portal_routes_govern_publication_transfer_and_earned_exit() {
    use game::PortalDestination;
    let mutate = |edit: fn(&str) -> String| {
        mutate_cutover_manifest("PortalRoute", ("from", "snake-pit"), edit)
    };
    let duplicate = mutate(|row| format!("{row}{row}")).err().unwrap();
    assert!(
        format!("{duplicate:#}")
            .contains("duplicate PortalRoute ownership kind=0x703 from=Some(\"snake-pit\")")
    );
    let bad_clear = mutate_cutover_manifest("PortalRoute", ("object", "Realm Portal"), |row| {
        row.replace("origin-realm", "random-realm")
    })
    .err()
    .unwrap();
    assert!(format!("{bad_clear:#}").contains("does not return to the originating Realm"));
    let no_exit =
        mutate_cutover_manifest("PortalRoute", ("object", "Realm Portal"), |_| String::new())
            .err()
            .unwrap();
    assert!(format!("{no_exit:#}").contains("no linked portal route kind=0x704"));

    for (element, attr, id, context, kind) in [
        (
            "PortalRoute",
            "from",
            "snake-pit",
            "World snake-pit map=snake-pit: physical portal",
            "0x703",
        ),
        (
            "Instance",
            "entrance",
            "Glowing Portal",
            "owned key 0x71f",
            "0x70c",
        ),
        (
            "PortalRoute",
            "object",
            "Glowing Realm Portal",
            "World sprite-world map=sprite-world: portal birth",
            "0x70e",
        ),
    ] {
        let error = mutate_cutover_manifest(element, (attr, id), |_| String::new())
            .err()
            .unwrap();
        let error = format!("{error:#}");
        assert!(
            error.contains(context)
                && error.contains(&format!("no linked portal route kind={kind}")),
            "{error}"
        );
    }

    for (content, expected) in [
        (
            crate::world::test_content(),
            (PortalDestination::OriginRealm, true, -3),
        ),
        (
            mutate(|row| row.replace("earnedExit=\"true\"", "earnedExit=\"false\"")).unwrap(),
            (PortalDestination::OriginRealm, false, -3),
        ),
        (
            mutate(|row| {
                row.replace("origin-realm", "nexus")
                    .replace("earnedExit=\"true\"", "earnedExit=\"false\"")
            })
            .unwrap(),
            (PortalDestination::Nexus, false, -2),
        ),
    ] {
        let content = Arc::new(content);
        let mut d = admission_tests::isolated_directory(content.clone());
        for id in [-2, -3] {
            let w = flat_nonrealm(content.clone());
            d.worlds.insert(id, w);
        }
        d.endgame.realms.insert(
            -3,
            endgame::RealmRecord::new(
                crate::endgame::RealmCycle::new(BTreeSet::from([1])).unwrap(),
                0,
                Instant::now(),
            ),
        );
        let source = d
            .instance("snake-pit", None, Some(DungeonReturn::Realm(-3)))
            .unwrap();
        let w = d.worlds.get_mut(&source).unwrap();
        // Display names are not route identity.
        w.name = "Renamed dungeon".into();
        let mut ch = Character::fresh(content.get(crate::content::WIZARD).unwrap()).unwrap();
        ch.account = 1;
        ch.id = 1;
        let player = w.enter(ch, None).unwrap();
        let pos = w.players[&player].pos;
        let (destination, earned_exit, target) = expected;
        let portal = w.ids.fetch_add(1, Ordering::Relaxed);
        w.xml_preflight_publication_kind_position(0x703, pos)
            .unwrap();
        w.xml_insert_publication(portal, 0x703, pos, "route mutation")
            .unwrap();
        assert_eq!(w.xml.portals[&portal].intent.destination, destination);
        assert_eq!(w.xml.portals[&portal].intent.earned_exit, earned_exit);
        let (account, character, actual) = d.destination(player, Some(portal)).unwrap();
        assert_eq!(actual, target);
        let (_, key, _) = d.issue(account, character, target).unwrap();
        d.bind_exit(player, Some(portal), &key).unwrap();
        assert_eq!(
            d.grants[&key].exit.as_ref().map(|e| e.dungeon),
            earned_exit.then_some(crate::dungeon::HistoricalDungeon::SnakePit)
        );
        if earned_exit {
            d.worlds.get_mut(&target).unwrap().name = "Renamed Realm".into();
            d.validate_exit_destination(&d.grants[&key]).unwrap();
            d.endgame.realms.remove(&target).unwrap();
            assert!(d.validate_exit_destination(&d.grants[&key]).is_err());
        }
        let w = d.worlds.get_mut(&source).unwrap();
        w.now = w.xml.portals[&portal].expires.unwrap();
        let error = d.destination(player, Some(portal)).unwrap_err();
        assert!(format!("{error:#}").contains("portal XML lifecycle expired"));
    }
}

#[test]
fn xml_cutover_lab_coil_policy_selects_room_order_and_count() {
    use crate::dungeon::lab::{BOSS, TESLA, generate};
    let seed = 17;
    let mutate = |edit: fn(&str) -> String| {
        mutate_cutover_manifest("CoilPlacement", ("order", "boss-first"), edit)
    };
    let coils = |map: &Map| {
        map.cells
            .iter()
            .enumerate()
            .filter(|(_, c)| c.object == Some(TESLA))
            .map(|(i, _)| i)
            .collect::<Vec<_>>()
    };
    let source = generate(&crate::world::test_content(), seed).unwrap();
    assert_eq!(coils(&source).len(), 10);
    let changed = mutate(|row| {
        row.replace("rooms=\"5\"", "rooms=\"1\"")
            .replace("perRoom=\"2\"", "perRoom=\"1\"")
    })
    .unwrap();
    let early = mutate(|row| {
        row.replace("rooms=\"5\"", "rooms=\"1\"")
            .replace("perRoom=\"2\"", "perRoom=\"1\"")
            .replace("boss-first", "entrance-first")
    })
    .unwrap();
    let late = generate(&changed, seed).unwrap();
    assert_eq!(coils(&late).len(), 1);
    assert_eq!(coils(&late), coils(&generate(&changed, seed).unwrap()));
    let early = generate(&early, seed).unwrap();
    assert_eq!(coils(&early).len(), 1);
    let boss = source
        .cells
        .iter()
        .position(|c| c.object == Some(BOSS))
        .unwrap();
    let distance = |i: usize| {
        let width = source.width as usize;
        (i % width).abs_diff(boss % width) + (i / width).abs_diff(boss / width)
    };
    assert!(distance(coils(&late)[0]) < distance(coils(&early)[0]));
    assert_eq!(source.spawns, early.spawns);
    for (edit, expected) in [
        (
            (|_: &str| String::new()) as fn(&str) -> String,
            "Lab generator requires exactly one CoilPlacement",
        ),
        (
            (|row| format!("{row}{row}")) as fn(&str) -> String,
            "duplicate CoilPlacement",
        ),
    ] {
        let error = mutate(edit).err().unwrap();
        assert!(format!("{error:#}").contains(expected), "{error:#}");
    }
}

#[test]
fn xml_cutover_wine_cellar_corner_births_depend_on_linked_placement_not_display_name() {
    let census = |content| {
        let mut d = admission_tests::isolated_directory(Arc::new(content));
        d.templates = Directory::linked_static_maps(&d.content).unwrap();
        let id = d
            .instance(crate::endgame::CELLAR, None, Some(DungeonReturn::Random))
            .unwrap();
        let world = &d.worlds[&id];
        let positions = |kind| {
            world
                .entities
                .values()
                .filter(|e| e.kind == kind)
                .map(|e| (e.pos.x.to_bits(), e.pos.y.to_bits()))
                .collect::<BTreeSet<_>>()
        };
        (world.name.clone(), positions(0xde5), positions(0x182e))
    };
    let mutate = |edit: fn(&str) -> String| {
        mutate_cutover_manifest("World", ("id", crate::endgame::CELLAR), edit).unwrap()
    };
    let (name, barrel, other) = census(crate::world::test_content());
    assert_eq!(name, "Wine Cellar");
    assert_eq!(barrel.len(), 38);
    assert!(other.is_empty());
    let renamed = census(mutate(|row| {
        row.replace("name=\"Wine Cellar\"", "name=\"Renamed Cellar\"")
    }));
    assert_eq!(
        renamed,
        ("Renamed Cellar".into(), barrel.clone(), BTreeSet::new())
    );
    let switched = census(mutate(|row| {
        row.replace("object=\"Wine Barrel\"", "object=\"GhostShip Barrel\"")
    }));
    assert_eq!(switched, (name, BTreeSet::new(), barrel));
}

#[test]
fn xml_cutover_hermit_island_birth_positions_and_ownership_follow_linked_stamp() {
    let census = |content: Content| {
        let mut map = flat_map(192);
        map.spawns = vec![(5.5, 5.5)];
        map.elevation = Some(vec![200; 192 * 192].into());
        for c in &mut map.cells {
            c.terrain = 6;
        }
        let mut world =
            World::in_domain(Arc::new(content), Arc::new(map), WorldDomain::Realm).unwrap();
        world.rng = rand::rngs::StdRng::seed_from_u64(19);
        let rule = world
            .xml_game()
            .realm_rules()
            .unwrap()
            .sites
            .iter()
            .find(|r| r.kind == 0xd61)
            .unwrap()
            .clone();
        world.allocate_site(&rule).unwrap();
        let root = world
            .entities
            .values()
            .find(|e| e.kind == rule.kind)
            .unwrap();
        let offsets = |kind, owned| {
            world
                .entities
                .values()
                .filter(|e| e.kind == kind)
                .map(|e| {
                    assert_eq!((*e.parent, *e.quest), (owned, owned));
                    (
                        (e.pos.x - root.pos.x + 15.5) as i32,
                        (e.pos.y - root.pos.y + 13.) as i32,
                    )
                })
                .collect::<BTreeSet<_>>()
        };
        (
            offsets(0xd65, Some(root.id)),
            offsets(0x17b, None),
            offsets(0xd64, Some(root.id)),
        )
    };
    let (spawners, pillars, tentacles) = census(crate::world::test_content());
    assert_eq!(
        spawners,
        BTreeSet::from([
            (13, 8),
            (20, 17),
            (16, 8),
            (18, 8),
            (10, 13),
            (21, 13),
            (11, 17),
            (15, 18)
        ])
    );
    assert_eq!(
        pillars,
        BTreeSet::from([(11, 7), (20, 7), (8, 13), (20, 19)])
    );
    assert!(tentacles.is_empty());
    let changed = mutate_cutover_manifest("NamedSite", ("defender", "Hermit God"), |row| {
        row.replace(
            "object=\"Hermit God Tentacle Spawner\" x=\"13\" y=\"8\"",
            "object=\"Hermit God Tentacle\" x=\"12\" y=\"8\"",
        )
        .replace(
            "object=\"Grey Pillar\" x=\"11\" y=\"7\"",
            "object=\"Grey Pillar\" x=\"12\" y=\"7\"",
        )
    })
    .unwrap();
    let (changed_spawners, changed_pillars, changed_tentacles) = census(changed);
    assert_eq!(
        changed_spawners,
        spawners
            .difference(&BTreeSet::from([(13, 8)]))
            .copied()
            .collect()
    );
    assert_eq!(
        changed_pillars,
        BTreeSet::from([(12, 7), (20, 7), (8, 13), (20, 19)])
    );
    assert_eq!(changed_tentacles, BTreeSet::from([(12, 8)]));
}

#[test]
fn xml_cutover_chamber_unlock_follows_linked_context_and_route_not_instance_name() {
    for (name, opened, destination) in [
        ("Oryx's Chamber", 0x700, "wine-cellar"),
        ("Renamed Chamber", 0x700, "wine-cellar"),
        ("Renamed Chamber", 0x718, "snake-pit"),
    ] {
        let content = mutate_cutover_manifest("World", ("id", crate::endgame::CHAMBER), |row| {
            let row = row.replace("name=\"Oryx's Chamber\"", &format!("name=\"{name}\""));
            if opened == 0x718 {
                row.replace("opened=\"Dungeon Portal\"", "opened=\"Snake Pit Portal\"")
            } else {
                row
            }
        })
        .unwrap();
        let mut d = admission_tests::isolated_directory(Arc::new(content));
        d.templates = Directory::linked_static_maps(&d.content).unwrap();
        let world = d
            .instance(crate::endgame::CHAMBER, None, Some(DungeonReturn::Random))
            .unwrap();
        let w = d.worlds.get_mut(&world).unwrap();
        assert_eq!(w.name, name);
        let mut ch = Character::fresh(w.content.get(crate::content::WIZARD).unwrap()).unwrap();
        ch.account = 1;
        ch.id = 1;
        ch.items[4] = 0x722;
        let player = w.enter(ch, None).unwrap();
        let pos = w.players[&player].pos;
        let locked = w.spawn_clear_portal(0x721, pos, true).unwrap();
        let slot = Slot {
            object: player,
            slot: 4,
            kind: 0x722,
        };
        let before = w.use_item(player, slot, pos).err().unwrap();
        assert!(before.to_string().contains("requires a defeated Oryx"));
        assert!(!w.entities[&locked].reserved);
        assert!(!w.players[&player].pending);
        w.cleared = true;
        let plan = w.use_item(player, slot, pos).unwrap().unwrap();
        let portal = plan.unlock.unwrap();
        assert_eq!(portal.locked, Some(locked));
        assert_eq!(w.entities[&portal.opened].kind, opened);
        assert!(!w.entities[&portal.opened].published);
        assert_eq!(w.players[&player].character.items[4], 0x722);
        d.stage_unlock(plan).unwrap();
        let target = d.portals[&portal.opened].destination.unwrap();
        assert_eq!(d.worlds[&target].xml.template.as_deref(), Some(destination));
    }
}

#[test]
fn arriving_player_is_hidden_from_xml_hostile_targeting_for_three_seconds() {
    let (mut w, player, id) = fixture("<Behavior>RemainVisible</Behavior>");
    let ch = w.players.remove(&player).unwrap().character;
    let player = w
        .enter_at(
            ch,
            None,
            Some(Pos { x: 65.5, y: 64.5 }),
            AccountPresentation {
                guild: None,
                credits: 0,
                fame: 0,
                best_fame: BTreeMap::new(),
            },
            false,
        )
        .unwrap();
    let scope = scope(&w, id);
    w.now = 2999;
    assert!(w.xml_hostile_targets(scope, 5.).unwrap().is_empty());
    w.now = 3000;
    assert_eq!(w.xml_hostile_targets(scope, 5.).unwrap()[0].0, player);
}

#[test]
fn a_leashed_grandchild_returns_toward_its_own_ancestor_without_players() {
    let (mut w, player, root) = fixture_program(
        "<Behavior>RemainVisible</Behavior>",
        r#"<Behavior protecteeId="Pirate" acquireRange="6" protectionRange="10" reprotectRange="5" speed="1">Protect</Behavior>"#,
        LISTS,
    );
    let parent = spawn(&mut w, 0x601, 66.5, 64.5);
    w.xml_own(parent, Some(root), Some(root as u64)).unwrap();
    let child = spawn(&mut w, 0x601, 68.5, 64.5);
    w.xml_own(child, Some(parent), Some(root as u64)).unwrap();
    frame(&mut w, 50);
    w.position_entity(child, Pos { x: 88.5, y: 64.5 }).unwrap();
    w.players.get_mut(&player).unwrap().paused = true;
    for at in (100..=30_000).step_by(50) {
        w.now = at;
        w.xml_step().unwrap();
    }
    assert!(w.entities[&child].pos.distance(w.entities[&root].pos) <= 10.);
}
