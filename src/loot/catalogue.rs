use super::*;
use serde_json::{Value, json};

fn balanced(kind: u16, item: u16) -> bool {
    matches!(
        (kind, item),
        (0x917, 0xc04)
            | (0xd06, 0xc03)
            | (0xd90, 0xc02)
            | (0x90a | 0xe1d, 0xc01)
            | (0xd78, 0xc05)
            | (crate::dungeon::lab::BOSS, 0xc30 | 0xc32)
            | (
                crate::dungeon::manor::BOSS | crate::dungeon::manor::COFFIN,
                0xc09
            )
    )
}
fn entries(content: &Content, kind: u16, picks: &[(f64, Pick)]) -> Vec<Value> {
    picks
        .iter()
        .map(|(probability, pick)| {
            let items = candidates(content, *pick);
            assert!(
                !items.is_empty(),
                "published loot pool is empty for {kind:#x}"
            );
            let balance = matches!(pick, Pick::Item(item) if balanced(kind, *item));
            let note = if kind == 0xd90 && matches!(pick, Pick::Item(0xc02)) {
                format!(
                    "Doom Bow additionally requires at least {} damage (15% HP).",
                    (content.objects[&kind].hp * 15 + 99) / 100
                )
            } else {
                String::new()
            };
            json!({"probability":probability,"items":items,"balance":balance,"note":note})
        })
        .collect()
}
fn rules(content: &Content, kind: u16) -> (String, String, &'static str) {
    use crate::dungeon::{abyss, lab, spider, tomb, trench};
    if abyss::ROSTER.contains(&kind) {
        return (
            if matches!(kind, abyss::MALPHAS | abyss::IDOL) {
                "At least 1% of max HP in aggregated account damage."
            } else {
                "At least one living contributor."
            }
            .into(),
            if matches!(kind, abyss::MALPHAS | abyss::IDOL) {
                "One private attempt per qualified account; VIT and DEF are guaranteed."
            } else {
                "One shared attempt per defeat."
            }
            .into(),
            "src/world/abyss.rs",
        );
    }
    if matches!(kind, lab::BOSS | lab::CREATION) {
        return (
            "Any positive damage; living account contributors.".into(),
            "One private attempt per account, plus a guaranteed participation potion.".into(),
            "src/world/lab.rs",
        );
    }
    if kind == spider::BOSS {
        return (
            "Shared pool; positive-damage accounts also receive private Ichor.".into(),
            "One shared attempt per defeat, plus participation rewards.".into(),
            "src/world/spider.rs",
        );
    }
    if potion_damage(kind).is_some() || matches!(kind, 0x91d | 0x91e) {
        return ("Any positive damage for equipment; potion eligibility is separate.".into(), "Equipment rolls once per kill; bag type determines allocation, with one owner for colored bags.".into(), "src/world.rs");
    }
    let required = damage_threshold(content, kind);
    let eligibility = if private(kind) {
        format!(
            "Damage {} {required}; living, present contributors only.",
            if kind == tomb::SPHINX { ">=" } else { ">" }
        )
    } else {
        "Any positive damage; living, present contributors only.".into()
    };
    let allocation = if tomb::BOSSES.contains(&kind) {
        "Two total private attempts, assigned cyclically to contributors sorted by damage; not two per player.".into()
    } else if kind == crate::dungeon::beach::BUM {
        "One private winner: the highest-damage eligible contributor.".into()
    } else if private(kind) {
        let suffix = if matches!(kind, trench::HERMIT | tomb::SPHINX) {
            " Stat-potion minimums are encounter-wide, not guaranteed to every player."
        } else {
            ""
        };
        format!("One private attempt per eligible player.{suffix}")
    } else if attempts(kind, 2) == 1 {
        "One shared attempt per kill.".into()
    } else {
        format!(
            "min({} × eligible contributors, 8) shared attempts per kill; duplicate equipment is removed.",
            attempts(kind, 1)
        )
    };
    (eligibility, allocation, "src/world.rs")
}

