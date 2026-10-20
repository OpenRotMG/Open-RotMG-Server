use crate::{
    content::Content,
    wire::{Packet, Reader, Writer},
    world::{Character, Room},
};

// Authored structural fixture with a few factual July scalars; not extracted XML/assets.
const OBJECTS: &str = r#"<Objects>
<Object type="782" id="Wizard"><Class>Player</Class></Object>
<Object type="1536" id="Pirate"><Class>Character</Class><Projectile id="0"><ObjectId>Blade</ObjectId><Damage>5</Damage><Speed>60</Speed><LifetimeMS>600</LifetimeMS></Projectile></Object>
<Object type="1280" id="Loot Bag"><Class>Container</Class></Object>
<Object type="2594" id="Health Potion"><Class>Equipment</Class><Activate stat="1" amount="1000">IncrementStat</Activate></Object>
<Object type="2711" id="Energy Staff"><Class>Equipment</Class></Object>
<Object type="42" id="Blade"><Class>Projectile</Class></Object>
</Objects>"#;

#[test]
fn historical_contracts_content_ids_references_and_required_fields() {
    let c = Content::parse(OBJECTS).unwrap();
    assert_eq!(c.get(1536).unwrap().arena_projectile(0).unwrap().speed, 6.0);
    assert_eq!(
        c.get(1536).unwrap().arena_projectile(0).unwrap().lifetime,
        0.6
    );
    assert!(c.get(32767).is_err());
    assert!(c.named("Nexus").is_err());
    for (from, to, error) in [
        ("type=\"42\"", "type=\"782\"", "duplicate object type"),
        ("id=\"Blade\"", "id=\"Wizard\"", "duplicate object name"),
        (
            "type=\"42\"",
            "type=\"42,43\"",
            "one nonnegative object type",
        ),
        ("type=\"42\"", "type=\"-1\"", "one nonnegative object type"),
        (
            "<Class>Projectile</Class>",
            "<Class>LaterClass</Class>",
            "unsupported object class",
        ),
        (
            "<ObjectId>Blade</ObjectId>",
            "<ObjectId>Absent</ObjectId>",
            "missing Absent",
        ),
        (
            "<Damage>5</Damage>",
            "<Damage>-1</Damage>",
            "invalid projectile damage",
        ),
        (
            "<Speed>60</Speed>",
            "<Speed>NaN</Speed>",
            "invalid projectile motion",
        ),
        (
            "<LifetimeMS>600</LifetimeMS>",
            "<LifetimeMS>0</LifetimeMS>",
            "invalid projectile motion",
        ),
        (
            "<LifetimeMS>600</LifetimeMS>",
            "<LifetimeMS>600</LifetimeMS><Amplitude>NaN</Amplitude>",
            "invalid projectile motion",
        ),
        ("<Class>Container</Class>", "", "missing Class"),
        ("<Objects>", "<LaterObjects>", ""),
    ] {
        let result = Content::parse(&OBJECTS.replace(from, to));
        let e = result.err().expect("malformed content accepted");
        assert!(format!("{e:#}").contains(error), "{from}: {e:#}");
    }
}

#[test]
fn historical_contracts_no_first_projectile_or_activation_fallback() {
    let second = "<Projectile id=\"1\"><ObjectId>Blade</ObjectId><Damage>80</Damage><Speed>100</Speed><LifetimeMS>2000</LifetimeMS><ConditionEffect duration=\"10\">Slowed</ConditionEffect></Projectile>";
    let xml = OBJECTS.replace("</Projectile>", &format!("</Projectile>{second}"));
    let c = Content::parse(&xml).unwrap();
    let pirate = c.named("Pirate").unwrap();
    assert_eq!(pirate.projectiles.len(), 2);
    assert_eq!(pirate.projectiles[1].1.min, 80);
    assert!(
        pirate
            .arena_projectile(1)
            .unwrap_err()
            .to_string()
            .contains("ConditionEffect")
    );
    assert!(pirate.arena_projectile(2).is_err());
    let duplicate = Content::parse(&xml.replace("id=\"1\"", "id=\"0\"")).unwrap();
    assert!(
        duplicate
            .named("Pirate")
            .unwrap()
            .arena_projectile(0)
            .unwrap_err()
            .to_string()
            .contains("ambiguous duplicate")
    );
    let xml = OBJECTS.replace("</Activate>", "</Activate><Activate>Teleport</Activate>");
    let c = Content::parse(&xml).unwrap();
    let potion = c.named("Health Potion").unwrap();
    assert_eq!(potion.activations.len(), 2);
    assert!(potion.single_activation().is_err());
    assert!(Content::parse(&OBJECTS.replace("IncrementStat", "LaterAbility")).is_err());
    assert!(Content::parse(&OBJECTS.replace("stat=\"1\"", "")).is_err());
}

