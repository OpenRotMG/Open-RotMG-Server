use super::*;

fn grants_ground(
    definition: &crate::content::game::Definition,
    states: impl IntoIterator<Item = u32>,
    ground: u8,
) -> bool {
    states
        .into_iter()
        .flat_map(|id| definition.states[id as usize].behaviors.iter())
        .filter(|b| b.kind == K::StayInTerrain)
        .filter_map(|b| match value(b, F::Terrain) {
            Some(Value::Grounds(gs)) => Some(gs),
            _ => None,
        })
        .any(|grounds| grounds.contains(&ground))
}
impl World {
    pub(crate) fn xml_placeable(
        &self,
        kind: u16,
        pos: Pos,
        ignored: &[i32],
        ground: Option<u8>,
    ) -> Result<bool> {
        self.xml_geometry(kind, pos, true, ignored, ground)
    }
    pub(super) fn xml_npc_walkable(&self, scope: Scope, pos: Pos) -> Result<bool> {
        let actor = self.xml_scope(scope)?;
        let allowed = self.map.cell(pos.x, pos.y).is_some_and(|cell| {
            (self.xml.unstarted.contains(&actor.id)
                && self
                    .xml_game()
                    .definitions
                    .get(&actor.kind)
                    .is_some_and(|d| {
                        grants_ground(
                            d,
                            std::iter::successors(Some(d.root), |id| {
                                d.states[*id as usize].children.first().copied()
                            }),
                            cell.ground,
                        )
                    }))
                || self.xml_activations(actor.id).any(|(activation, states)| {
                    self.xml_program(*activation)
                        .is_some_and(|d| grants_ground(d, states.iter().copied(), cell.ground))
                })
        });
        self.xml_geometry(
            actor.kind,
            pos,
            self.content.objects[&actor.kind].flying || allowed,
            &[actor.id],
            None,
        )
    }
    pub(super) fn xml_teleport_endpoint(&self, scope: Scope, pos: Pos) -> Result<bool> {
        ensure!(
            pos.x.is_finite() && pos.y.is_finite(),
            "nonfinite Shadow teleport endpoint"
        );
        if !self.xml_npc_walkable(scope, pos)? {
            return Ok(false);
        }
        let actor = self.xml_scope(scope)?;
        let mut programs = Vec::new();
        if self.xml.unstarted.contains(&actor.id) {
            let d = self.xml_declared_admission(actor.kind, "Shadow initial environment")?;
            programs.push((d, super::interest::initial_states(d)));
        }
        for (activation, states) in self.xml_activations(actor.id) {
            let d = self
                .xml_program(*activation)
                .context("Shadow active environment program absent")?;
            programs.push((d.as_ref(), states.iter().copied().collect()));
        }
        for (d, states) in programs {
            for b in states
                .into_iter()
                .flat_map(|s| &d.states[s as usize].behaviors)
            {
                if b.kind == K::StayAbove {
                    if !super::admission::stay_above_applies(self.xml.domain, b)? {
                        continue;
                    }
                    let altitude = self
                        .map
                        .elevation_at(pos.x, pos.y)
                        .context("Shadow endpoint missing active source elevation")?;
                    if f64::from(altitude) < n(b, F::MinAltitude) {
                        return Ok(false);
                    }
                } else if b.kind == K::StayInTerrain
                    && !self.xml_terrain_matches(b, pos, actor.kind)?
                {
                    return Ok(false);
                } else if b.kind == K::Walk {
                    let Some(Value::Grounds(grounds)) = value(b, F::GroundTypes) else {
                        bail!("Shadow active Walk ground types absent")
                    };
                    if !grounds.contains(
                        &self
                            .map
                            .cell(pos.x, pos.y)
                            .context("Shadow endpoint outside map")?
                            .ground,
                    ) {
                        return Ok(false);
                    }
                }
            }
        }
        Ok(true)
    }
    pub(super) fn xml_geometry(
        &self,
        kind: u16,
        pos: Pos,
        allow_no_walk: bool,
        ignored: &[i32],
        ground: Option<u8>,
    ) -> Result<bool> {
        if !pos.x.is_finite() || !pos.y.is_finite() {
            return Ok(false);
        }
        let Some(cell) = self.map.cell(pos.x, pos.y) else {
            return Ok(false);
        };
        let ground = ground.unwrap_or(cell.ground);
        ensure!(
            self.content.grounds.contains_key(&ground),
            "native geometry references absent ground {ground}"
        );
        if self.content.void_grounds.contains(&ground) {
            return Ok(false);
        }
        self.content.get(kind)?;
        let is_npc = self
            .xml_game()
            .definitions
            .get(&kind)
            .is_some_and(|d| d.disposition == Disposition::Active);
        if self.content.grounds[&ground] <= 0. && !allow_no_walk {
            return Ok(false);
        }
        let loot = crate::world::LOOT_BAGS.contains(&kind);
        let mut under_scenery = false;
        let clear = self
            .entities_at(pos)
            .filter(|e| !ignored.contains(&e.id))
            .all(|e| {
                let o = &self.content.objects[&e.kind];
                // Original loot could rest under a tree or rock a flying/enemy corpse stood over (base refused it too).
                let scenery = loot
                    && o.static_object
                    && o.occupy_square
                    && !o.full_occupy
                    && !o.enemy_occupy_square;
                under_scenery |= scenery;
                // A corpse in terminal custody is leaving; its reservation must not deadlock neighbours' publications.
                (!e.reserved || self.xml.terminal_leases.contains_key(&e.id))
                    && (scenery
                        || !if is_npc {
                            o.enemy_occupy_square || o.full_occupy
                        } else {
                            o.blocks
                        })
            });
        // Such a bag must stay pickable (1.5 tiles) from a walkable neighbour.
        Ok(clear
            && (!under_scenery
                || (-1..=1)
                    .flat_map(|dx| (-1..=1).map(move |dy| (dx, dy)))
                    .any(|(dx, dy)| {
                        let tile = Pos {
                            x: pos.x.floor() + dx as f32 + 0.5,
                            y: pos.y.floor() + dy as f32 + 0.5,
                        };
                        pos.distance(tile) < 1.5 && self.walkable(tile)
                    })))
    }
}
