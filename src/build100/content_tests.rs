use super::*;

// Authored structure and source-pinned scalar vectors, not extracted content.
const FIXTURE: &str = r#"<Objects>
<Object type="782" id="Wizard"><Class>Player</Class></Object>
<Object type="1536" id="Pirate"><Class>Character</Class></Object>
<Object type="1280" id="Loot Bag"><Class>Container</Class><Loot/></Object>
<Object type="2594" id="Health Potion"><Class>Equipment</Class></Object>
<Object type="2711" id="Energy Staff"><Class>Equipment</Class></Object>
<Object type="42" id="Blade"><Class>Projectile</Class></Object>
<Object type="50" id="Vector"><Class>Equipment</Class>FIELDS</Object>
</Objects>"#;
const PROJECTILE: &str = "<Projectile><ObjectId>Blade</ObjectId><Damage>5</Damage><Speed>100</Speed><LifetimeMS>1000</LifetimeMS>FEATURES</Projectile>";
fn parse(fields: &str) -> Result<Content> {
    Content::parse(&FIXTURE.replace("FIELDS", fields))
}
fn projectile(features: &str) -> String {
    PROJECTILE.replace("FEATURES", features)
}
fn fails(fields: &str, message: &str) {
    let error = parse(fields).err().expect("malformed vector accepted");
    assert!(format!("{error:#}").contains(message), "{error:#}");
    assert!(format!("{error:#}").contains("Vector"), "{error:#}");
}

#[test]
fn content_contracts_absence_zero_defaults_and_root_nested_separation() {
    let c = parse("").unwrap();
    let o = c.named("Vector").unwrap();
    assert_eq!(
        (o.root_damage, o.rate_of_fire, o.mp_cost),
        (None, None, None)
    );
    assert_eq!(
        (o.num_projectiles, o.arc_gap_radians, o.xp_multiplier),
        (1, std::f32::consts::PI / 16.0, 1.0)
    );
    let fields = format!(
        "<MinDamage>450</MinDamage><MaxDamage>500</MaxDamage><MpCost>0</MpCost><XpMult>0</XpMult><ActivateOnEquip stat=\"21\" amount=\"2\">IncrementStat</ActivateOnEquip>{}",
        projectile("<ActivateOnEquip stat=\"20\" amount=\"5\">IncrementStat</ActivateOnEquip>")
    );
    let c = parse(&fields).unwrap();
    let o = c.named("Vector").unwrap();
    assert_eq!(o.root_damage, Some((450, 500)));
    assert_eq!((o.mp_cost, o.xp_multiplier), (Some(0), 0.0));
    assert_eq!(o.bonuses, [(21, 2)]);
    assert_eq!(o.normal_projectile(0).unwrap().min, 5);
    assert!(o.arena_projectile(0).is_err());
}

#[test]
fn content_contracts_projectile_flags_conditions_and_strict_selectors() {
    for (name, condition) in [
        ("Armor Pierce", Condition::ArmorPierce),
        ("Slowed", Condition::Slowed),
    ] {
        let p = projectile(&format!(
            "<Wavy/><Continues/><ConditionEffect duration=\"0\">{name}</ConditionEffect>"
        ));
        let c = parse(&p).unwrap();
        let o = c.named("Vector").unwrap();
        let p = &o.projectiles[0].1;
        assert!(p.wavy && p.continues);
        assert_eq!(
            p.effects,
            [ConditionEffect {
                condition,
                duration_seconds: 0.0
            }]
        );
        assert_eq!(p.armor_piercing(), condition == Condition::ArmorPierce);
        assert!(o.arena_projectile(0).is_err());
        assert!(o.normal_projectile(0).is_err());
    }
    let c = parse(&projectile("<FutureMotion/>")).unwrap();
    assert!(c.named("Vector").unwrap().normal_projectile(0).is_err());
    let c = parse(&projectile("").repeat(2)).unwrap();
    assert_eq!(c.named("Vector").unwrap().projectiles.len(), 2);
    assert!(c.named("Vector").unwrap().normal_projectile(0).is_err());
    assert!(c.named("Vector").unwrap().normal_projectile(1).is_err());
}