#[test]
fn historical_contracts_room_move_shoot_and_ping_reply_boundaries() {
    let c = crate::world::tests::content();
    let character = Character::new(&c, 0x30e, "Phase".into(), 1).unwrap();
    let mut room = Room::new(42, 0, 2);
    let packets = room.attach(crate::world::PLAYER, character, &c).unwrap();
    let tick = packets.iter().find(|p| p.id == 13).unwrap();
    let mut r = Reader::new(&tick.body);
    assert_eq!(r.i16().unwrap(), 4);
    assert_eq!(r.i32().unwrap(), 1);
    assert_eq!((r.f32().unwrap(), r.f32().unwrap()), (16.5, 16.5));
    let count = r.i16().unwrap();
    let stats: std::collections::BTreeMap<_, _> = (0..count)
        .map(|_| (r.u8().unwrap(), r.i32().unwrap()))
        .collect();
    assert_eq!(stats, room.character(crate::world::PLAYER).stats);
    assert_eq!(stats[&5], 50); // Current arena threshold, not a recovered XP curve.
    // Synthetic plaintext fields in original Move/Shoot order, not a capture.
    let move_body = [0x41, 0x84, 0, 0, 0x41, 0x84, 0, 0]; // 16.5, 16.5
    room.handle(
        crate::world::PLAYER,
        Packet {
            id: 5,
            body: move_body.to_vec(),
        },
        &c,
    )
    .unwrap();
    for invalid in [
        vec![0; 7],
        vec![0; 9],
        [f32::NAN.to_be_bytes(), 16.5f32.to_be_bytes()].concat(),
    ] {
        assert!(
            room.handle(
                crate::world::PLAYER,
                Packet {
                    id: 5,
                    body: invalid
                },
                &c
            )
            .is_err()
        );
    }
    let before = serde_json::to_string(&room.character(crate::world::PLAYER)).unwrap();
    room.handle(
        crate::world::PLAYER,
        Packet {
            id: 23,
            body: vec![0, 0, 0, 42],
        },
        &c,
    )
    .unwrap();
    assert_eq!(
        serde_json::to_string(&room.character(crate::world::PLAYER)).unwrap(),
        before
    );
    assert!(
        room.handle(
            crate::world::PLAYER,
            Packet {
                id: 21,
                body: vec![0, 0, 0, 42]
            },
            &c
        )
        .is_err()
    );
    let mut shoot = Writer::new();
    shoot
        .u8(1)
        .i32(1)
        .u8(41)
        .i16(0xa97)
        .f32(16.8)
        .f32(16.5)
        .f32(0.0)
        .i16(32767);
    room.handle(
        crate::world::PLAYER,
        Packet {
            id: 8,
            body: shoot.0.clone(),
        },
        &c,
    )
    .unwrap();
    assert!(
        room.handle(
            crate::world::PLAYER,
            Packet {
                id: 8,
                body: shoot.0
            },
            &c
        )
        .is_err()
    );
}