pub(crate) fn snapshot(content: &Content, revision: Option<&str>) -> Value {
    let mut groups = Vec::new();
    for enemy in content
        .objects
        .values()
        .filter(|e| e.enemy && crate::encounter::profile(e.kind).is_some())
    {
        let kind = enemy.kind;
        let (eligibility, allocation, rules_file) = rules(content, kind);
        let source_file = if crate::endgame::ROSTER.contains(&kind) {
            "src/endgame.rs"
        } else {
            "src/loot.rs"
        };
        let mut add = |channel: &str,
                       mode: &str,
                       picks: &[(f64, Pick)],
                       eligibility: String,
                       allocation: String,
                       source_file: &str| {
            if !picks.is_empty() {
                groups.push(json!({"enemy":{"id":kind,"name":enemy.name,"hp":enemy.hp},"channel":channel,"mode":mode,"eligibility":eligibility,"allocation":allocation,"rules_file":rules_file,"source_file":source_file,"entries":entries(content,kind,picks)}));
            }
        };
        let per_item = per_item(kind);
        add(
            "Main loot",
            if per_item {
                "per_item"
            } else if independent(kind) {
                "independent"
            } else {
                "ordered"
            },
            table(kind),
            eligibility.clone(),
            allocation.clone(),
            source_file,
        );
        let bonus_picks: Vec<_> = bonus(kind)
            .iter()
            .map(|&(p, item)| (p, Pick::Item(item)))
            .collect();
        let bonus_eligibility = potion_damage(kind).map_or(eligibility.clone(), |damage| {
            format!("At least {damage} damage for private stat potions.")
        });
        add(
            "Bonus loot",
            "independent",
            &bonus_picks,
            bonus_eligibility,
            if potion_damage(kind).is_some() {
                "One private potion attempt per eligible player; equipment remains separately eligible.".into()
            } else {
                allocation.clone()
            },
            "src/loot.rs",
        );
        if !fauna_private_table(kind).is_empty() {
            add(
                "Private fauna loot",
                "ordered",
                fauna_private_table(kind),
                "More than 1 damage; eligible contributors sorted by damage.".into(),
                format!(
                    "{} × contributors attempts, capped at 8 successful rolls; successes assigned round-robin and deduplicated per owner.",
                    fauna_multiplier(kind)
                ),
                "src/loot.rs",
            );
        }
        if let Some(item) = participation_reward(kind) {
            add(
                "Participation guarantee",
                "independent",
                &[(1., Pick::Item(item))],
                "Any positive damage; living account contributors.".into(),
                "One private participation item per account, in addition to the main table.".into(),
                "src/loot.rs",
            );
        }
        if kind == 0xe37 {
            add(
                "Shared rum",
                "independent",
                &[(0.2, Pick::Item(0xc25))],
                "At least one positive-damage contributor.".into(),
                "One shared roll per defeat, outside the private table.".into(),
                "src/world.rs",
            );
        }
        if kind == crate::dungeon::trench::THESSAL {
            add(
                "High-damage mana guarantee",
                "independent",
                &[(1., Pick::Item(0xaea))],
                "At least 32% of selected max HP in damage.".into(),
                "One private Mana per qualified player; duplicate Mana from other rolls is removed.".into(),
                "src/world.rs",
            );
        }
    }
    for kind in [0xdc2, 0x925, 0x675, 0x678, 0x67d, 0x668] {
        groups.push(json!({"enemy":{"id":kind,"name":format!("{} chest",content.objects[&kind].name),"hp":0},"channel":"Landmark chest","mode":if matches!(kind,0xdc2|0x925){"capped"}else{"independent"},"eligibility":"Reach the spawned landmark chest; not a private kill reward.","allocation":if matches!(kind,0xdc2|0x925){"Shared chest; maximum 3–7 successful item rolls, chosen uniformly. Earlier successes can suppress later entries."}else{"Shared chest; independent tier rolls, including repeated potion attempts."},"rules_file":"src/world/realm_setpieces.rs","source_file":"src/loot.rs","entries":entries(content,kind,&chest_table(kind))}));
    }
    let ids: std::collections::BTreeSet<_> = groups
        .iter()
        .flat_map(|g| g["entries"].as_array().unwrap())
        .flat_map(|e| e["items"].as_array().unwrap())
        .map(|id| u16::try_from(id.as_u64().unwrap()).unwrap())
        .collect();
    let items: Vec<_> = ids
        .into_iter()
        .map(|id| {
            let o = &content.objects[&id];
            json!({"id":id,"name":o.name,"slot":o.slot_type,"tier":o.tier})
        })
        .collect();
    json!({"revision":revision,"items":items,"groups":groups})
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::mock::StepRng;

    #[test]
    fn dungeon_balance_and_catalogue_follow_real_reward_rolls() {
        let content = Content::load(std::path::Path::new("data")).unwrap();
        let snapshot = snapshot(&content, Some(&"a".repeat(40)));
        let pairs = [
            (0x917, 0xc04),
            (0xd06, 0xc03),
            (0xd90, 0xc02),
            (0x90a, 0xc01),
            (0xe1d, 0xc01),
            (0xd78, 0xc05),
            (crate::dungeon::lab::BOSS, 0xc30),
            (crate::dungeon::lab::BOSS, 0xc32),
            (crate::dungeon::manor::BOSS, 0xc09),
            (crate::dungeon::manor::COFFIN, 0xc09),
        ];
        for (kind, item) in pairs {
            for (p, expected) in [(0.0099, true), (0.0101, false)] {
                let sample = (p * u64::MAX as f64) as u64 & !u64::from(u32::MAX);
                assert_eq!(
                    rewards(&content, kind, &mut StepRng::new(sample, 0)).contains(&item),
                    expected,
                    "{kind:#x}/{item:#x} cutoff {p}"
                );
            }
            let group = snapshot["groups"]
                .as_array()
                .unwrap()
                .iter()
                .find(|g| g["enemy"]["id"] == kind && g["channel"] == "Main loot")
                .unwrap();
            let entry = group["entries"]
                .as_array()
                .unwrap()
                .iter()
                .find(|e| e["items"] == json!([item]))
                .unwrap();
            assert_eq!(entry["probability"], 0.01);
            assert_eq!(entry["balance"], true);
        }
        assert!(
            rewards(
                &content,
                crate::dungeon::snake::GUARD,
                &mut StepRng::new((0.04 * u64::MAX as f64) as u64 & !u64::from(u32::MAX), 0)
            )
            .contains(&0xc04)
        );
        assert_eq!(bonus(0xd59), &[(0.001, 0xc0a)]);
        assert_eq!(bonus(0xd60), &[(0.004, 0xc06)]);
        assert_eq!(table(crate::dungeon::tomb::BES)[0].0, 0.009);
        assert!(!item_qualified(0xd90, 0xc02, 1199, 8000));
        assert!(item_qualified(0xd90, 0xc02, 1200, 8000));
        assert_eq!(attempts(crate::dungeon::tomb::BES, 10), 2);
        assert_eq!(
            participation_reward(crate::dungeon::lab::CREATION),
            Some(0xa20)
        );
        assert!(snapshot["groups"].as_array().unwrap().len() > 300);
    }
}
