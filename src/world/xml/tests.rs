use super::*;
use crate::content::{Cell, game};

#[path = "journal_tests.rs"]
mod journal_tests;

fn authored(name: &str, id: u16, body: &str) -> String {
    format!(
        r#"<Object type="{id}" id="{name}" disposition="active" provenance="authored" reason="native World regression" origin="independent fixture">{body}</Object>"#
    )
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

pub(crate) fn drain_events(w: &mut World) -> Result<()> {
    if w.pending_terminal_events().is_empty() && w.delivery_events().is_empty() {
        return Ok(());
    }
    std::thread::scope(|scope| {
        scope.spawn(||->Result<()> {
        let runtime=tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        runtime.block_on(async {
            let path=std::env::temp_dir().join(format!("world-terminal-{}-{}.db",std::process::id(),w.xml.instance));
            let store=crate::store::Store::open(&path,&w.content)?;
            let db=rusqlite::Connection::open(&path)?;
            for p in w.players.values() {
                // The Store holds the last committed history; shots fired since are not yet saved.
                let ch=&crate::store::Character{lifetime:p.committed_lifetime.clone(),..p.character.clone()};
                db.execute("INSERT OR IGNORE INTO accounts(id,guid,name) VALUES(?1,?2,?3)",rusqlite::params![ch.account,format!("world-component-{}",ch.account),ch.name])?;
                db.execute("INSERT OR IGNORE INTO account_fame VALUES(?1,0,0)",[ch.account])?;
                db.execute("INSERT OR IGNORE INTO vaults(account,chest,items) VALUES(?1,0,'[-1,-1,-1,-1,-1,-1,-1,-1]')",[ch.account])?;
                db.execute("INSERT INTO characters(id,account,stats,items,revision,alive) SELECT ?1,?2,?3,?4,?5,?6 WHERE NOT EXISTS(SELECT 1 FROM characters WHERE id=?1)",rusqlite::params![ch.id,ch.account,serde_json::to_string(ch)?,serde_json::to_string(&ch.items)?,ch.revision,ch.alive])?;
            }
            drop(db);
            for event in w.pending_terminal_events() {
                let record=store.coordinate_terminal(event.clone()).await?;
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

fn fixture(body: &str) -> (World, i32, i32) {
    let mut content = crate::world::test_content();
    let mut rules = authored("Pirate", 0x600, body)
        + &authored("Piratess", 0x601, "<Behavior>RemainVisible</Behavior>");
    for (kind, service) in [(6, "projectile"), (0x504, "container"), (0x307, "player")] {
        let name = &content.objects[&kind].name;
        rules += &format!(
            r#"<Object type="{kind}" id="{name}" disposition="native" nativeService="{service}" provenance="authored" origin="selected native fixture service" reason="explicit bounded World fixture dependency"/>"#
        );
    }
    let inputs = crate::content::game::test_inputs::selected([
        (
            "game/123.5.1/game.xml",
            br#"<Game schema="1"><Include file="rules.xml"/><Root object="Pirate"/></Game>"#
                .to_vec(),
        ),
        (
            "game/123.5.1/rules.xml",
            format!("<Objects>{rules}</Objects>").into_bytes(),
        ),
    ]);
    content.game = Some(Arc::new(game::compile_fixture(&content, inputs).unwrap()));
    let mut w = flat_nonrealm(Arc::new(content));
    let mut ch = Character::fresh(w.content.get(0x307).unwrap()).unwrap();
    ch.account = 1;
    ch.id = 1;
    let player = w.enter(ch, None).unwrap();
    w.players.get_mut(&player).unwrap().pos = Pos { x: 65.5, y: 64.5 };
    let id = spawn(&mut w, 0x600, 64.5, 64.5);
    (w, player, id)
}

#[test]
fn xml_runtime_world_xml_only_cooldown_state_and_birth_mutations_change_observed_services() {
    fn candidate(name: &str, cooldown: &str, state: &str, children: usize) -> World {
        let (mut w, _, _) = fixture(&format!(
            r#"<Behavior childIds="Piratess" maxChildren="{children}" cooldown="10">MakeMinions</Behavior><Behavior type="auto" projectileId="0" cooldown="{cooldown}">Shoot</Behavior><State id="shield"><Behavior effect="Invulnerable">ConditionEffect</Behavior><Transition afterTime="{state}">open</Transition></State><State id="open"/>"#
        ));
        w.name = name.into();
        for at in [0, 50, 100, 150] {
            w.now = at;
            w.xml_step().unwrap();
        }
        w
    }
    let a = candidate("Fixture", "0.1", "0.1", 1);
    let renamed = candidate("Renamed Fixture", "0.1", "0.1", 1);
    let b = candidate("Fixture", "0.2", "0.2", 2);
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
    assert_eq!(renamed.bullets.len(), a.bullets.len());
    assert_eq!(renamed.entities.len(), a.entities.len());
    assert_eq!(root(&renamed), root(&a));
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
fn child_counts_follow_adoption_reparenting_and_removal() {
    let (mut w, _, first) = fixture("<Behavior>RemainVisible</Behavior>");
    let second = spawn(&mut w, 0x600, 60.5, 60.5);
    let child = spawn(&mut w, 0x601, 62.5, 62.5);
    w.set_parent(child, Some(first));
    assert_eq!(
        (w.xml_owned_count(first), w.xml_owned_count(second)),
        (1, 0)
    );
    w.set_parent(child, Some(second));
    assert_eq!(
        (w.xml_owned_count(first), w.xml_owned_count(second)),
        (0, 1)
    );
    w.remove_entity(child);
    assert_eq!(w.xml_owned_count(second), 0);
}
