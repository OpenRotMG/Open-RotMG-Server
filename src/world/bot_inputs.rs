use super::*;

#[cfg(test)]
mod tests;

#[derive(Clone, Default, Debug, Serialize)]
pub(crate) struct Composition {
    pub phase: u8,
    pub text: String,
}

#[derive(Clone, Default, Debug, Serialize)]
pub(crate) struct Inputs {
    pub composition: Composition,
    pub keys: [i8; 2],
    pub cursor: Option<Pos>,
    pub fire: bool,
    #[serde(skip)]
    waypoint: Option<Pos>,
    #[serde(skip)]
    anchor: Option<Pos>,
    pub blocked: Option<Pos>,
    #[serde(skip)]
    desired: Option<Pos>,
    #[serde(skip)]
    tracking_speed: f32,
}
impl Inputs {
    fn translate(&mut self, position: Pos) {
        if let Some(anchor) = self.anchor {
            for point in [&mut self.cursor, &mut self.desired].into_iter().flatten() {
                point.x += position.x - anchor.x;
                point.y += position.y - anchor.y;
            }
        }
        self.anchor = Some(position);
        self.cursor.get_or_insert(position);
    }
    pub(crate) fn release(&mut self) {
        self.keys = [0, 0];
        self.waypoint = None;
        self.desired = None;
        self.fire = false;
    }
    fn bounded(position: Pos, point: Pos) -> Pos {
        let distance = position.distance(point);
        let scale = (5. / distance).min(1.);
        Pos {
            x: position.x + (point.x - position.x) * scale,
            y: position.y + (point.y - position.y) * scale,
        }
    }
    fn observe(&mut self, position: Pos, movement: Option<Pos>, aim: Option<Pos>, reaction: u64) {
        self.translate(position);
        self.blocked = None;
        self.keys = movement.map_or([0, 0], |p| {
            let heading = (p.y - position.y).atan2(p.x - position.x);
            let octant = (heading / std::f32::consts::FRAC_PI_4).round();
            let heading = octant * std::f32::consts::FRAC_PI_4;
            [heading.cos().round() as i8, heading.sin().round() as i8]
        });
        self.waypoint = movement;
        self.desired = aim.map(|point| Self::bounded(position, point));
        self.fire = aim.is_some();
        // Observation cadence already supplies response latency; tracking adds convergence, not another queued reaction.
        self.tracking_speed = 120. / reaction as f32;
        if aim.is_some() && self.cursor.is_none() {
            self.cursor = Some(position);
        }
    }
    pub(crate) fn aim(&mut self, position: Pos, desired: Pos) -> Pos {
        self.translate(position);
        self.desired = Some(Self::bounded(position, desired));
        *self.cursor.get_or_insert(position)
    }
    fn track(&mut self) {
        if let (Some(cursor), Some(desired)) = (&mut self.cursor, self.desired) {
            let distance = cursor.distance(desired);
            if distance > 0. {
                let fraction = (self.tracking_speed * 0.05 / distance).min(1.);
                cursor.x += (desired.x - cursor.x) * fraction;
                cursor.y += (desired.y - cursor.y) * fraction;
            }
        }
    }
    fn movement(&mut self, position: Pos, step: f32) -> Option<Pos> {
        let waypoint = self.waypoint?;
        let [x, y] = self.keys.map(f32::from);
        let length = x.hypot(y);
        if length == 0.
            || position.distance(waypoint) <= 0.1
            || (waypoint.x - position.x) * x + (waypoint.y - position.y) * y <= 0.
        {
            self.keys = [0, 0];
            self.waypoint = None;
            return None;
        }
        let step = step.min(position.distance(waypoint));
        Some(Pos {
            x: position.x + x / length * step,
            y: position.y + y / length * step,
        })
    }
}
impl Directory {
    pub(crate) fn submit_bot_chat(&mut self, id: i32, text: String, now: Instant) -> Result<()> {
        let world = self.location(id)?;
        let p = &self.worlds[&world].players[&id];
        ensure!(
            p.controller == Controller::Autonomous && p.active() && !p.pending,
            "bot chat source is not ready"
        );
        ensure!(
            !text.trim_start().starts_with('/')
                && p.inputs
                    .as_ref()
                    .is_some_and(|input| matches!(input.composition.phase, 1 | 2)
                        && input.composition.text == text),
            "bot Enter does not match its public composition"
        );
        self.chat(id, text, now)?;
        self.worlds
            .get_mut(&world)
            .expect("chat world admitted")
            .players
            .get_mut(&id)
            .expect("chat player admitted")
            .inputs
            .as_mut()
            .expect("chat inputs admitted")
            .composition = Composition::default();
        Ok(())
    }
}

