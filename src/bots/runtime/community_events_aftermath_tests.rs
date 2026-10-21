use super::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(serde::Serialize)]
pub(super) struct AftermathExit {
    account: i32,
    character: i32,
    origin_world: i32,
    origin_player: i32,
    destination_player: i32,
    portal: i32,
    world_local_tick: u64,
    world_local_time_ms: u64,
    directory_bracket_ms: [u64; 2],
    directory_choice_ms: u64,
}

pub(super) fn canonical_death_revisions(
    dir: &std::path::Path,
    actors: &[(i32, i32)],
) -> BTreeMap<(i32, i32), i32> {
    let db = rusqlite::Connection::open_with_flags(
        dir.join("game.db"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    actors.iter().filter_map(|&(account, character)| {
        let row: Option<(i32, i32, String)> = db.query_row(
            "SELECT alive,revision,items FROM characters WHERE account=? AND id=? AND EXISTS(SELECT 1 FROM deaths WHERE character=?)",
            [account, character, character], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
        ).optional().unwrap();
        let (alive, revision, items) = row?;
        let items: Vec<i32> = serde_json::from_str(&items).unwrap();
        (alive==0 && items.len()==12 && items.iter().all(|k| *k == -1))
            .then_some(((account, character),revision))
    }).collect()
}
fn binding(v: &Value) -> Option<(i32, i32)> {
    Some((
        i32::try_from(v["session"]["account"].as_i64()?).ok()?,
        i32::try_from(v["session"]["character"].as_i64()?).ok()?,
    ))
}
fn has_key(v: &Value) -> bool {
    v["items"]
        .as_array()
        .is_some_and(|items| items.contains(&json!(crate::endgame::INCANTATION)))
}
fn living_settled(v: &Value) -> bool {
    v["lifecycle"]["alive"] == true
        && v["lifecycle"]["death_pending"] == false
        && v["lifecycle"]["quarantined"] == false
        && v["lifecycle"]["busy"] == false
        && v["lifecycle"]["output"] == "live"
        && v["diagnostic"]["pending"] == false
}

pub(super) fn completed_oryx_key_lost_aftermath(
    fields: &[Value],
    sequence: &[Value],
    initial: &[Value],
    deaths: &BTreeMap<(i32, i32), i32>,
) -> Option<Vec<AftermathExit>> {
    let actors = initial
        .iter()
        .map(|p| {
            Some((
                i32::try_from(p["character"]["account"].as_i64()?).ok()?,
                i32::try_from(p["character"]["id"].as_i64()?).ok()?,
            ))
        })
        .collect::<Option<BTreeSet<_>>>()?;
    if actors.len() != 6
        || initial.len() != 6
        || actors.iter().any(|(a, ch)| *a <= 0 || *ch <= 0)
        || !super::castle::earned_chamber_prefix(fields)
        || fields.iter().any(|f| {
            (f["message"] == "consumable committed; bound dungeon portal published"
                && f["kind"] == 0x700)
                || (f["message"] == "autonomous durable transfer completed"
                    && f["portal_kind"] == 0x700)
        })
    {
        return None;
    }
    let initial_items = initial
        .iter()
        .map(|p| p["character"]["items"].as_array())
        .collect::<Option<Vec<_>>>()?;
    let keys = initial
        .iter()
        .zip(initial_items)
        .flat_map(|(p, items)| {
            items
                .iter()
                .filter(|k| **k == json!(crate::endgame::INCANTATION))
                .map(move |_| &p["character"])
        })
        .collect::<Vec<_>>();
    if keys.len() != 1 {
        return None;
    }
    let key_owner = (
        i32::try_from(keys[0]["account"].as_i64()?).ok()?,
        i32::try_from(keys[0]["id"].as_i64()?).ok()?,
    );
    let key_death_revision = *deaths.get(&key_owner)?;
    let views = sequence
        .iter()
        .map(|s| s["actors"].as_array())
        .collect::<Option<Vec<_>>>()?;
    if views.iter().flat_map(|a| a.iter()).any(|p| {
        binding(p).is_none_or(|b| !actors.contains(&b))
            || p["map"] == "Wine Cellar"
            || has_key(&p["character"]) && binding(p) != Some(key_owner)
    }) {
        return None;
    }
    let key_last_living = views
        .iter()
        .flat_map(|a| a.iter())
        .rfind(|p| binding(p) == Some(key_owner) && p["lifecycle"]["alive"] == true)?;
    if !has_key(&key_last_living["character"])
        || key_last_living["character"]["inventory_revision"]
            .as_i64()?
            .checked_add(1)?
            != i64::from(key_death_revision)
    {
        return None;
    }
    let key_player = key_last_living["session"]["player"].clone();
    let key_death = fields.iter().position(|f| {
        f["message"] == "permanent death committed"
            && f["character"] == key_owner.1
            && f["player"] == key_player
    })?;
    let engaged = actors
        .iter()
        .filter(|b| {
            views.iter().flat_map(|a| a.iter()).any(|p| {
                binding(p) == Some(**b)
                    && p["shots"].as_u64().is_some_and(|n| n > 0)
                    && p["diagnostic"]["character"]["hits"]
                        .as_u64()
                        .is_some_and(|n| n > 0)
            })
        })
        .count();
    if engaged < 3 {
        return None;
    }
    let final_views = views.last()?;
    let mut exits = Vec::new();
    for &(account, character) in &actors {
        if deaths.contains_key(&(account, character)) {
            if !fields
                .iter()
                .any(|f| f["message"] == "permanent death committed" && f["character"] == character)
            {
                return None;
            }
            continue;
        }
        let terminal = final_views
            .iter()
            .find(|p| binding(p) == Some((account, character)))?;
        if !living_settled(terminal)
            || ![json!("Nexus"), json!("Realm")].contains(&terminal["map"])
            || has_key(&terminal["character"])
        {
            return None;
        }
        if guarded_exit(fields, account) || readiness_exit(fields, account) {
            continue;
        }
        let (exit_index, transfer) = fields.iter().enumerate().find(|(_, f)| {
            f["message"] == "autonomous durable transfer completed"
                && f["account"] == account
                && f["character"] == character
                && f["destination"] == -2
                && f["portal_kind"] == 0x71d
        })?;
        let portal = transfer["portal"]
            .as_str()?
            .strip_prefix("Some(")?
            .strip_suffix(')')?
            .parse::<i32>()
            .ok()?;
        let (choice_index, choice) =
            fields[..exit_index]
                .iter()
                .enumerate()
                .rev()
                .find(|(_, f)| {
                    f["message"] == "autonomous event action selected"
                        && f["account"] == account
                        && f["character"] == character
                        && f["world"] == "Oryx's Chamber"
                        && f["goal"] == format!("Portal({portal})")
                        && f["journey"] == format!("Some(Portal({portal}))")
                })?;
        if portal <= 0
            || key_death >= choice_index
            || !super::castle::earned_chamber_prefix(&fields[..choice_index])
        {
            return None;
        }
        let tick = choice["tick"].as_u64()?;
        let time_ms = choice["time_ms"].as_u64()?;
        if tick.checked_mul(50)? != time_ms
            || choice["held_health"] != 0
            || choice["hp"].as_i64()? <= 0
            || choice["hp"].as_i64()? * 4 >= choice["max_hp"].as_i64()? * 3
        {
            return None;
        }
        let destination_player = i32::try_from(transfer["player"].as_i64()?).ok()?;
        let (post_index, post) = sequence.iter().enumerate().find_map(|(i, s)| {
            views[i]
                .iter()
                .find(|p| {
                    binding(p) == Some((account, character))
                        && p["session"]["player"] == destination_player
                        && p["session"]["world"] == -2
                        && p["map"] == "Nexus"
                })
                .map(|p| (i, (s, p)))
        })?;
        let (pre_index, pre) =
            sequence[..post_index]
                .iter()
                .enumerate()
                .rev()
                .find_map(|(i, s)| {
                    views[i]
                        .iter()
                        .find(|p| {
                            binding(p) == Some((account, character))
                                && p["map"] == "Oryx's Chamber"
                                && p["diagnostic"]["time_ms"]
                                    .as_u64()
                                    .is_some_and(|n| n <= time_ms)
                        })
                        .map(|p| (i, (s, p)))
                })?;
        let origin_world = i32::try_from(pre.1["session"]["world"].as_i64()?).ok()?;
        let origin_player = i32::try_from(pre.1["session"]["player"].as_i64()?).ok()?;
        if origin_world <= 0
            || origin_player == destination_player
            || !living_settled(pre.1)
            || !living_settled(post.1)
            || pre.1["diagnostic"]["player"] != origin_player
            || pre.1["character"]["items"] != post.1["character"]["items"]
            || choice["mp"].as_i64()? * 3 >= pre.1["character"]["stats"][1].as_i64()?
        {
            return None;
        }
        let (entry_index, _) = fields[..choice_index]
            .iter()
            .enumerate()
            .rev()
            .find(|(_, f)| {
                f["message"] == "autonomous durable transfer completed"
                    && f["account"] == account
                    && f["character"] == character
                    && f["destination"] == origin_world
                    && f["player"] == origin_player
                    && f["portal_kind"] == 0xd7b
            })?;
        let defeat = fields[..choice_index].iter().rposition(|f| {
            f["message"] == "actual Oryx defeat opened earned exits"
                && f["boss"] == i32::from(crate::endgame::ORYX)
        })?;
        let clear = fields[..choice_index].iter().rposition(|f| {
            f["message"] == "autonomous shared clear cue observed"
                && f["account"] == account
                && f["world"] == "Oryx's Chamber"
        })?;
        if entry_index >= defeat
            || defeat >= clear
            || fields[entry_index + 1..exit_index].iter().any(|f| {
                f["message"] == "autonomous durable transfer completed" && f["account"] == account
            })
        {
            return None;
        }
        let own_portal = pre.1["diagnostic"]["entities"].as_array()?.iter().any(|e| {
            e["id"] == portal
                && e["kind"] == 0x71d
                && e["published"] == true
                && e["extra_stats"]
                    .as_array()
                    .is_some_and(|s| s.contains(&json!([37, 1])) && !s.contains(&json!([37, 0])))
        });
        let own_damage = views[..=pre_index]
            .iter()
            .flat_map(|a| a.iter())
            .filter(|p| {
                binding(p) == Some((account, character))
                    && p["session"]["world"] == origin_world
                    && p["session"]["player"] == origin_player
            })
            .any(|p| {
                p["diagnostic"]["entities"]
                    .as_array()
                    .is_some_and(|entities| {
                        entities.iter().any(|e| {
                            e["kind"] == i32::from(crate::endgame::ORYX)
                                && e["hp"].as_i64().is_some_and(|n| n > 0)
                                && e["damage"]
                                    .get(account.to_string())
                                    .and_then(Value::as_i64)
                                    .is_some_and(|n| n > 0)
                        })
                    })
            });
        if !own_portal || !own_damage {
            return None;
        }
        // A newly created Chamber and its Directory have different clock origins.
        let before = pre.0["game_ms"].as_u64()?;
        let after = post.0["game_ms"].as_u64()?;
        let local_pre = pre.1["diagnostic"]["time_ms"].as_u64()?;
        if pre.1["diagnostic"]["tick"].as_u64()?.checked_mul(50)? != local_pre {
            return None;
        }
        let directory_choice = before.checked_sub(local_pre)?.checked_add(time_ms)?;
        if before >= after || directory_choice < before || directory_choice > after {
            return None;
        }
        exits.push(AftermathExit {
            account,
            character,
            origin_world,
            origin_player,
            destination_player,
            portal,
            world_local_tick: tick,
            world_local_time_ms: time_ms,
            directory_bracket_ms: [before, after],
            directory_choice_ms: directory_choice,
        });
    }
    (!exits.is_empty()).then_some(exits)
}

#[derive(Clone)]
struct Context {
    fields: Vec<Value>,
    sequence: Vec<Value>,
    initial: Vec<Value>,
    deaths: BTreeMap<(i32, i32), i32>,
}
impl Context {
    fn classified(&self) -> bool {
        completed_oryx_key_lost_aftermath(&self.fields, &self.sequence, &self.initial, &self.deaths)
            .is_some()
    }
}
fn actor(account: i32, player: i32, world: i32, map: &str, tick: u64) -> Value {
    let mut items = vec![2824, 2852, 2821, 2765, -1, -1, -1, -1, -1, -1, -1, -1];
    if account == 2 {
        items[10] = i32::from(crate::endgame::INCANTATION);
    }
    json!({"session":{"account":account,"character":account,"player":player,"world":world},"map":map,
        "shots":1,"character":{"items":items,"inventory_revision":10,"stats":[100,100,40,20,40,40,20,20]},
        "lifecycle":{"alive":true,"busy":false,"death_pending":false,"quarantined":false,"output":"live"},
        "diagnostic":{"player":player,"tick":tick,"time_ms":tick*50,"pending":false,"character":{"hits":1},"entities":[]}})
}
fn aftermath_context() -> Context {
    let initial = (1..=6)
        .map(|account| {
            let a = actor(account, 200 + account, 2, "Oryx's Chamber", 4);
            json!({"character":{"account":account,"id":account,"items":a["character"]["items"]}})
        })
        .collect();
    let key_owner = actor(2, 202, 2, "Oryx's Chamber", 4);
    let guarded = actor(5, 205, 2, "Oryx's Chamber", 4);
    let mut witness = actor(6, 206, 2, "Oryx's Chamber", 4);
    witness["diagnostic"]["entities"] =
        json!([{"id":999,"kind":crate::endgame::ORYX,"hp":1,"damage":{"6":1}}]);
    let mut pre = actor(6, 206, 2, "Oryx's Chamber", 10);
    pre["diagnostic"]["entities"] =
        json!([{"id":99,"kind":0x71d,"published":true,"extra_stats":[[37,1]]}]);
    let nexus5 = actor(5, 305, -2, "Nexus", 30);
    let nexus6 = actor(6, 306, -2, "Nexus", 32);
    Context {
        initial,
        deaths: (1..=4).map(|a| ((a, a), 11)).collect(),
        sequence: vec![
            json!({"game_ms":1200,"actors":[key_owner,guarded,witness]}),
            json!({"game_ms":1500,"actors":[nexus5.clone(),pre]}),
            json!({"game_ms":1600,"actors":[nexus5,nexus6]}),
        ],
        fields: vec![
            json!({"message":"actual bound Guardian sword defeat credited"}),
            json!({"message":"actual Guardian pair defeated; Chamber binding requested"}),
            json!({"message":"autonomous durable transfer completed","account":5,"character":5,"destination":2,"player":205,"portal_kind":0xd7b}),
            json!({"message":"autonomous durable transfer completed","account":6,"character":6,"destination":2,"player":206,"portal_kind":0xd7b}),
            json!({"message":"permanent death committed","character":1,"player":201}),
            json!({"message":"permanent death committed","character":2,"player":202}),
            json!({"message":"permanent death committed","character":3,"player":203}),
            json!({"message":"permanent death committed","character":4,"player":204}),
            json!({"message":"autonomous event action selected","account":5,"character":5,"world":"Oryx's Chamber","hp":10,"max_hp":100,"recovery":0.5,"held_health":0,"journey":"Some(Nexus)"}),
            json!({"message":"autonomous durable transfer completed","account":5,"character":5,"destination":-2,"player":305,"portal":"None"}),
            json!({"message":"actual Oryx defeat opened earned exits","boss":crate::endgame::ORYX}),
            json!({"message":"autonomous shared clear cue observed","account":6,"world":"Oryx's Chamber"}),
            json!({"message":"autonomous event action selected","account":6,"character":6,"world":"Oryx's Chamber","tick":11,"time_ms":550,"hp":40,"max_hp":100,"mp":1,"held_health":0,"goal":"Portal(99)","journey":"Some(Portal(99))"}),
            json!({"message":"autonomous durable transfer completed","account":6,"character":6,"destination":-2,"player":306,"portal":"Some(99)","portal_kind":0x71d}),
        ],
    }
}
#[test]
fn community_events_completed_oryx_key_lost_aftermath_is_not_cellar_or_emergency_success() {
    let c = aftermath_context();
    assert!(c.classified());
    assert!(!guarded_exit(&c.fields, 6));
    assert!(!readiness_exit(&c.fields, 6));
    let proof =
        completed_oryx_key_lost_aftermath(&c.fields, &c.sequence, &c.initial, &c.deaths).unwrap();
    assert_eq!(proof[0].world_local_time_ms, 550);
    assert_eq!(proof[0].directory_choice_ms, 1550);
    assert_eq!(proof[0].directory_bracket_ms, [1500, 1600]);
    let mut c = c.clone();
    c.fields.push(
        json!({"message":"consumable committed; bound dungeon portal published","kind":0x700}),
    );
    assert!(!c.classified());
}
#[test]
fn community_events_completed_aftermath_rejects_wrong_portal_kind_destination_and_origin() {
    for (field, value) in [
        ("portal", json!("Some(98)")),
        ("portal_kind", json!(0x700)),
        ("destination", json!(-3)),
        ("character", json!(7)),
    ] {
        let mut c = aftermath_context();
        c.fields[13][field] = value;
        assert!(!c.classified(), "{field}");
    }
    for (field, value) in [
        ("destination", json!(3)),
        ("player", json!(207)),
        ("character", json!(7)),
    ] {
        let mut c = aftermath_context();
        c.fields[3][field] = value;
        assert!(!c.classified(), "entry {field}");
    }
    let mut c = aftermath_context();
    c.sequence[1]["actors"][1]["session"]["world"] = json!(3);
    assert!(!c.classified());
}
#[test]
fn community_events_completed_aftermath_rejects_missing_reordered_or_unwitnessed_clear() {
    for index in [0, 1, 10, 11, 12, 13] {
        let mut c = aftermath_context();
        c.fields.remove(index);
        assert!(!c.classified(), "missing {index}");
    }
    let mut c = aftermath_context();
    c.fields.swap(0, 1);
    assert!(!c.classified());
    let mut c = aftermath_context();
    c.fields.swap(10, 11);
    assert!(!c.classified());
    let mut c = aftermath_context();
    c.fields[11]["account"] = json!(5);
    assert!(!c.classified());
}
#[test]
fn community_events_completed_aftermath_rejects_pending_cleanup_only_and_transfer_gaps() {
    for field in ["busy", "death_pending", "quarantined"] {
        let mut c = aftermath_context();
        c.sequence[2]["actors"][1]["lifecycle"][field] = json!(true);
        assert!(!c.classified(), "{field}");
    }
    let mut c = aftermath_context();
    c.sequence[2]["actors"][1]["diagnostic"]["pending"] = json!(true);
    assert!(!c.classified());
    let mut c = aftermath_context();
    c.sequence[2]["actors"][1]["lifecycle"]["output"] = json!("lost");
    assert!(!c.classified());
    let mut c = aftermath_context();
    c.fields[13]["message"] = json!("player left");
    assert!(!c.classified());
    let mut c = aftermath_context();
    c.sequence[2]["actors"].as_array_mut().unwrap().pop();
    assert!(!c.classified());
    let mut c = aftermath_context();
    c.sequence[2]["actors"][1]["session"]["player"] = json!(206);
    assert!(!c.classified());
}
#[test]
fn community_events_completed_aftermath_rejects_mixed_directory_and_world_clock_axes() {
    for (tick, time) in [(31, 1550), (30, 1500), (11, 1500)] {
        let mut c = aftermath_context();
        c.fields[12]["tick"] = json!(tick);
        c.fields[12]["time_ms"] = json!(time);
        assert!(!c.classified());
    }
    let mut c = aftermath_context();
    c.sequence[1]["actors"][1]["diagnostic"]["tick"] = json!(30);
    assert!(!c.classified());
    let mut c = aftermath_context();
    c.sequence[2]["game_ms"] = json!(1501);
    assert!(!c.classified());
}
#[test]
fn community_events_completed_aftermath_rejects_available_or_unproven_lost_key() {
    let mut c = aftermath_context();
    c.deaths.remove(&(2, 2));
    assert!(!c.classified());
    let mut c = aftermath_context();
    c.deaths.insert((2, 2), 12);
    assert!(!c.classified());
    let mut c = aftermath_context();
    c.fields[5]["message"] = json!("death pending");
    assert!(!c.classified());
    let mut c = aftermath_context();
    c.initial[1]["character"]["items"][10] = json!(-1);
    assert!(!c.classified());
    let mut c = aftermath_context();
    c.initial[4]["character"]["items"][10] = json!(crate::endgame::INCANTATION);
    assert!(!c.classified());
    let mut c = aftermath_context();
    c.sequence[2]["actors"][0]["character"]["items"][10] = json!(crate::endgame::INCANTATION);
    assert!(!c.classified());
    let mut c = aftermath_context();
    c.sequence[0]["actors"][0]["session"]["character"] = json!(20);
    assert!(!c.classified());
}
#[test]
fn community_events_completed_aftermath_requires_own_damage_public_choice_and_other_participant_accounting()
 {
    let mut c = aftermath_context();
    c.sequence[0]["actors"][2]["diagnostic"]["entities"][0]["damage"]["6"] = json!(0);
    assert!(!c.classified());
    let mut c = aftermath_context();
    c.sequence[1]["actors"][1]["diagnostic"]["entities"][0]["extra_stats"] = json!([[37, 0]]);
    assert!(!c.classified());
    let mut c = aftermath_context();
    c.fields[12]["journey"] = json!("None");
    assert!(!c.classified());
    let mut c = aftermath_context();
    c.fields[8]["journey"] = json!("None");
    assert!(!c.classified());
    let mut c = aftermath_context();
    c.sequence[0]["actors"][0]["shots"] = json!(0);
    assert!(!c.classified());
    let mut c = aftermath_context();
    c.fields[12]["hp"] = json!(90);
    assert!(!c.classified());
    let mut c = aftermath_context();
    c.fields[12]["held_health"] = json!(1);
    assert!(!c.classified());
}
