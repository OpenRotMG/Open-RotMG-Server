use super::*;
use crate::bots::tests::{object, view};

fn vault(c: &Content) -> PlayerView {
    let mut v = view(c);
    v.map.name = "Vault".into();
    v.character.items[4..].fill(-1);
    let mut chest = object(22, 0x504, 12.5, 11.5);
    chest.contents = vec![-1; 8];
    v.objects.push(chest);
    v
}

#[test]
fn community_resources_withdraw_missing_supplies_then_reassess_observed_stock() {
    let c = Content::load(std::path::Path::new("data")).unwrap();
    let mut v = vault(&c);
    v.objects[0].contents[0] = 0xa22;
    let d = bank(&v, &c, &Grid::new(&v), None).unwrap().unwrap();
    assert!(
        matches!(d.item, Some(Action::Swap { from, to }) if from.object == 22 && from.kind == 0xa22 && to.object == 1)
    );
    v.objects[0].contents[0] = -1;
    let d = bank(&v, &c, &Grid::new(&v), None).unwrap().unwrap();
    assert!(d.item.is_none());
    assert!(matches!(d.journey, Some(Journey::Nexus)));
}

#[test]
fn community_resources_full_backpack_reconsiders_potion_for_upgrade_and_item_for_potion() {
    let c = Content::load(std::path::Path::new("data")).unwrap();
    let mut v = vault(&c);
    v.character.items[4..].fill(0xa23);
    v.objects[0].contents[0] = 0xa22;
    let d = bank(&v, &c, &Grid::new(&v), None).unwrap().unwrap();
    assert!(
        matches!(d.item, Some(Action::Swap { from, to }) if from.object == 22 && from.kind == 0xa22 && to.object == 1)
    );
    v.objects[0].contents[0] = 0xa9a;
    let d = bank(&v, &c, &Grid::new(&v), None).unwrap().unwrap();
    assert!(
        matches!(d.item, Some(Action::Swap { from, to }) if from.kind == 0xa9a && to.kind == 0xa23)
    );
}

#[test]
fn community_resources_full_vault_keeps_better_carried_stock_without_discard() {
    let c = Content::load(std::path::Path::new("data")).unwrap();
    let mut v = vault(&c);
    v.character.items[0] = 0xa9a;
    v.character.items[4] = 0xa21;
    v.objects[0].contents.fill(0xa9a);
    let d = bank(&v, &c, &Grid::new(&v), None).unwrap().unwrap();
    assert!(d.item.is_none());
    assert!(matches!(d.journey, Some(Journey::Nexus)));
    assert_eq!(v.character.items[4], 0xa21);
}

#[test]
fn community_resources_cross_class_retention_has_a_future_purpose_not_starter_gear_farming() {
    let c = Content::load(std::path::Path::new("data")).unwrap();
    let mut v = view(&c);
    v.character.items[4..].fill(-1);
    let mut bag = object(22, 0x500, 12.5, 11.5);
    bag.contents = vec![0xa05];
    assert!(pickup(&v, &c, &bag, None).unwrap().is_none());
    assert!(
        matches!(pickup(&v, &c, &bag, Some(crate::content::PRIEST)).unwrap(),Some(Action::Swap {to,..}) if to.slot>=4)
    );
    v.character.items[4] = 0xa05;
    let mut mind = pursuits::Mind::new(v.session.account);
    mind.future_class = Some(crate::content::PRIEST);
    assert!(mind.assess(&v, &c).unwrap().is_none());
    assert_eq!(v.character.class, crate::content::WIZARD);
    for kind in c
        .get(crate::content::WIZARD)
        .unwrap()
        .equipment
        .iter()
        .filter(|k| **k >= 0)
    {
        assert!(
            !spare(&c, *kind).unwrap(),
            "replacement starter must not trigger banking"
        );
    }
}

