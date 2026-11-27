use super::*;

pub(in crate::world) struct Blast {
    pub at: u64,
    pub kind: u16,
    pub pos: Pos,
    pub radius: f32,
    pub damage: i32,
    pub effect: Option<(u8, u64)>,
    pub threatened: Vec<(i32, f32)>,
}
pub(super) struct Poison {
    pub player: i32,
    pub kind: u16,
    pub born: u64,
    pub duration: u64,
    pub total: i32,
    pub paid: i32,
}
impl World {
    pub(super) fn xml_attack_target(&self, b: &Behavior, step: Step) -> Result<Option<(i32, Pos)>> {
        let pos = self.xml_scope(step.token.scope)?.pos;
        // TrapTrigger has no range attribute and arms within 8 tiles; only tosses declare minRange.
        let range = if b.kind == K::TrapTrigger {
            8.
        } else {
            n(b, F::Range)
        };
        let min = if matches!(b.kind, K::GrenadeToss | K::ObjectToss) {
            n(b, F::MinRange)
        } else {
            0.
        };
        Ok(self.xml_nearest_hostile_target(pos, min, range))
    }
    pub(super) fn xml_attack(
        &mut self,
        b: &Behavior,
        step: Step,
        rng: &mut rand::rngs::StdRng,
    ) -> Result<ActionOutcome> {
        let id = step.token.scope.identity.actor;
        let e = self.xml_scope(step.token.scope)?;
        let (pos, kind) = (e.pos, e.kind);
        let target = self.xml_attack_target(b, step)?;
        let mut angle = if targeted(b) {
            let Some((_, target)) = target else {
                return Ok(ActionOutcome::Blocked);
            };
            heading(pos, target)
        } else {
            // Explode has no defaultAngle attribute; its volley starts east.
            if b.kind == K::Explode {
                0.
            } else {
                n(b, F::DefaultAngle)
            }
        };
        if matches!(b.kind, K::Shoot | K::Explode) {
            if !objects(b, F::ChildId).is_empty() {
                let outcome = self.xml_spawn(b, step, rng)?;
                if matches!(outcome, ActionOutcome::Applied) {
                    self.xml_shoot_blast(b, id)?;
                }
                return Ok(outcome);
            }
            let Some(Value::Projectile(projectile)) = value(b, F::ProjectileId) else {
                bail!("missing linked projectile")
            };
            let count = n(b, F::NumShots) as usize;
            ensure!(count <= 255, "native volley count exceeds wire limit");
            if count == 0 || self.bullets.len() + count > 2048 {
                return Ok(ActionOutcome::Blocked);
            }
            let spec = &self.content.objects[&kind].projectiles[projectile];
            if let Some((player, p)) = target {
                let factor = if b.kind == K::Shoot {
                    n(b, F::Predictive)
                } else {
                    0.
                };
                let speed = f64::from(spec.speed) / 10.;
                if factor > 0.
                    && speed > 0.
                    && let Some((at, v)) = self
                        .players
                        .get(&player)
                        .map(|p| (p.motion.at, p.motion.velocity))
                    && self.now.saturating_sub(at) <= 200
                {
                    let time = f64::from(pos.distance(p)) / speed;
                    angle = heading(
                        pos,
                        Pos {
                            x: p.x + (f64::from(v.x) * time * factor) as f32,
                            y: p.y + (f64::from(v.y) * time * factor) as f32,
                        },
                    );
                }
                // An absent Shoot arcDegrees leaves the facing unrestricted.
                if b.kind == K::Shoot
                    && targeted(b)
                    && let Some(arc) = opt_n(b, F::ArcDegrees).filter(|arc| *arc > 0.)
                {
                    let motion = self.entities[&id].last_step;
                    let facing = if motion.x != 0. || motion.y != 0. {
                        heading(Pos { x: 0., y: 0. }, motion)
                    } else {
                        n(b, F::DefaultAngle)
                    };
                    let half = arc / 2.;
                    let delta = (angle - facing + std::f64::consts::PI)
                        .rem_euclid(std::f64::consts::TAU)
                        - std::f64::consts::PI;
                    if delta.abs() > half {
                        return Ok(ActionOutcome::Blocked);
                    }
                }
            }
            let gap = if b.kind == K::Explode {
                let arc = n(b, F::ArcDegrees);
                arc / if arc >= std::f64::consts::TAU {
                    count.max(1)
                } else {
                    count.saturating_sub(1).max(1)
                } as f64
            } else {
                n(b, F::Angle)
            };
            let mut shots: Vec<_> = (0..count)
                .map(|i| {
                    (
                        *projectile,
                        (angle + (i as f64 - count.saturating_sub(1) as f64 / 2.) * gap) as f32,
                    )
                })
                .collect();
            if self.entities[&id].conditions(self.now)
                & crate::condition::bit(crate::condition::DAZED)
                != 0
            {
                shots = enemy_shots::dazed(shots);
            }
            self.xml_publish_actor(id)?;
            for packet in self.emit_enemy_shots(id, shots) {
                self.send_enemy_shot(id, packet);
            }
            self.xml_shoot_blast(b, id)?;
        } else if b.kind == K::GrenadeToss {
            if self.xml.blasts.len() >= 128 {
                return Ok(ActionOutcome::Blocked);
            }
            let target = match target {
                Some((_, p)) if targeted(b) => p,
                _ => point(pos, angle, n(b, F::Range)),
            };
            let effect = match value(b, F::Effect) {
                Some(Value::Condition(c)) => Some((index(c), ms(b, F::Duration))),
                None => None,
                other => bail!("invalid grenade condition {other:?}"),
            };
            let at = self
                .now
                .checked_add(1500)
                .context("grenade landing overflow")?;
            self.xml_publish_actor(id)?;
            self.xml_queue_blast(
                kind,
                target,
                n(b, F::Radius) as f32,
                n(b, F::Damage) as i32,
                effect,
                at,
            );
            self.xml_effect_known(4, id, target, Pos { x: 0., y: 0. }, 0xffff0000);
        } else if b.kind == K::TrapTrigger {
            let mut draw = rng.clone();
            let low = n(b, F::MinDamage) as i32;
            let high = n(b, F::MaxDamage) as i32;
            ensure!(low <= high, "inverted trap damage");
            let jitter = n(b, F::PosJitter);
            let center = if jitter > 0. {
                point(
                    pos,
                    draw.gen_range(0.0..std::f64::consts::TAU),
                    jitter * draw.r#gen::<f64>(),
                )
            } else {
                pos
            };
            let variability = n(b, F::RadiusVariability);
            let radius = n(b, F::Radius)
                * if variability > 0. {
                    1. + draw.gen_range(-variability..=variability)
                } else {
                    1.
                };
            ensure!(radius >= 0., "negative trap radius");
            let damage = draw.gen_range(low..=high);
            let targets: Vec<_> = self
                .entities_near(center, radius as f32)
                .filter(|e| e.hp > 0 && self.content.objects[&e.kind].enemy && e.id != id)
                .map(|e| e.id)
                .collect();
            let colors = match value(b, F::Color) {
                Some(Value::Colors(cs)) => cs.as_slice(),
                None => &[0xaa00ff],
                _ => bail!("invalid trap colors"),
            };
            let color = colors[draw.gen_range(0..colors.len())];
            for target in targets {
                self.area_hit(None, target, damage, false, &[])?;
            }
            self.xml_diffuse(id, center, radius as f32, color)?;
            if let Some(sound) = opt_n(b, F::SoundId) {
                self.xml_sound(id, sound as u32)?;
            }
            *rng = draw;
        } else if b.kind == K::Zap {
            let Some((player, p)) = target else {
                return Ok(ActionOutcome::Blocked);
            };
            let total = n(b, F::PoisonTotalDamage) as i32;
            let duration = ms(b, F::PoisonDuration);
            ensure!(
                total == 0 || duration > 0,
                "positive Zap poison requires duration"
            );
            if total > 0 && self.xml.poisons.len() >= 128 {
                return Ok(ActionOutcome::Blocked);
            }
            let effect = match value(b, F::ConditionEffect) {
                Some(Value::Condition(c)) => Some((index(c), ms(b, F::ConditionDuration))),
                None => None,
                _ => bail!("invalid Zap condition"),
            };
            self.xml_hurt_player(player, kind, n(b, F::Damage) as i32, false, effect, None)?;
            if total > 0 {
                self.xml.poisons.push(Poison {
                    player,
                    kind,
                    born: self.now,
                    duration,
                    total,
                    paid: 0,
                });
            }
            self.xml_lightning(id, p, color(b, 0xffffffff))?;
        } else {
            bail!("unexpected attack {:?}", b.kind)
        }
        Ok(ActionOutcome::Applied)
    }
    fn xml_hurt_player(
        &mut self,
        id: i32,
        kind: u16,
        raw: i32,
        piercing: bool,
        effect: Option<(u8, u64)>,
        area: Option<(Pos, f32)>,
    ) -> Result<()> {
        let Some(p) = self.players.get(&id).filter(|p| eligible(p)) else {
            return Ok(());
        };
        let flags = p.conditions(self.now);
        let defense = self.defense(id)?;
        let damage = crate::condition::damage(raw, defense, flags, piercing);
        if let Some((effect, duration)) = effect.filter(|(_, d)| *d > 0) {
            self.apply_condition(id, effect, duration)?;
        }
        let p = self.players.get_mut(&id).expect("preflight player");
        let hp_before = p.character.hp;
        p.character.hp = hp_before.saturating_sub(damage).max(0);
        p.record_bot_damage(self.now, hp_before);
        if let Some((position, radius)) = area {
            p.audit_fact(id, &self.name, self.now, "area_damage", || serde_json::json!({"source_kind":kind,
                "position":[position.x,position.y],"radius":radius,"raw_damage":raw,"damage":damage,
                "conditions":flags,"defense":defense,"hp_before":hp_before,"hp_after":p.character.hp}));
        }
        if p.character.hp == 0 {
            p.death_pending = true;
            p.killer = self.content.objects[&kind].name.clone();
            p.bot_goal = None;
            p.bot_aim = None;
        }
        Ok(())
    }
    pub(in crate::world) fn xml_queue_blast(
        &mut self,
        kind: u16,
        pos: Pos,
        radius: f32,
        damage: i32,
        effect: Option<(u8, u64)>,
        at: u64,
    ) {
        let threatened = self
            .players
            .iter()
            .map(|(id, p)| (*id, p.pos.distance(pos)))
            .filter(|(_, distance)| *distance <= radius)
            .collect();
        self.xml.blasts.push(Blast {
            at,
            kind,
            pos,
            radius,
            damage,
            effect,
            threatened,
        });
    }
    pub(super) fn xml_combat_deadlines(&mut self) -> Result<()> {
        for blast in std::mem::take(&mut self.xml.blasts) {
            if blast.at > self.now {
                self.xml.blasts.push(blast);
                continue;
            }
            for (id, warned) in blast.threatened {
                if let Some(p) = self.players.get(&id) {
                    p.audit_fact(id, &self.name, self.now, "area_threat", || {
                        serde_json::json!({"source_kind":blast.kind,
                        "position":[blast.pos.x,blast.pos.y],"radius":blast.radius,
                        "distance_at_throw":warned,"distance_at_impact":p.pos.distance(blast.pos)})
                    });
                }
            }
            let targets: Vec<_> = self
                .players
                .iter()
                .filter(|(_, p)| eligible(p) && p.pos.distance(blast.pos) <= blast.radius)
                .map(|(id, _)| *id)
                .collect();
            for id in targets {
                self.xml_hurt_player(
                    id,
                    blast.kind,
                    blast.damage,
                    false,
                    blast.effect,
                    Some((blast.pos, blast.radius)),
                )?;
            }
        }
        for mut poison in std::mem::take(&mut self.xml.poisons) {
            let elapsed = self.now.saturating_sub(poison.born).min(poison.duration);
            let due = (i128::from(poison.total) * i128::from(elapsed) / i128::from(poison.duration))
                as i32;
            let amount = due - poison.paid;
            self.xml_hurt_player(poison.player, poison.kind, amount, true, None, None)?;
            poison.paid = due;
            if elapsed < poison.duration && self.players.get(&poison.player).is_some_and(eligible) {
                self.xml.poisons.push(poison);
            }
        }
        Ok(())
    }
    pub(super) fn xml_validate_sound(&self, kind: u16, sound: u32) -> Result<()> {
        validate_sound(&self.content, kind, sound)
    }
    pub(super) fn xml_sound(&mut self, id: i32, sound: u32) -> Result<()> {
        self.xml_validate_sound(
            self.entities.get(&id).context("sound actor absent")?.kind,
            sound,
        )?;
        let sound = u8::try_from(sound).context("soundId exceeds native byte")?;
        self.xml_publish_actor(id)?;
        let mut out = Writer::new();
        out.i32(id).u8(sound);
        for p in self.players.values().filter(|p| {
            p.known.contains(&id)
                && self
                    .entities
                    .get(&id)
                    .is_some_and(|e| e.accessible(p.character.account))
        }) {
            send(p, wire::PLAY_SOUND, out.0.clone());
        }
        Ok(())
    }
}
fn index(c: &Condition) -> u8 {
    match c {
        Condition::Selected(i) => *i,
        Condition::StunImmune => 12,
    }
}

pub(super) fn validate_sound(content: &Content, kind: u16, sound: u32) -> Result<()> {
    let id = u8::try_from(sound).context("soundId exceeds native byte")?;
    ensure!(
        content.get(kind)?.sound_ids.contains(&id),
        "selected sound capability absent object={kind:#x} soundId={id}"
    );
    Ok(())
}