#[test]
fn content_contracts_compound_actions_and_independent_loot_entries() {
    let c = parse("<Activate range=\"15\" healing=\"200\">HealNova</Activate><Activate range=\"15\" healing=\"200\">RemoveNegativeConditions</Activate><Loot prob=\"0.25\">Health Potion</Loot><Loot prob=\"0.25\">Health Potion</Loot>").unwrap();
    let o = c.named("Vector").unwrap();
    assert_eq!(
        o.activations
            .iter()
            .map(|a| a.action().unwrap())
            .collect::<Vec<_>>(),
        [
            Action::HealNova {
                range: 15.0,
                healing: 200
            },
            Action::RemoveNegativeConditions {
                range: 15.0,
                healing: 200
            }
        ]
    );
    assert!(o.single_activation().is_err());
    assert_eq!(o.loot.len(), 2);
    assert_eq!(o.loot[0], o.loot[1]);
    assert_eq!(c.named("Loot Bag").unwrap().loot, [Loot::EmptyContainer]);
    let c = parse("<ConditionEffect duration=\"5\">Invisible</ConditionEffect><Activate>Teleport</Activate><Activate>ConditionEffectSelf</Activate>").unwrap();
    assert_eq!(
        c.named("Vector")
            .unwrap()
            .activations
            .iter()
            .map(|a| a.action().unwrap())
            .collect::<Vec<_>>(),
        [Action::Teleport, Action::ConditionEffectSelf]
    );
    assert_eq!(Condition::ArmorPierce as u8, 17);
    assert_eq!(Condition::Berserk as u8, 20);
    assert_eq!(Condition::Paused as u8, 21);
}

#[test]
fn content_contracts_reject_malformed_missing_conflicting_and_unknown_operands() {
    for (fields, message) in [
        ("<MinDamage>1</MinDamage>", "damage range"),
        (
            "<MinDamage>2</MinDamage><MaxDamage>1</MaxDamage>",
            "damage range",
        ),
        (
            "<MinDamage>1</MinDamage><MinDamage>1</MinDamage><MaxDamage>2</MaxDamage>",
            "duplicate MinDamage",
        ),
        ("<RateOfFire>NaN</RateOfFire>", "RateOfFire"),
        ("<RateOfFire>0</RateOfFire>", "RateOfFire"),
        ("<MpCost>-1</MpCost>", "MpCost"),
        ("<MpCost/>", "empty MpCost"),
        ("<MpCost>1<Future/></MpCost>", "nested MpCost"),
        ("<MpCost>1</MpCost><MpCost>2</MpCost>", "duplicate MpCost"),
        ("<NumProjectiles>0</NumProjectiles>", "NumProjectiles"),
        ("<NumProjectiles>1.5</NumProjectiles>", "NumProjectiles"),
        ("<ArcGap>inf</ArcGap>", "ArcGap"),
        ("<XpMult>NaN</XpMult>", "XpMult"),
        ("<XpMult>-1</XpMult>", "XpMult"),
        (
            "<ConditionEffect duration=\"3\">Speedy</ConditionEffect>",
            "unknown July condition",
        ),
        (
            "<ConditionEffect>Quiet</ConditionEffect>",
            "missing duration",
        ),
        (
            "<ConditionEffect duration=\"NaN\">Quiet</ConditionEffect>",
            "condition duration",
        ),
        (
            "<ConditionEffect duration=\"-1\">Quiet</ConditionEffect>",
            "condition duration",
        ),
        (
            "<Activate>ConditionEffectSelf</Activate>",
            "missing root ConditionEffect",
        ),
        (
            "<Activate>Shoot</Activate>",
            "missing root damage/projectile",
        ),
        (
            "<Activate>FutureAbility</Activate>",
            "unsupported build100 activation",
        ),
        (
            "<Activate stat=\"99\" amount=\"1\">IncrementStat</Activate>",
            "unknown stat ID",
        ),
        (
            "<Activate stat=\"1\">IncrementStat</Activate>",
            "missing amount",
        ),
        (
            "<Activate stat=\"1\" amount=\"0\">IncrementStat</Activate>",
            "invalid amount",
        ),
        (
            "<Activate range=\"inf\" healing=\"1\">HealNova</Activate>",
            "invalid range",
        ),
        (
            "<Activate range=\"1\">HealNova</Activate>",
            "missing healing",
        ),
        (
            "<Activate range=\"1\">ConditionEffectAura</Activate>",
            "missing root ConditionEffect",
        ),
        ("<Activate>OpenPortal</Activate>", "missing dungeonName"),
        (
            "<Activate range=\"1\">Teleport</Activate>",
            "unsupported activation attribute",
        ),
        (
            "<ActivateOnEquip stat=\"1\" amount=\"5\">IncrementStat</ActivateOnEquip>",
            "unknown stat ID",
        ),
        (
            "<ActivateOnEquip stat=\"21\">IncrementStat</ActivateOnEquip>",
            "missing amount",
        ),
        (
            "<ActivateOnEquip>FutureAction</ActivateOnEquip>",
            "unsupported ActivateOnEquip",
        ),
        ("<Loot/>", "invalid empty Loot marker"),
        (
            "<MaxHitPoints max=\"99\">100</MaxHitPoints>",
            "invalid MaxHitPoints cap",
        ),
        ("<MaxHitPoints>-1</MaxHitPoints>", "negative MaxHitPoints"),
        (
            "<MaxHitPoints max=\"100\">100</MaxHitPoints><LevelIncrease min=\"2\" max=\"1\">MaxHitPoints</LevelIncrease>",
            "invalid growth range/cap",
        ),
        (
            "<Loot prob=\"NaN\">Health Potion</Loot>",
            "Loot probability",
        ),
        (
            "<Loot prob=\"1.1\">Health Potion</Loot>",
            "Loot probability",
        ),
        ("<Loot>Health Potion</Loot>", "missing prob"),
    ] {
        fails(fields, message);
    }
    for (features, message) in [
        ("<Wavy>1</Wavy>", "invalid Wavy"),
        ("<Continues/><Continues/>", "invalid Continues"),
        (
            "<ActivateOnEquip>FutureAction</ActivateOnEquip>",
            "unsupported ActivateOnEquip",
        ),
        (
            "<ConditionEffect duration=\"1\">Unknown</ConditionEffect>",
            "unknown July condition",
        ),
        ("<Speed>100</Speed>", "duplicate Speed"),
        (
            "<MinDamage>1</MinDamage><MaxDamage>2</MaxDamage>",
            "conflicting projectile damage",
        ),
    ] {
        fails(&projectile(features), message);
    }
    assert!(parse("<Loot prob=\"0.1\">Absent</Loot>").is_err());
    assert!(parse("<Loot prob=\"0.1\">Blade</Loot>").is_err());
}