#[test]
fn community_resources_unfamiliar_inspection_finishes_and_re_equips_after_need_changes() {
    let c = Content::load(std::path::Path::new("data")).unwrap();
    let mut v = view(&c);
    v.character.items[3] = 0xa28;
    v.character.items[4] = 0xa27;
    let mut mind = pursuits::Mind::new(v.session.account);
    assert!(mind.inspecting(&v, &c));
    assert!(mind.assess(&v, &c).unwrap().is_none());
    assert!(mind.assess(&v, &c).unwrap().is_none());
    assert!(!mind.inspecting(&v, &c));
    assert!(matches!(
        mind.assess(&v, &c).unwrap(),
        Some(Action::Swap { .. })
    ));
    v.character.items.swap(3, 4);
    v.character.mp = 10;
    mind.facts.push(pursuits::ItemFact {
        kind: 0xa28,
        assessments: 2,
    });
    assert!(matches!(
        mind.assess(&v, &c).unwrap(),
        Some(Action::Swap { .. })
    ));
}

#[test]
fn community_resources_familiarity_under_danger_changes_attended_projectile_not_action_guards() {
    let c = Content::load(std::path::Path::new("data")).unwrap();
    let mut v = view(&c);
    v.character.items[4] = 0xa9a;
    let mut mind = pursuits::Mind::new(v.session.account);
    mind.cognition.attention = 2;
    for (id, y) in [(0, 18.), (1, 11.5)] {
        v.projectiles.push(crate::world::view::VisibleProjectile {
            owner: 99,
            id,
            start: Pos { x: 14., y },
            position: Pos { x: 14., y },
            angle: std::f32::consts::PI,
            speed: 50.,
            lifetime_ms: 2000.,
            amplitude: 0.,
            frequency: 0.,
            passes_cover: false,
        });
    }
    let occupied = combat::Perception::attend(&v, &mind.cognition, mind.inspecting(&v, &c));
    assert!(combat::clearance(&v, v.position, &occupied) > 4.);
    mind.assess(&v, &c).unwrap();
    mind.assess(&v, &c).unwrap();
    let attended = combat::Perception::attend(&v, &mind.cognition, mind.inspecting(&v, &c));
    assert!(combat::clearance(&v, v.position, &attended) < 0.2);
}