#[test]
#[ignore = "requires private qualified assets: BUILD100_CLIENT_DIR; run explicitly with --include-ignored"]
fn historical_contracts_qualified_july_inventory_and_seven_classes() {
    use sha2::{Digest, Sha256};
    let path = std::path::PathBuf::from(
        std::env::var("BUILD100_CLIENT_DIR").expect("BUILD100_CLIENT_DIR required"),
    );
    let hash = format!(
        "{:x}",
        Sha256::digest(std::fs::read(path.join("client.swf")).unwrap())
    );
    assert_eq!(
        hash,
        "ec031064b61d22e860de802ae1967ef1bb193b2c35d297ca1d25b1a96f76684e"
    );
    let c = Content::load(&path).unwrap();
    assert_eq!(c.objects.len(), 528);
    assert_eq!(
        c.objects
            .values()
            .map(|o| o.projectiles.len())
            .sum::<usize>(),
        259
    );
    assert_eq!(
        c.objects
            .values()
            .map(|o| o.activations.len())
            .sum::<usize>(),
        52
    );
    assert_eq!(c.named("Oryx the Mad God").unwrap().projectiles.len(), 7);
    let cube = c.named("Cube God").unwrap();
    assert_eq!(
        cube.projectiles
            .iter()
            .map(|(id, p)| (*id, p.visual.as_str()))
            .collect::<Vec<_>>(),
        [(0, "Dark Blue Magic"), (0, "Blue Bolt")]
    );
    assert!(cube.arena_projectile(0).is_err());
    for (name, id, hp, ability, action) in [
        ("Rogue", 0x300, 150, 0xa56, "ConditionEffectSelf"),
        ("Archer", 0x307, 130, 0xa61, "Shoot"),
        ("Wizard", 0x30e, 100, 0xa2e, "BulletNova"),
        ("Priest", 0x310, 100, 0xa31, "HealNova"),
        ("Warrior", 0x31d, 200, 0xa66, "ConditionEffectSelf"),
        ("Knight", 0x31e, 200, 0xa08, "Shoot"),
        ("Paladin", 0x31f, 200, 0xa52, "ConditionEffectAura"),
    ] {
        let def = c.named(name).unwrap();
        assert_eq!(def.id, id);
        let character = Character::new(&c, id, "Phase".into(), 1).unwrap();
        assert_eq!(character.stats[&0], hp);
        assert_eq!(character.stats[&9], ability);
        assert_eq!(def.growth.len(), 8);
        assert_eq!(
            c.get(ability as i16)
                .unwrap()
                .single_activation()
                .unwrap()
                .name,
            action
        );
        assert_eq!(
            c.get(character.stats[&8] as i16).unwrap().slot_type,
            Some(def.slots[0])
        );
    }
    assert_eq!(c.named("Divine Heal Spell").unwrap().activations.len(), 2);
    assert_eq!(
        c.named("Cloak of the Planewalker")
            .unwrap()
            .activations
            .len(),
        2
    );
    let xml = std::fs::read_to_string(path.join("grounds.xml")).unwrap();
    let doc = roxmltree::Document::parse(&xml).unwrap();
    let grounds: Vec<_> = doc
        .root_element()
        .children()
        .filter(roxmltree::Node::is_element)
        .collect();
    assert_eq!(grounds.len(), 115);
    let ground = |id| {
        grounds
            .iter()
            .copied()
            .find(|n| n.attribute("id") == Some(id))
            .unwrap()
    };
    assert!(
        ground("Dark Water")
            .children()
            .any(|n| n.has_tag_name("NoWalk"))
    );
    assert!(ground("Water").children().any(|n| n.has_tag_name("Sink")));
    assert_eq!(
        ground("Lava")
            .children()
            .find(|n| n.has_tag_name("MinDamage"))
            .unwrap()
            .text(),
        Some("30")
    );
    assert_eq!(
        ground("Lava")
            .children()
            .find(|n| n.has_tag_name("MaxDamage"))
            .unwrap()
            .text(),
        Some("60")
    );
    assert!(!ground("Ice").children().any(|n| n.has_tag_name("Speed")));
    let xml = std::fs::read_to_string(path.join("objects.xml")).unwrap();
    let doc = roxmltree::Document::parse(&xml).unwrap();
    for (tag, count) in [("Behavior", 779), ("Loot", 723)] {
        assert_eq!(
            doc.descendants().filter(|n| n.has_tag_name(tag)).count(),
            count
        );
    }
    assert_eq!(
        doc.descendants()
            .filter(|n| n.has_tag_name("Behavior"))
            .map(|n| n.text().unwrap())
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        33
    );
    let object = |id| {
        doc.root_element()
            .children()
            .find(|n| n.attribute("id") == Some(id))
            .unwrap()
    };
    let pirate = object("Pirate");
    let key = pirate
        .children()
        .find(|n| n.has_tag_name("Loot") && n.text() == Some("Pirate Cave Key"))
        .unwrap();
    assert_eq!(key.attribute("prob"), Some("0.02"));
    let fire = object("Fire Spray Spell");
    assert_eq!(
        fire.children()
            .find(|n| n.has_tag_name("MpCost"))
            .unwrap()
            .text(),
        Some("0")
    );
    assert_eq!(
        fire.children()
            .find(|n| n.has_tag_name("MinDamage"))
            .unwrap()
            .text(),
        Some("300")
    );
    assert_eq!(
        c.named("Fire Spray Spell")
            .unwrap()
            .arena_projectile(0)
            .unwrap()
            .min,
        30
    );
}
