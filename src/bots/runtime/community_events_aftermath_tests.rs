use super::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(serde::Serialize)]
pub(super) struct AftermathExit {
    account: i32,
    character: i32,
    origin_world: i32,
    origin_player: i32,
    destination_player: i32,
    portal: Option<i32>,
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

fn complete_key_universe(
    audit: &Value,
    initial: &[Value],
    actors: &BTreeSet<(i32, i32)>,
) -> Option<()> {
    if audit["allocation_complete"] != true
        || audit["storage_world_seen"] != false
        || audit["any_pending"] != false
        || audit["pending_opening"] != false
        || audit["open_cellar"] != false
        || !audit["key_sources"].as_array()?.is_empty()
    {
        return None;
    }
    let next = audit["next_world"].as_i64()?;
    let ids = audit["positive_world_ids"].as_array()?;
    if next <= 0
        || i64::try_from(ids.len()).ok()? != next - 1
        || !ids
            .iter()
            .enumerate()
            .all(|(i, id)| id.as_i64() == i64::try_from(i + 1).ok())
    {
        return None;
    }
    let mut seen = BTreeSet::new();
    for p in audit["actors"].as_array()? {
        let pair = (
            i32::try_from(p["account"].as_i64()?).ok()?,
            i32::try_from(p["character"].as_i64()?).ok()?,
        );
        if !actors.contains(&pair) || !seen.insert(pair.0) {
            return None;
        }
    }
    for p in initial {
        let vaults = p["vaults"].as_array()?;
        if vaults.is_empty() {
            return None;
        }
        let mut chests = BTreeSet::new();
        for v in vaults {
            if v["account"] != p["character"]["account"]
                || v["revision"] != 0
                || v["chest"].as_i64()? < 0
                || !chests.insert(v["chest"].as_i64()?)
                || v["items"].as_array()?.len() != 8
                || v["items"].as_array()?.iter().any(|k| *k != -1)
            {
                return None;
            }
        }
    }
    Some(())
}

fn selected_postclear_input(
    fields: &[Value],
    choice: usize,
    transfer: usize,
    initial: &[Value],
    actors: &BTreeSet<(i32, i32)>,
) -> Option<(Value, Value)> {
    use sha2::Digest;
    let selected = fields[choice + 1..transfer].iter().rev().find(|f| {
        f["message"] == "prepared postclear final selected decision"
            && f["account"] == fields[choice]["account"]
            && f["character"] == fields[choice]["character"]
            && f["tick"] == fields[choice]["tick"]
            && f["time_ms"] == fields[choice]["time_ms"]
            && f["goal"] == fields[choice]["goal"]
            && f["journey"] == fields[choice]["journey"]
            && f["item"] == "None"
            && f["movement"] == "None"
    })?;
    let session: Value = serde_json::from_str(selected["session"].as_str()?).ok()?;
    if session["account"] != fields[choice]["account"]
        || session["character"] != fields[choice]["character"]
    {
        return None;
    }
    let audit = fields[..choice]
        .iter()
        .rev()
        .filter(|f| f["message"] == "prepared postclear evaluator key audit")
        .filter_map(|f| serde_json::from_str::<Value>(f["audit"].as_str()?).ok())
        .find(|a| {
            a["normal_view_sha256"] == selected["normal_view_sha256"] && a["session"] == session
        })?;
    let raw = audit["normal_view_raw"].as_str()?;
    if format!("{:x}", sha2::Sha256::digest(raw.as_bytes()))
        != selected["normal_view_sha256"].as_str()?
    {
        return None;
    }
    let normal: Value = serde_json::from_str(raw).ok()?;
    if normal["session"] != session
        || normal["tick"] != selected["tick"]
        || normal["time_ms"] != selected["time_ms"]
        || normal["map"]["name"] != "Oryx's Chamber"
        || normal["lifecycle"]["alive"] != true
        || normal["lifecycle"]["death_pending"] != false
        || normal["lifecycle"]["quarantined"] != false
        || normal["lifecycle"]["busy"] != false
        || normal["lifecycle"]["departing"] != false
        || normal["lifecycle"]["output"] != "live"
        || has_key(&normal["character"])
        || normal["objects"].as_array()?.iter().any(|o| {
            o["contents"]
                .as_array()
                .is_some_and(|items| items.contains(&json!(crate::endgame::INCANTATION)))
        })
        || audit["tick"] != normal["tick"]
        || audit["time_ms"] != normal["time_ms"]
        || audit["chamber_cleared"] != true
    {
        return None;
    }
    complete_key_universe(&audit, initial, actors)?;
    if !audit["actors"].as_array()?.iter().any(|p| {
        p["account"] == session["account"]
            && p["character"] == session["character"]
            && p["world"] == session["world"]
            && p["player"] == session["player"]
    }) {
        return None;
    }
    let terminal = fields
        .iter()
        .rposition(|f| f["message"] == "prepared postclear terminal key audit")?;
    let ack = fields.get(transfer + 1..terminal)?.iter().find(|f| {
        f["message"] == "prepared postclear durable transfer ACK"
            && f["normal_view_sha256"] == selected["normal_view_sha256"]
            && f["account"] == session["account"]
            && f["character"] == session["character"]
            && f["origin_world"] == session["world"]
            && f["origin_player"] == session["player"]
            && f["player"] == fields[transfer]["player"]
            && f["destination"] == -2
            && f["portal"] == fields[transfer]["portal"]
            && f["tick"] == normal["tick"]
            && f["time_ms"] == normal["time_ms"]
    })?;
    if ack["player"] == session["player"] {
        return None;
    }
    Some((normal, audit))
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
    let terminal = fields
        .iter()
        .rev()
        .find(|f| f["message"] == "prepared postclear terminal key audit")?;
    let terminal: Value = serde_json::from_str(terminal["audit"].as_str()?).ok()?;
    complete_key_universe(&terminal, initial, &actors)?;
    if terminal["clock_paused"] != true
        || terminal["directory_tick"].as_u64()?.checked_mul(50)?
            < sequence.last()?["game_ms"].as_u64()?
    {
        return None;
    }
    let initial_items = initial
        .iter()
        .map(|p| p["character"]["items"].as_array())
        .collect::<Option<Vec<_>>>()?;
    if initial_items.iter().any(|items| items.len() != 12) {
        return None;
    }
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
        p["normal_view"]["objects"]
            .as_array()
            .is_some_and(|objects| {
                objects.iter().any(|o| {
                    o["contents"]
                        .as_array()
                        .is_some_and(|items| items.contains(&json!(crate::endgame::INCANTATION)))
                })
            })
            || binding(p).is_none_or(|b| !actors.contains(&b))
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
                && (f["portal_kind"] == 0x71d || f["portal"] == "None")
        })?;
        let portal = if transfer["portal"] == "None" {
            None
        } else {
            Some(
                transfer["portal"]
                    .as_str()?
                    .strip_prefix("Some(")?
                    .strip_suffix(')')?
                    .parse::<i32>()
                    .ok()?,
            )
        };
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
                        && f["goal"]
                            == portal.map_or_else(|| "Wait".to_owned(), |p| format!("Portal({p})"))
                        && f["journey"]
                            == portal.map_or_else(
                                || "Some(Nexus)".to_owned(),
                                |p| format!("Some(Portal({p}))"),
                            )
                })?;
        if portal.is_some_and(|p| p <= 0)
            || key_death >= choice_index
            || !super::castle::earned_chamber_prefix(&fields[..choice_index])
        {
            return None;
        }
        let (normal, audit) =
            selected_postclear_input(fields, choice_index, exit_index, initial, &actors)?;
        if portal.is_none() {
            let branch = fields[..choice_index].iter().rev().find(|f| {
                f["message"] == "autonomous postclear own-view branch witness"
                    && f["normal_view"] == audit["normal_view_raw"]
            })?;
            if branch["clear_world"] != normal["session"]["world"]
                || branch["clear_character"] != character
                || branch["clear_time_ms"].as_u64()? > normal["time_ms"].as_u64()?
                || branch["pending"] != false
                || branch["applicable_remedy"] != false
                || branch["loot_waiting"] != false
                || branch["live_objective"] != false
            {
                return None;
            }
            let objects = normal["objects"].as_array()?;
            if !objects
                .iter()
                .any(|o| o["kind"] == 0x721 && o["usable"] == true)
                || objects.iter().any(|o| {
                    o["usable"] == true
                        && [json!(0x71d), json!(0x712), json!(0x700)].contains(&o["kind"])
                })
            {
                return None;
            }
        }
        let tick = choice["tick"].as_u64()?;
        let time_ms = choice["time_ms"].as_u64()?;
        if tick.checked_mul(50)? != time_ms
            || choice["hp"].as_i64()? <= 0
            || normal["character"]["hp"] != choice["hp"]
            || normal["character"]["mp"] != choice["mp"]
            || normal["character"]["stats"][0] != choice["max_hp"]
            || portal.is_some()
                && (choice["held_health"] != 0
                    || choice["hp"].as_i64()? * 4 >= choice["max_hp"].as_i64()? * 3)
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
            || normal["session"] != pre.1["session"]
            || normal["character"]["items"] != pre.1["character"]["items"]
            || portal.is_some()
                && choice["mp"].as_i64()? * 3 >= pre.1["character"]["stats"][1].as_i64()?
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
        if key_death >= defeat
            || entry_index >= defeat
            || defeat >= clear
            || fields[entry_index + 1..exit_index].iter().any(|f| {
                f["message"] == "autonomous durable transfer completed" && f["account"] == account
            })
        {
            return None;
        }
        let own_portal = pre.1["diagnostic"]["entities"].as_array()?.iter().any(|e| {
            e["id"].as_i64().and_then(|p| i32::try_from(p).ok()) == portal
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
        if portal.is_some() && !own_portal || !own_damage {
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
        if audit["directory_tick"].as_u64()?.checked_mul(50)? != directory_choice
            || before >= after
            || directory_choice < before
            || directory_choice > after
        {
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
fn declared_normal() -> crate::world::view::PlayerView {
    let content = crate::bots::tests::content();
    let mut v = crate::bots::tests::view(&content);
    v.session = crate::world::view::Session {
        account: 6,
        character: 6,
        player: 206,
        world: 2,
    };
    v.map.name = "Oryx's Chamber".into();
    v.tick = 11;
    v.time_ms = 550;
    v.character.level = 20;
    v.character.items = vec![2824, 2852, 2821, 2765, -1, -1, -1, -1, -1, -1, -1, -1];
    v.character.inventory_revision = 10;
    v.character.stats = [100, 100, 40, 20, 40, 40, 20, 20];
    v.character.base_stats = v.character.stats;
    v.character.hp = 40;
    v.character.mp = 1;
    v.objects
        .push(crate::bots::tests::object(99, 0x71d, 12.5, 11.5));
    v
}
fn declared_selection(
    normal: &crate::world::view::PlayerView,
    goal: &str,
    journey: &str,
    portal: &str,
    directory_tick: u64,
) -> (Value, Value, Value) {
    use sha2::Digest;
    let raw = serde_json::to_string(normal).unwrap();
    let hash = format!("{:x}", sha2::Sha256::digest(raw.as_bytes()));
    let key_sources: Vec<_> = normal
        .objects
        .iter()
        .filter(|o| o.contents.contains(&i32::from(crate::endgame::INCANTATION)))
        .map(|o| json!({"container":o.id}))
        .chain(
            normal
                .character
                .items
                .contains(&i32::from(crate::endgame::INCANTATION))
                .then(|| json!({"player":normal.session.player})),
        )
        .collect();
    let audit = json!({"session":normal.session,"tick":normal.tick,"time_ms":normal.time_ms,"normal_view_raw":raw,"normal_view_sha256":hash,
        "directory_tick":directory_tick,"chamber_cleared":true,"next_world":3,"positive_world_ids":[1,2],"allocation_complete":true,"storage_world_seen":false,
        "actors":[{"account":5,"character":5,"world":-2,"player":305},{"account":6,"character":6,"world":2,"player":206}],
        "key_sources":key_sources,"any_pending":false,"pending_opening":false,"open_cellar":false});
    (
        json!({"message":"prepared postclear evaluator key audit","audit":audit.to_string()}),
        json!({"message":"prepared postclear final selected decision","account":6,"character":6,"session":serde_json::to_string(&normal.session).unwrap(),"tick":normal.tick,"time_ms":normal.time_ms,"normal_view_sha256":hash,"goal":goal,"journey":journey,"item":"None","movement":"None"}),
        json!({"message":"prepared postclear durable transfer ACK","account":6,"character":6,"origin_world":2,"origin_player":206,"destination":-2,"player":306,"tick":normal.tick,"time_ms":normal.time_ms,"normal_view_sha256":hash,"portal":portal}),
    )
}

fn declared_terminal(audit: &Value, tick: u64) -> Value {
    let mut a: Value = serde_json::from_str(audit["audit"].as_str().unwrap()).unwrap();
    for field in [
        "session",
        "tick",
        "time_ms",
        "normal_view_raw",
        "normal_view_sha256",
        "chamber_cleared",
    ] {
        a.as_object_mut().unwrap().remove(field);
    }
    a["clock_paused"] = json!(true);
    a["directory_tick"] = json!(tick);
    a["actors"] = json!([{"account":5,"character":5,"world":-2,"player":305},{"account":6,"character":6,"world":-2,"player":306}]);
    json!({"message":"prepared postclear terminal key audit","audit":a.to_string()})
}

fn aftermath_context() -> Context {
    let initial = (1..=6)
        .map(|account| {
            let a = actor(account, 200 + account, 2, "Oryx's Chamber", 4);
            json!({"character":{"account":account,"id":account,"items":a["character"]["items"]},"vaults":[{"account":account,"chest":0,"revision":0,"items":[-1,-1,-1,-1,-1,-1,-1,-1]}]})
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
    let normal = declared_normal();
    let (audit, selected, ack) =
        declared_selection(&normal, "Portal(99)", "Some(Portal(99))", "Some(99)", 31);
    let terminal = declared_terminal(&audit, 32);
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
            audit,
            json!({"message":"autonomous event action selected","account":6,"character":6,"world":"Oryx's Chamber","tick":11,"time_ms":550,"hp":40,"max_hp":100,"mp":1,"held_health":0,"goal":"Portal(99)","journey":"Some(Portal(99))"}),
            selected,
            json!({"message":"autonomous durable transfer completed","account":6,"character":6,"destination":-2,"player":306,"portal":"Some(99)","portal_kind":0x71d}),
            ack,
            terminal,
        ],
    }
}
#[test]
fn community_events_c13_visible_replacement_key_in_declared_normal_view_rejects_aftermath() {
    let mut c = aftermath_context();
    let content = crate::bots::tests::content();
    let mut normal = crate::bots::tests::view(&content);
    normal.session = crate::world::view::Session {
        account: 6,
        character: 6,
        player: 206,
        world: 2,
    };
    normal.map.name = "Oryx's Chamber".into();
    normal.tick = 10;
    normal.time_ms = 500;
    normal.character.items =
        serde_json::from_value(c.sequence[1]["actors"][1]["character"]["items"].clone()).unwrap();
    normal.character.stats =
        serde_json::from_value(c.sequence[1]["actors"][1]["character"]["stats"].clone()).unwrap();
    normal.character.base_stats = normal.character.stats;
    normal
        .objects
        .push(crate::bots::tests::object(99, 0x71d, 12.5, 11.5));
    let mut bag = crate::bots::tests::object(77, 0x500, 12.5, 11.5);
    bag.contents = vec![i32::from(crate::endgame::INCANTATION)];
    normal.objects.push(bag);
    c.sequence[1]["actors"][1]["normal_view"] = serde_json::to_value(normal).unwrap();
    assert!(
        !c.classified(),
        "known visible Incantation container cannot count as unavailable replacement key"
    );
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
        c.fields[15][field] = value;
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
    for index in [0, 1, 10, 11, 12, 13, 14, 15, 16, 17] {
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
    c.fields[15]["message"] = json!("player left");
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
        c.fields[13]["tick"] = json!(tick);
        c.fields[13]["time_ms"] = json!(time);
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
    c.fields[13]["journey"] = json!("None");
    assert!(!c.classified());
    let mut c = aftermath_context();
    c.fields[8]["journey"] = json!("None");
    assert!(!c.classified());
    let mut c = aftermath_context();
    c.sequence[0]["actors"][0]["shots"] = json!(0);
    assert!(!c.classified());
    let mut c = aftermath_context();
    c.fields[13]["hp"] = json!(90);
    assert!(!c.classified());
    let mut c = aftermath_context();
    c.fields[13]["held_health"] = json!(1);
    assert!(!c.classified());
}

fn nexus_context_with(change: impl FnOnce(&mut crate::world::view::PlayerView)) -> Context {
    let mut c = aftermath_context();
    let mut normal = declared_normal();
    normal.tick = 300;
    normal.time_ms = 15000;
    normal.elapsed_ms = 16000;
    normal.character.stats = [670, 385, 75, 25, 50, 75, 40, 60];
    normal.character.base_stats = normal.character.stats;
    normal.character.hp = 670;
    normal.character.mp = 385;
    normal.character.items[4..6].fill(0xa22);
    normal.objects.clear();
    normal
        .objects
        .push(crate::bots::tests::object(721, 0x721, 12.5, 11.5));
    change(&mut normal);
    c.initial[5]["character"]["items"] = json!(normal.character.items);
    for sample in &mut c.sequence {
        for p in sample["actors"].as_array_mut().unwrap() {
            if binding(p) == Some((6, 6)) {
                p["character"]["items"] = json!(normal.character.items);
                p["character"]["stats"] = json!(normal.character.stats);
            }
        }
    }
    c.sequence[1]["game_ms"] = json!(15950);
    c.sequence[1]["actors"][1]["diagnostic"]["tick"] = json!(299);
    c.sequence[1]["actors"][1]["diagnostic"]["time_ms"] = json!(14950);
    c.sequence[2]["game_ms"] = json!(16050);
    let (audit, selected, ack) = declared_selection(&normal, "Wait", "Some(Nexus)", "None", 320);
    c.fields[17] = declared_terminal(&audit, 321);
    c.fields[12] = audit;
    c.fields[13] = json!({"message":"autonomous event action selected","account":6,"character":6,"world":"Oryx's Chamber","tick":normal.tick,"time_ms":normal.time_ms,"hp":normal.character.hp,"max_hp":normal.character.stats[0],"mp":normal.character.mp,"held_health":2,"goal":"Wait","journey":"Some(Nexus)"});
    c.fields[14] = selected;
    c.fields[15] = json!({"message":"autonomous durable transfer completed","account":6,"character":6,"destination":-2,"player":306,"portal":"None"});
    c.fields[16] = ack;
    c.fields.insert(13,json!({"message":"autonomous postclear own-view branch witness","normal_view":serde_json::to_string(&normal).unwrap(),"clear_world":2,"clear_character":6,"clear_time_ms":5000,"pending":false,"applicable_remedy":false,"loot_waiting":false,"live_objective":false}));
    c
}
fn edit_declared_audit(c: &mut Context, change: impl FnOnce(&mut Value)) {
    let mut audit: Value = serde_json::from_str(c.fields[12]["audit"].as_str().unwrap()).unwrap();
    change(&mut audit);
    c.fields[12]["audit"] = json!(audit.to_string());
}

#[test]
fn community_events_c13_exact_healthy_postclear_nexus_is_only_prepared_cellar_negative() {
    let c = nexus_context_with(|_| {});
    let exits =
        completed_oryx_key_lost_aftermath(&c.fields, &c.sequence, &c.initial, &c.deaths).unwrap();
    assert_eq!(exits.len(), 1);
    assert_eq!(exits[0].portal, None);
    assert_eq!(exits[0].directory_choice_ms, 16000);
    assert!(!guarded_exit(&c.fields, 6));
    assert!(!readiness_exit(&c.fields, 6));
}
#[test]
fn community_events_c13_unknown_storage_reclaimed_vault_pending_and_replacement_keys_reject_both_paths()
 {
    for nexus in [false, true] {
        for field in [
            "allocation_complete",
            "storage_world_seen",
            "any_pending",
            "pending_opening",
            "open_cellar",
            "chamber_cleared",
        ] {
            let mut c = if nexus {
                nexus_context_with(|_| {})
            } else {
                aftermath_context()
            };
            edit_declared_audit(&mut c, |a| {
                a[field] = if matches!(field, "allocation_complete" | "chamber_cleared") {
                    json!(false)
                } else {
                    json!(true)
                }
            });
            assert!(!c.classified(), "{nexus} {field}");
        }
        for field in ["key_sources", "actors", "next_world", "positive_world_ids"] {
            let mut c = if nexus {
                nexus_context_with(|_| {})
            } else {
                aftermath_context()
            };
            edit_declared_audit(&mut c, |a| {
                a[field] = match field {
                    "key_sources" => json!([{"container":77}]),
                    "actors" => json!([{"account":99,"character":99}]),
                    "next_world" => json!(4),
                    _ => json!([1, 3]),
                }
            });
            assert!(!c.classified(), "{nexus} {field}");
        }
        let mut c = if nexus {
            nexus_context_with(|_| {})
        } else {
            aftermath_context()
        };
        c.initial[0]["vaults"] = Value::Null;
        assert!(!c.classified());
        let mut c = if nexus {
            nexus_context_with(|_| {})
        } else {
            aftermath_context()
        };
        c.initial[0]["vaults"][0]["items"][0] = json!(crate::endgame::INCANTATION);
        assert!(!c.classified());
    }
}
#[test]
fn community_events_c13_safe_opening_held_key_visible_key_loot_or_missing_locked_cue_reject() {
    for kind in [0x71d, 0x712, 0x700] {
        let c = nexus_context_with(|v| {
            v.objects
                .push(crate::bots::tests::object(98, kind, 12.5, 11.5))
        });
        assert!(!c.classified(), "usable opening {kind}");
    }
    let c = nexus_context_with(|v| v.character.items[10] = i32::from(crate::endgame::INCANTATION));
    assert!(!c.classified());
    let c = nexus_context_with(|v| {
        let mut bag = crate::bots::tests::object(77, 0x500, 12.5, 11.5);
        bag.contents = vec![i32::from(crate::endgame::INCANTATION)];
        v.objects.push(bag);
    });
    assert!(!c.classified());
    let c = nexus_context_with(|v| v.objects.clear());
    assert!(!c.classified());
}
#[test]
fn community_events_c13_applicable_remedy_midfight_pending_override_unknown_or_stale_witness_reject()
 {
    for flag in [
        "applicable_remedy",
        "live_objective",
        "pending",
        "loot_waiting",
    ] {
        let mut c = nexus_context_with(|_| {});
        c.fields[13][flag] = json!(true);
        assert!(!c.classified(), "{flag}");
    }
    for index in [12, 13, 15, 17, 18] {
        let mut c = nexus_context_with(|_| {});
        c.fields.remove(index);
        assert!(!c.classified(), "missing audit/branch/final/ACK {index}");
    }
    for (field, value) in [
        ("item", json!("Some(Use)")),
        ("journey", json!("None")),
        ("normal_view_sha256", json!("stale")),
    ] {
        let mut c = nexus_context_with(|_| {});
        c.fields[15][field] = value;
        assert!(!c.classified(), "final {field}");
    }
    let mut c = nexus_context_with(|v| v.lifecycle.busy = true);
    assert!(!c.classified());
    c = nexus_context_with(|_| {});
    c.fields[13]["clear_character"] = json!(99);
    assert!(!c.classified());
    c = nexus_context_with(|_| {});
    c.fields[17]["origin_player"] = json!(999);
    assert!(!c.classified());
    c = nexus_context_with(|_| {});
    c.fields.swap(5, 10);
    assert!(
        !c.classified(),
        "Oryx defeat must follow canonical key death"
    );
}

#[test]
fn community_events_c13_terminal_authority_closes_choice_to_transfer_gap() {
    for nexus in [false, true] {
        for field in [
            "key_sources",
            "pending_opening",
            "open_cellar",
            "any_pending",
            "allocation_complete",
            "clock_paused",
        ] {
            let mut c = if nexus {
                nexus_context_with(|_| {})
            } else {
                aftermath_context()
            };
            let choice: Value =
                serde_json::from_str(c.fields[12]["audit"].as_str().unwrap()).unwrap();
            assert!(choice["key_sources"].as_array().unwrap().is_empty());
            let last = c.fields.len() - 1;
            let mut terminal: Value =
                serde_json::from_str(c.fields[last]["audit"].as_str().unwrap()).unwrap();
            terminal[field] = if field == "key_sources" {
                json!([{"container":777}])
            } else if matches!(field, "allocation_complete" | "clock_paused") {
                json!(false)
            } else {
                json!(true)
            };
            c.fields[last]["audit"] = json!(terminal.to_string());
            assert!(!c.classified(), "later terminal {nexus} {field}");
        }
        let mut c = if nexus {
            nexus_context_with(|_| {})
        } else {
            aftermath_context()
        };
        c.fields.pop();
        assert!(!c.classified(), "missing terminal authority is UNKNOWN");
        let mut c = if nexus {
            nexus_context_with(|_| {})
        } else {
            aftermath_context()
        };
        let last = c.fields.len() - 1;
        c.fields.swap(last, last - 1);
        assert!(
            !c.classified(),
            "a late ACK cannot backfill the terminal boundary"
        );
    }
}