#[tokio::test]
async fn community_resources_stale_vault_revision_and_stock_cannot_mint_holdings() {
    use crate::{
        content::{Cell, Map},
        store::{InventoryWrite, Store},
        world::{Output, World},
    };
    let c = Arc::new(Content::load(std::path::Path::new("data")).unwrap());
    let dir = std::env::temp_dir().join(format!("resources-stale-{}", rand::random::<u64>()));
    std::fs::create_dir(&dir).unwrap();
    let store = Store::open(&dir.join("game.db"), &c).unwrap();
    let account = store.list("resources-stale".into()).await.unwrap().id;
    let mut ch = store
        .create(account, c.get(crate::content::WIZARD).unwrap())
        .await
        .unwrap();
    ch.items[4..].fill(-1);
    let mut vault = store.vault(account).await.unwrap();
    vault.items[0] = 0xa22;
    ch.revision = store
        .exchange(
            vec![InventoryWrite {
                id: ch.id,
                account,
                revision: ch.revision,
                items: ch.items.clone(),
            }],
            Some(vault),
        )
        .await
        .unwrap()[0];
    let vault = store.vault(account).await.unwrap();
    let mut cells = vec![
        Cell {
            ground: 0x61,
            object: None,
            config: String::new(),
            terrain: 0,
            region: 0
        };
        32 * 32
    ];
    cells[11 * 32 + 12].region = 12;
    let mut world = World::new(
        c.clone(),
        Arc::new(Map {
            width: 32,
            height: 32,
            cells,
            spawns: vec![(11.5, 11.5)],
        }),
    )
    .unwrap();
    world.name = "Vault".into();
    world.owner = Some(account);
    world.load_vault(vault.clone()).unwrap();
    let (packets, _receiver) = tokio::sync::mpsc::channel(OUTPUT_CAPACITY);
    let (lost, _) = tokio::sync::watch::channel(false);
    let id = world
        .enter_autonomous(ch.clone(), Output { packets, lost })
        .unwrap();
    let chest = world
        .entities
        .values()
        .find(|e| e.kind == 0x504)
        .unwrap()
        .id;
    assert!(
        world
            .autonomous_action(
                id,
                Action::Swap {
                    from: Slot {
                        object: chest,
                        slot: 0,
                        kind: 0xa23
                    },
                    to: Slot {
                        object: id,
                        slot: 4,
                        kind: -1
                    }
                }
            )
            .is_err()
    );
    let plan = world
        .autonomous_action(
            id,
            Action::Swap {
                from: Slot {
                    object: chest,
                    slot: 0,
                    kind: 0xa22,
                },
                to: Slot {
                    object: id,
                    slot: 4,
                    kind: -1,
                },
            },
        )
        .unwrap()
        .unwrap();
    let mut changed = vault.clone();
    changed.items[0] = 0xa23;
    let revision = store
        .exchange(
            vec![InventoryWrite {
                id: ch.id,
                account,
                revision: ch.revision,
                items: ch.items.clone(),
            }],
            Some(changed),
        )
        .await
        .unwrap()[0];
    let result = store
        .exchange(
            vec![InventoryWrite {
                id: ch.id,
                account,
                revision,
                items: plan.items.clone(),
            }],
            plan.vault.clone(),
        )
        .await;
    assert!(result.is_err());
    assert!(
        world
            .finish_inventory(plan, Err(anyhow::anyhow!("offline stale-revision fixture")))
            .is_err()
    );
    assert!(
        store
            .load(account, ch.id)
            .await
            .unwrap()
            .items
            .iter()
            .all(|k| *k != 0xa22)
    );
    assert_eq!(store.vault(account).await.unwrap().items[0], 0xa23);
    println!("resource stale-revision evidence: {}", dir.display());
}

#[tokio::test]
async fn community_resources_death_keeps_account_interest_not_dead_stock_or_replacement_gear_income()
 {
    let c = Content::load(std::path::Path::new("data")).unwrap();
    let dir = std::env::temp_dir().join(format!("resources-death-{}", rand::random::<u64>()));
    std::fs::create_dir(&dir).unwrap();
    let store = crate::store::Store::open(&dir.join("game.db"), &c).unwrap();
    let account = store.list("resources-death".into()).await.unwrap().id;
    let mut mind = pursuits::Mind::new(account);
    mind.future_class = Some(crate::content::PRIEST);
    store.save_bot_mind(mind).await.unwrap();
    for _ in 0..2 {
        let mut ch = store
            .create(account, c.get(crate::content::WIZARD).unwrap())
            .await
            .unwrap();
        assert_eq!(ch.items, c.get(crate::content::WIZARD).unwrap().equipment);
        let mut v = view(&c);
        v.character.items = ch.items.clone();
        assert!(!stash(&v, &c).unwrap());
        ch.items[4] = 0xa9a;
        ch.revision = store
            .exchange(
                vec![crate::store::InventoryWrite {
                    id: ch.id,
                    account,
                    revision: ch.revision,
                    items: ch.items.clone(),
                }],
                None,
            )
            .await
            .unwrap()[0];
        ch.hp = 0;
        ch.alive = false;
        let dead = ch.id;
        store
            .death(ch, "resource death fixture".into())
            .await
            .unwrap();
        assert!(store.load(account, dead).await.is_err());
        assert!(
            store
                .vault(account)
                .await
                .unwrap()
                .items
                .iter()
                .all(|k| *k == -1)
        );
        assert_eq!(
            store.bot_mind(account).await.unwrap().unwrap().future_class,
            Some(crate::content::PRIEST)
        );
        assert!(
            store
                .create(account, c.get(crate::content::PRIEST).unwrap())
                .await
                .is_err()
        );
    }
    println!("resource permanent-death evidence: {}", dir.display());
}