#[test]
#[ignore = "requires both BUILD100_CONTENT_ORIGINAL_DIR and BUILD100_CONTENT_REPAIRED_DIR"]
fn content_contracts_qualified_original_and_repaired_vectors() {
    for (env, repaired) in [
        ("BUILD100_CONTENT_ORIGINAL_DIR", false),
        ("BUILD100_CONTENT_REPAIRED_DIR", true),
    ] {
        let dir = std::env::var_os(env).expect(env);
        let c = Content::load(Path::new(&dir)).unwrap();
        assert_eq!(c.objects.len(), 528);
        assert_eq!(c.objects.values().map(|o| o.loot.len()).sum::<usize>(), 723);
        assert_eq!(
            c.objects
                .values()
                .flat_map(|o| &o.loot)
                .filter(|l| **l == Loot::EmptyContainer)
                .count(),
            4
        );
        let cube = c.named("Cube God").unwrap();
        assert_eq!(
            cube.projectiles
                .iter()
                .map(|(id, _)| *id)
                .collect::<Vec<_>>(),
            [0, 0]
        );
        assert!(cube.normal_projectile(0).is_err());
        assert!(c.named("Cube Defender").unwrap().projectiles[0].1.wavy);
        let oryx = &c.named("Oryx the Mad God").unwrap().projectiles[6].1;
        assert!(oryx.armor_piercing());
        assert_eq!(oryx.effects[0].duration_seconds, 0.0);
        assert_eq!(
            c.named("Arachna the Spider Queen")
                .unwrap()
                .loot
                .iter()
                .filter(|l| **l
                    == Loot::Item {
                        name: "Healing Ichor".into(),
                        probability: 0.25
                    })
                .count(),
            3
        );
        for (
            name,
            original,
            repaired_damage,
            nested,
            original_speed,
            repaired_speed,
            lifetime,
            original_rate,
        ) in [
            (
                "Blunt Sword",
                (400, 500),
                (30, 60),
                (30, 60),
                50.0,
                10.0,
                if repaired { 0.3 } else { 0.7 },
                30.0,
            ),
            (
                "Fire Wand",
                (400, 500),
                (20, 40),
                (20, 40),
                80.0,
                16.0,
                1.0,
                60.0,
            ),
            (
                "Blunt Dagger",
                (400, 500),
                (20, 60),
                (20, 60),
                30.0,
                14.0,
                0.4,
                1.0,
            ),
            (
                "Poor Quality Bow",
                (450, 500),
                (10, 40),
                (10, 40),
                36.0,
                16.0,
                1.0,
                1.0,
            ),
        ] {
            let o = c.named(name).unwrap();
            assert_eq!(
                o.root_damage,
                Some(if repaired { repaired_damage } else { original }),
                "{name}"
            );
            assert_eq!(
                o.rate_of_fire,
                Some(if repaired { 1.0 } else { original_rate })
            );
            let p = o.normal_projectile(0).unwrap();
            assert_eq!((p.min, p.max), nested);
            assert_eq!(
                (p.speed, p.lifetime),
                (
                    if repaired {
                        repaired_speed
                    } else {
                        original_speed
                    },
                    lifetime
                )
            );
        }
        assert!(c.named("Poor Quality Bow").unwrap().bonuses.is_empty());
        assert_eq!(
            c.named("Old Helm").unwrap().bonuses,
            [(21, if repaired { 2 } else { 500 })]
        );
        let cloak = c.named("Worn Cloak").unwrap();
        assert_eq!(cloak.bonuses, [(21, if repaired { 2 } else { 10 })]);
        assert_eq!(
            cloak.effects[0],
            ConditionEffect {
                condition: Condition::Invisible,
                duration_seconds: if repaired { 3.0 } else { 30.0 }
            }
        );
        assert_eq!(
            c.named("Ring of Minor Defense").unwrap().bonuses,
            if repaired {
                vec![(21, 1)]
            } else {
                vec![(20, 5)]
            }
        );
        let spell = c.named("Fire Spray Spell").unwrap();
        assert_eq!(
            spell.root_damage,
            Some(if repaired { (30, 50) } else { (300, 500) })
        );
        assert_eq!(spell.mp_cost, Some(if repaired { 40 } else { 0 }));
        assert_eq!(
            (
                spell.projectiles[0].1.min,
                spell.projectiles[0].1.max,
                spell.projectiles[0].1.speed,
                spell.projectiles[0].1.lifetime
            ),
            (30, 50, 0.3, 1.0)
        );
        assert_eq!(c.named("Energy Staff").unwrap().root_damage, Some((10, 25)));
        assert_eq!(c.named("Energy Staff").unwrap().num_projectiles, 2);
        assert_eq!(c.named("Energy Staff").unwrap().arc_gap_radians, 0.0);
        assert_eq!(c.named("Ent Ancient").unwrap().xp_multiplier, 5.0);
        assert!(c.named("Pirate").unwrap().loot.contains(&Loot::Item {
            name: "Pirate Cave Key".into(),
            probability: 0.02
        }));
        assert_eq!(
            c.named("Divine Heal Spell")
                .unwrap()
                .activations
                .iter()
                .map(|a| a.action().unwrap())
                .collect::<Vec<_>>(),
            [
                Action::HealNova {
                    range: 15.0,
                    healing: 200
                },
                Action::RemoveNegativeConditions {
                    range: 15.0,
                    healing: 200
                }
            ]
        );
        assert_eq!(
            c.named("Cloak of the Planewalker")
                .unwrap()
                .activations
                .iter()
                .map(|a| a.action().unwrap())
                .collect::<Vec<_>>(),
            [Action::Teleport, Action::ConditionEffectSelf]
        );
        assert_eq!(
            c.named("Seal of the Holy Warrior")
                .unwrap()
                .effects
                .iter()
                .map(|e| e.condition)
                .collect::<Vec<_>>(),
            [Condition::Healing, Condition::Damaging]
        );
        for (name, weapon, ability, slots, base, caps, growth) in [
            (
                "Rogue",
                0xa14,
                0xa56,
                [2, 13, 6, 9],
                [150, 100, 10, 0, 15, 15, 10, 15],
                [1000, 500, 50, 25, 75, 75, 50, 75],
                [
                    (20, 30),
                    (2, 8),
                    (0, 2),
                    (0, 0),
                    (1, 2),
                    (1, 2),
                    (0, 2),
                    (1, 2),
                ],
            ),
            (
                "Archer",
                0xa1a,
                0xa61,
                [3, 15, 6, 9],
                [130, 100, 12, 0, 12, 50, 10, 12],
                [1000, 500, 75, 25, 50, 175, 50, 50],
                [
                    (20, 30),
                    (2, 8),
                    (1, 2),
                    (0, 0),
                    (0, 2),
                    (0, 2),
                    (0, 2),
                    (0, 2),
                ],
            ),
            (
                "Wizard",
                0xa97,
                0xa2e,
                [17, 11, 14, 9],
                [100, 100, 12, 0, 10, 12, 12, 15],
                [1000, 1000, 75, 25, 50, 75, 75, 75],
                [
                    (20, 30),
                    (5, 15),
                    (1, 2),
                    (0, 0),
                    (0, 2),
                    (0, 2),
                    (1, 2),
                    (1, 2),
                ],
            ),
            (
                "Priest",
                0xa04,
                0xa31,
                [8, 4, 14, 9],
                [100, 100, 10, 0, 12, 10, 15, 12],
                [1000, 1000, 50, 25, 50, 75, 75, 50],
                [
                    (20, 30),
                    (5, 15),
                    (0, 2),
                    (0, 0),
                    (1, 2),
                    (0, 2),
                    (1, 2),
                    (0, 2),
                ],
            ),
            (
                "Warrior",
                0xa00,
                0xa66,
                [1, 16, 7, 9],
                [200, 100, 15, 0, 7, 10, 10, 10],
                [1000, 500, 75, 25, 50, 75, 50, 50],
                [
                    (20, 30),
                    (2, 8),
                    (1, 2),
                    (0, 0),
                    (0, 2),
                    (0, 2),
                    (0, 2),
                    (0, 2),
                ],
            ),
            (
                "Knight",
                0xa00,
                0xa08,
                [1, 5, 7, 9],
                [200, 100, 15, 0, 7, 10, 10, 10],
                [1000, 500, 50, 50, 50, 75, 50, 50],
                [
                    (20, 30),
                    (2, 8),
                    (1, 2),
                    (0, 0),
                    (0, 2),
                    (0, 2),
                    (0, 2),
                    (0, 2),
                ],
            ),
            (
                "Paladin",
                0xa00,
                0xa52,
                [1, 12, 7, 9],
                [200, 100, 12, 0, 7, 10, 10, 10],
                [1000, 500, 50, 25, 50, 75, 50, 50],
                [
                    (20, 30),
                    (2, 8),
                    (1, 2),
                    (0, 0),
                    (0, 2),
                    (0, 2),
                    (0, 2),
                    (0, 2),
                ],
            ),
        ] {
            let o = c.named(name).unwrap();
            let ability_object = c.get(ability as i16).unwrap();
            let (cost, action) = match name {
                "Rogue" => (80, Action::ConditionEffectSelf),
                "Archer" => (60, Action::Shoot),
                "Wizard" => (if repaired { 40 } else { 0 }, Action::BulletNova),
                "Priest" => (
                    60,
                    Action::HealNova {
                        range: 10.0,
                        healing: 100,
                    },
                ),
                "Warrior" => (60, Action::ConditionEffectSelf),
                "Knight" => (80, Action::Shoot),
                "Paladin" => (60, Action::ConditionEffectAura { range: 10.0 }),
                _ => unreachable!(),
            };
            assert_eq!(ability_object.mp_cost, Some(cost));
            assert_eq!(
                ability_object
                    .single_activation()
                    .unwrap()
                    .action()
                    .unwrap(),
                action
            );
            assert_eq!(
                o.equipment,
                [weapon, ability, -1, -1, 0xa22, -1, -1, -1, -1, -1, -1, -1]
            );
            assert_eq!(o.slots, [&slots[..], &[0; 8]].concat());
            assert_eq!(o.growth.len(), 8);
            for (i, stat) in [0, 3, 20, 21, 22, 26, 27, 28].into_iter().enumerate() {
                assert_eq!(o.stats[&stat], base[i], "{name} {stat}");
                assert!(
                    o.growth
                        .contains(&(stat, growth[i].0, growth[i].1, caps[i])),
                    "{name} {stat}"
                );
            }
        }
    }
}