impl World {
    pub(crate) fn compose_bot_inputs(
        &mut self,
        session: (i32, i32, i32),
        composition: Composition,
    ) -> Result<bool> {
        let p = self
            .players
            .get(&session.0)
            .context("composition player missing")?;
        ensure!(
            (p.character.account, p.character.id) == (session.1, session.2),
            "composition owner replaced"
        );
        if !p.active() || p.pending {
            return Ok(false);
        }
        self.bot_inputs(session, None, None, 1)?;
        self.players
            .get_mut(&session.0)
            .expect("composition player admitted")
            .inputs
            .as_mut()
            .expect("composition virtual input")
            .composition = composition;
        Ok(true)
    }
    pub(crate) fn bot_inputs(
        &mut self,
        session: (i32, i32, i32),
        movement: Option<Pos>,
        aim: Option<Pos>,
        reaction: u64,
    ) -> Result<()> {
        let (id, account, character) = session;
        ensure!(reaction > 0, "invalid virtual input reaction");
        let p = self
            .players
            .get_mut(&id)
            .context("virtual input player missing")?;
        ensure!(
            p.character.account == account
                && p.character.id == character
                && p.controller == Controller::Autonomous,
            "virtual input identity changed"
        );
        ensure!(
            p.active() && !p.pending,
            controller("virtual input player is not ready")
        );
        ensure!(
            movement
                .into_iter()
                .chain(aim)
                .all(|point| point.x.is_finite()
                    && point.y.is_finite()
                    && p.pos.distance(point).is_finite()),
            "nonfinite virtual input point"
        );
        p.bot_goal = None;
        p.bot_aim = None;
        let inputs = p.inputs.get_or_insert_with(Inputs::default);
        let composing = inputs.composition.phase != 0;
        inputs.observe(
            p.pos,
            movement.filter(|_| !composing),
            aim.filter(|_| !composing),
            reaction,
        );
        Ok(())
    }
    pub(super) fn step_bot_inputs(&mut self, id: i32) {
        let p = self
            .players
            .get_mut(&id)
            .expect("virtual input player missing");
        if p.inputs.is_none() && p.controller == Controller::Autonomous && p.active() && !p.pending
        {
            p.inputs = Some(Inputs::default());
        }
        let Some(input) = &mut p.inputs else {
            return;
        };
        if p.paused {
            return;
        }
        if p.controller != Controller::Autonomous
            || !p.character.alive
            || p.death_pending
            || p.departing
            || p.persistence_failure.is_some()
        {
            *input = Inputs::default();
            p.bot_aim = None;
            return;
        }
        input.translate(p.pos);
        if p.pending {
            input.release();
            input.composition = Composition::default();
            p.bot_aim = None;
            return;
        }
        input.track();
        let ground = self
            .map
            .cell(p.pos.x, p.pos.y)
            .expect("player inside map")
            .ground;
        let step = p
            .speed(&self.content, self.now)
            .expect("valid player equipment")
            * self.content.grounds[&ground]
            * 50.;
        let p = self.players.get_mut(&id).unwrap();
        let destination = p.inputs.as_mut().unwrap().movement(p.pos, step);
        if let Some(destination) = destination
            && let Err(error) = self.move_to(id, destination, self.now as i32)
        {
            let input = self.players.get_mut(&id).unwrap().inputs.as_mut().unwrap();
            input.keys = [0, 0];
            input.waypoint = None;
            input.blocked = Some(destination);
            tracing::debug!(player=id,%error,"virtual held movement blocked");
        }
        let p = self.players.get_mut(&id).unwrap();
        p.inputs.as_mut().unwrap().translate(p.pos);
        let input = p.inputs.as_ref().unwrap();
        p.bot_aim = input
            .cursor
            .filter(|_| input.fire && p.conditions(self.now) & crate::content::STUNNED == 0)
            .map(|point| (point.y - p.pos.y).atan2(point.x - p.pos.x));
    }
}
