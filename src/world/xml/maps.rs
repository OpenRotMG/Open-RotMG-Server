use super::*;
use crate::content::game::geometry::{Geometry, MapBinding, Rotation, Template};

fn template(binding: &MapBinding) -> Result<&Template> {
    let Geometry::Template(t) = &binding.geometry else {
        bail!(
            "PlaceMap {} requires a Template stamp, not full-world geometry",
            binding.name
        )
    };
    ensure!(
        matches!(t.rotation, Rotation::Fixed) && t.sockets.is_empty() && t.pack.is_empty(),
        "PlaceMap {} has unsupported rotation/socket/population features",
        binding.name
    );
    ensure!(
        t.anchor.is_some(),
        "PlaceMap {} needs explicit anchor",
        binding.name
    );
    Ok(t)
}
pub(super) fn preflight_program(
    game: &crate::content::game::GameData,
    d: &crate::content::game::Definition,
) -> Result<()> {
    validate_program(game, d, true)
}
fn validate_program(
    game: &crate::content::game::GameData,
    d: &crate::content::game::Definition,
    require_publishable_route: bool,
) -> Result<()> {
    for b in d
        .states
        .iter()
        .flat_map(|s| &s.behaviors)
        .filter(|b| b.kind == K::PlaceMap)
    {
        (|| {
            let Some(Value::Map(index)) = value(b, F::Map) else {
                bail!("PlaceMap missing linked map")
            };
            let binding = game
                .maps
                .get(*index as usize)
                .context("PlaceMap binding absent")?;
            template(binding)?;
            let context = format!("PlaceMap={}", binding.name);
            for &kind in &binding.objects {
                let d = super::admission::definition(game, kind, &context)?;
                if game.properties[&kind].is_portal() {
                    ensure!(
                        matches!(
                            d.admission_role()?,
                            AdmissionRole::Scripted | AdmissionRole::Native(NativeService::Portal)
                        ),
                        "unsupported physical portal role for kind={kind:#x}"
                    );
                    ensure!(
                        !require_publishable_route
                            || game.portal_routes.get(&kind).is_some_and(|rules| rules
                                .iter()
                                .any(|r| r.destination.can_publish())),
                        "PlaceMap has no linked portal route kind={kind:#x}"
                    );
                } else {
                    super::admission::raw(game, kind, &context)?;
                }
            }
            Ok::<_, anyhow::Error>(())
        })()
        .with_context(|| b.source.to_string())?;
    }
    Ok(())
}
#[derive(Clone, Debug, Serialize, serde::Deserialize)]
struct CellChange {
    x: i32,
    y: i32,
    ground: Option<u8>,
    remove: Vec<i32>,
    object: Option<u16>,
    clear_object: bool,
    before: (u8, Option<u16>, String),
}
/// Placement may have to wait for the actor to move or another interaction to settle.
#[derive(Debug)]
pub(super) struct StampBlocked(&'static str);
impl std::fmt::Display for StampBlocked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for StampBlocked {}
#[derive(Clone, Debug, Serialize, serde::Deserialize)]
pub(super) struct Stamp {
    pos: Pos,
    map: u32,
    changes: Vec<CellChange>,
    births: usize,
}
impl World {
    pub(super) fn xml_prepare_stamp(&self, map: u32, pos: Pos) -> Result<Stamp> {
        self.xml_prepare_stamp_for(map, pos, None)
    }
    fn xml_prepare_stamp_for(&self, map: u32, pos: Pos, owner: Option<i32>) -> Result<Stamp> {
        let binding = self
            .xml_game()
            .maps
            .get(map as usize)
            .context("stamp binding absent")?;
        let t = template(binding)?;
        let anchor = t.anchor.context("validated stamp anchor")?;
        let origin = (
            pos.x.floor() as i32 - anchor[0] as i32,
            pos.y.floor() as i32 - anchor[1] as i32,
        );
        let (width, height) = (
            i32::try_from(t.dimensions[0])?,
            i32::try_from(t.dimensions[1])?,
        );
        ensure!(
            width <= self.map.width && height <= self.map.height,
            "PlaceMap {} cannot fit owning map",
            binding.name
        );
        if origin.0 < 0
            || origin.1 < 0
            || origin.0 > self.map.width - width
            || origin.1 > self.map.height - height
        {
            return Err(
                StampBlocked("stamp extends outside owning map from current position").into(),
            );
        }
        let mut changes = vec![];
        let mut births = 0;
        for (i, index) in t.cells.iter().enumerate() {
            let Some(index) = index else { continue };
            let tile = &t.palette[*index as usize].tile;
            if tile.ground.is_none() && tile.object.is_none() && !tile.clear_object {
                continue;
            }
            let (x, y) = (
                origin.0 + (i as u32 % t.dimensions[0]) as i32,
                origin.1 + (i as u32 / t.dimensions[0]) as i32,
            );
            if !self
                .entities_at(Pos {
                    x: x as f32 + 0.5,
                    y: y as f32 + 0.5,
                })
                .all(|e| !e.reserved || Some(e.id) == owner)
            {
                return Err(StampBlocked("stamp intersects reserved interaction").into());
            }
            let remove = if tile.clear_object || tile.object.is_some() {
                self.entities_at(Pos {
                    x: x as f32 + 0.5,
                    y: y as f32 + 0.5,
                })
                .filter(|e| {
                    self.content.objects[&e.kind].static_object
                        && !self
                            .xml_game()
                            .definitions
                            .get(&e.kind)
                            .is_some_and(|d| d.disposition == Disposition::Active)
                })
                .map(|e| e.id)
                .collect()
            } else {
                vec![]
            };
            if let Some(kind) = tile.object {
                let pos = Pos {
                    x: x as f32 + 0.5,
                    y: y as f32 + 0.5,
                };
                self.xml_preflight_publication_kind_position(kind, pos)?;
                if !self.xml_placeable(kind, pos, &remove, tile.ground)? {
                    return Err(StampBlocked("stamp object geometry blocked").into());
                }
                births += 1;
            }
            if remove.iter().any(|id| self.entities[id].reserved) {
                return Err(StampBlocked("stamp intersects reserved interaction").into());
            }
            changes.push(CellChange {
                x,
                y,
                ground: tile.ground,
                remove,
                object: tile.object,
                clear_object: tile.clear_object,
                before: {
                    let cell = self
                        .map
                        .cell(x as f32 + 0.5, y as f32 + 0.5)
                        .context("stamp cell absent")?;
                    (cell.ground, cell.object, cell.config.clone())
                },
            });
        }
        Ok(Stamp {
            pos,
            map,
            changes,
            births,
        })
    }
    pub(super) fn xml_stamp_capacity(&self, stamp: &Stamp, owner: Identity) -> bool {
        self.xml_live_count() + stamp.births <= ACTOR_LIMIT
            && self.xml_owned_count(owner.actor) + stamp.births <= CHILD_LIMIT
    }
    pub(super) fn xml_apply_stamp(&mut self, stamp: Stamp, owner: Identity) -> Result<()> {
        self.xml_apply_journaled_stamp(stamp, owner, None)
    }
    fn xml_apply_journaled_stamp(
        &mut self,
        stamp: Stamp,
        owner: Identity,
        event: Option<&str>,
    ) -> Result<()> {
        ensure!(
            !stamp.has_portals(self) || event.is_some(),
            "physical portal stamp requires phase4 coordinator receipt"
        );
        self.xml_check_catalog()?;
        ensure!(
            owner == self.xml_identity(owner.actor)?,
            "stamp owner identity mismatch"
        );
        let current = self.xml_prepare_stamp_for(stamp.map, stamp.pos, Some(owner.actor))?;
        ensure!(
            serde_json::to_value(&current)? == serde_json::to_value(&stamp)?,
            "stamp original geometry/objects changed before publication"
        );
        ensure!(
            self.xml_stamp_capacity(&stamp, owner),
            "stamp shared population budget changed"
        );
        let mut ids = self.xml_reserve_ids(stamp.births)?.into_iter();
        for c in stamp.changes {
            let object_changed = c.object.is_some() || c.clear_object;
            for id in c.remove {
                self.remove_entity(id);
            }
            let i = (c.y * self.map.width + c.x) as usize;
            if let Some(ground) = c.ground {
                Arc::make_mut(&mut self.map).cells[i].ground = ground;
                for p in self
                    .players
                    .values_mut()
                    .filter(|p| p.tiles.contains(&(c.x as i16, c.y as i16)))
                {
                    p.tile_updates.push((c.x as i16, c.y as i16, ground));
                }
            }
            if object_changed {
                let cell = &mut Arc::make_mut(&mut self.map).cells[i];
                cell.object = c.object;
                cell.config.clear();
            }
            if let Some(kind) = c.object {
                let id = ids.next().expect("reserved stamp ID");
                let pos = Pos {
                    x: c.x as f32 + 0.5,
                    y: c.y as f32 + 0.5,
                };
                if let Some(event) = event {
                    self.xml_insert_publication(id, kind, pos, event)
                        .expect("preflighted journaled stamp publication");
                } else {
                    self.insert_entity_with_id(id, kind, pos, vec![])
                        .expect("preflighted stamp admission");
                }
                self.xml_own(id, Some(owner.actor), owner.encounter)
                    .expect("preflighted stamp ownership");
                self.xml_activate_publication(id)
                    .expect("preflighted stamp portal activation");
            }
        }
        self.invalidate_sight();
        Ok(())
    }
    pub(crate) fn xml_validate_content(content: &Content) -> Result<()> {
        let game = content
            .game
            .as_ref()
            .context("runtime requires retained validated XML GameData")?;
        for d in game.definitions.values() {
            behavior::validate(d)?;
            for state in &d.states {
                let mut root = state.id;
                while let Some(parent) = d.states[root as usize].parent {
                    root = parent;
                }
                let kinds = if root == d.root {
                    vec![d.kind]
                } else {
                    let order = d
                        .states
                        .iter()
                        .flat_map(|s| &s.orders)
                        .find(|o| o.state == root)
                        .context("disconnected native order program")?;
                    order
                        .targets
                        .clone()
                        .unwrap_or_else(|| game.definitions.keys().copied().collect())
                };
                for b in &state.behaviors {
                    if let Some(sound) = opt_n(b, F::SoundId) {
                        for kind in &kinds {
                            super::combat::validate_sound(content, *kind, sound as u32)
                                .with_context(|| format!("{} soundId", b.source))?;
                        }
                    }
                    native_validate(b)?;
                }
            }
        }
        game.definitions
            .values()
            .try_for_each(|d| validate_program(game, d, false))
    }
}

impl Stamp {
    pub(super) fn has_portals(&self, w: &World) -> bool {
        self.changes
            .iter()
            .filter_map(|c| c.object)
            .any(|kind| w.xml_is_portal(kind))
    }
}
pub(super) struct PendingStamp {
    pub event: crate::store::world_events::Event,
    pub source: serde_json::Value,
    pub stamp: Stamp,
    pub owner: Identity,
    pub published: bool,
    pub next_retry: Instant,
}
pub(super) fn source(step: Step) -> serde_json::Value {
    let s = step.token.scope;
    serde_json::json!({"world":s.identity.world,"actor":s.identity.actor,"parent":s.identity.parent,"encounter":s.identity.encounter,"issuer":s.issuer,"order":s.order,"listener":s.listener,"generation":s.generation,"local":step.token.local,"emissions":step.emissions})
}
impl World {
    pub(super) fn xml_stage_stamp(
        &mut self,
        stamp: Stamp,
        step: Step,
        behavior: &Behavior,
    ) -> Result<ActionOutcome> {
        let owner = step.token.scope.identity;
        let source = source(step);
        let portals = stamp
            .changes
            .iter()
            .filter_map(|c| c.object)
            .filter(|kind| self.xml_is_portal(*kind))
            .map(|kind| Ok((kind, self.xml_portal_intent(kind)?)))
            .collect::<Result<Vec<_>>>()?;
        let event = crate::store::world_events::Event::new(
            "stamp",
            &self.xml_game().fingerprint,
            serde_json::json!({"origin":{"instance":self.xml.instance,"source":source,"kind":self.entities[&owner.actor].kind,"return_route":self.terminal_return_route()},"stamp":stamp,"portals":portals,"behavior":behavior,"frame":{"active_ms":step.active_ms,"age_ms":step.age_ms,"delta_ms":step.delta_ms,"text_choice":step.text_choice}}),
        )?;
        // Fallible portal intents/event encoding precede custody so a genuine error holds no permit.
        if let Err(error) = self.reserve_terminal_actor(owner.actor) {
            tracing::debug!(%error, "PlaceMap custody backpressure; opportunity remains unspent");
            return Ok(ActionOutcome::Blocked);
        }
        self.xml.terminal_leases.get_mut(&owner.actor).unwrap().1 = Some(event.id.clone());
        self.entities
            .get_mut(&owner.actor)
            .context("stamp actor missing")?
            .reserved = true;
        self.xml.stamps.insert(
            owner.actor,
            PendingStamp {
                event,
                source,
                stamp,
                owner,
                published: false,
                next_retry: Instant::now(),
            },
        );
        Ok(ActionOutcome::Pending)
    }
    pub(super) fn finish_stamp_event(
        &mut self,
        record: crate::store::world_events::Record,
    ) -> Result<()> {
        let payload = record.event.native_payload()?;
        let actor = payload["origin"]["source"]["actor"]
            .as_i64()
            .context("stamp actor missing")? as i32;
        ensure!(
            matches!(record.status, 1 | 2)
                && record.event.content == self.xml_game().fingerprint
                && payload["origin"]["instance"] == self.xml.instance
                && payload["origin"]["return_route"] == self.terminal_return_route(),
            "stamp receipt scope/catalogue/route mismatch"
        );
        if !self.xml.stamps.contains_key(&actor) && record.status == 2 {
            return Ok(());
        }
        let pending = self
            .xml
            .stamps
            .get(&actor)
            .context("stamp original custody absent")?;
        ensure!(
            pending.event == record.event,
            "stamp receipt differs from captured source/plan"
        );
        if pending.published {
            return Ok(());
        }
        let stamp = pending.stamp.clone();
        let owner = pending.owner;
        let entity = self
            .entities
            .get(&actor)
            .context("stamp source disappeared")?;
        ensure!(
            payload["origin"]["kind"] == entity.kind
                && serde_json::to_value(entity.pos)? == serde_json::to_value(stamp.pos)?,
            "stamp original source kind/position changed"
        );
        self.xml_apply_journaled_stamp(stamp, owner, Some(&record.event.id))?;
        self.xml.stamps.get_mut(&actor).unwrap().published = true;
        self.entities.get_mut(&actor).unwrap().reserved = false;
        self.xml.terminal_publications.insert(record.event.id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn xml_runtime_world_stamp_commit_portal_fault_preflights_before_other_paint_removal_and_ids() {
        let content = Arc::new(crate::world::test_content());
        let mut world = World::in_domain(
            content.clone(),
            Arc::new(Map {
                width: 128,
                height: 128,
                elevation: Some(vec![200; 128 * 128].into()),
                cells: vec![
                    crate::content::Cell {
                        ground: 0x18,
                        object: None,
                        config: String::new(),
                        terrain: 0,
                        region: 0
                    };
                    128 * 128
                ],
                spawns: vec![(64.5, 64.5)],
            }),
            WorldDomain::Realm,
        )
        .unwrap();
        let owner = world
            .spawn_entity(0x600, Pos { x: 10.5, y: 10.5 }, vec![])
            .unwrap();
        let map = world
            .xml_game()
            .maps
            .iter()
            .position(|m| m.name == "jungle/totem_floor.jm")
            .unwrap() as u32;
        let pos = Pos { x: 64.5, y: 64.5 };
        let stamp = world.xml_prepare_stamp(map, pos).unwrap();
        let wall_pos = Pos {
            x: stamp.changes[0].x as f32 + 0.5,
            y: stamp.changes[0].y as f32 + 0.5,
        };
        let wall = world.spawn_entity(0x120, wall_pos, vec![]).unwrap();
        let before = serde_json::to_vec(&world.map.cells).unwrap();
        let ids = world.ids.load(Ordering::Relaxed);
        let identity = world.xml_identity(owner).unwrap();
        for last in [false, true] {
            let mut stamp = world.xml_prepare_stamp(map, pos).unwrap();
            stamp.changes[0].clear_object = true;
            stamp.changes[0].remove = vec![wall];
            let index = if last { stamp.changes.len() - 1 } else { 0 };
            stamp.changes[index].object = Some(0x741);
            stamp.births += 1;
            let error = format!("{:#}", world.xml_apply_stamp(stamp, identity).unwrap_err());
            assert!(error.contains("requires phase4 coordinator"), "{error}");
            assert_eq!(world.ids.load(Ordering::Relaxed), ids);
            assert_eq!(serde_json::to_vec(&world.map.cells).unwrap(), before);
            assert!(world.entities.contains_key(&wall));
            assert_eq!(world.entities.len(), 2);
        }
        let mut dynamic = world.xml_prepare_stamp(map, pos).unwrap();
        dynamic.changes[0].object = Some(0x181a);
        dynamic.changes[0].ground = Some(255);
        dynamic.births += 1;
        assert!(
            world.xml_apply_stamp(dynamic, identity).is_err(),
            "initial passive-wall geometry is not dynamic stamp permission"
        );
        assert_eq!(serde_json::to_vec(&world.map.cells).unwrap(), before);
        assert_eq!(world.ids.load(Ordering::Relaxed), ids);
        let stamp = world.xml_prepare_stamp(map, pos).unwrap();
        let game = world.content.game.as_ref().unwrap().clone();
        Arc::make_mut(&mut world.content).game = Some(Arc::new(game.as_ref().clone()));
        assert!(world.xml_apply_stamp(stamp, identity).is_err());
        assert_eq!(serde_json::to_vec(&world.map.cells).unwrap(), before);
        assert_eq!(world.ids.load(Ordering::Relaxed), ids);
    }
}
