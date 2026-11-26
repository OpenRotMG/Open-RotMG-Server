use super::*;

pub(super) fn initial_states(d: &crate::content::game::Definition) -> Vec<u32> {
    let mut path = vec![];
    let mut id = d.root;
    loop {
        path.push(id);
        match d.states[id as usize].children.first() {
            Some(child) => id = *child,
            None => break,
        }
    }
    path
}
impl World {
    pub(super) fn xml_activations(
        &self,
        id: i32,
    ) -> impl Iterator<Item = (&Scope, &BTreeSet<u32>)> {
        let scopes = self.xml_actor_scopes(id);
        (self.xml.active.range(scopes.clone())).chain(self.xml.listening.range(scopes))
    }
    pub(super) fn xml_activations_mut(
        &mut self,
        scope: Scope,
    ) -> &mut BTreeMap<Scope, BTreeSet<u32>> {
        if scope.order.is_none() {
            &mut self.xml.active
        } else {
            &mut self.xml.listening
        }
    }
    pub(super) fn xml_actor_scopes(&self, id: i32) -> std::ops::Range<Scope> {
        let floor = |actor| Scope {
            identity: Identity {
                world: self.xml.world,
                actor,
                parent: None,
                encounter: None,
            },
            issuer: i32::MIN,
            order: None,
            listener: None,
            generation: 0,
        };
        floor(id)..floor(id.checked_add(1).expect("bounded actor ID"))
    }
    pub(super) fn xml_revoke_scopes(&mut self, dead: impl Fn(&Scope) -> bool) -> Vec<(i32, u32)> {
        let conditions: Vec<_> = self
            .xml
            .conditions
            .iter()
            .filter(|(t, _)| dead(&t.scope))
            .map(|(t, c)| (*t, c.clone()))
            .collect();
        for (token, condition) in conditions {
            self.xml_condition(token, &condition, false)
                .expect("scoped condition cleanup");
        }
        self.xml.motions.retain(|t, _| !dead(&t.scope));
        self.xml.protectors.retain(|t, _| !dead(&t.scope));
        self.xml.deadlines.retain(|t, _| !dead(&t.scope));
        self.xml.auras.retain(|(t, _, _), _| !dead(&t.scope));
        // Own states (order None) only die with their actor, which xml_forget removes by range.
        self.xml.listening.retain(|scope, _| !dead(scope));
        self.xml_refresh_auras();
        for actor in self.xml.visible.clone() {
            self.xml_refresh_interest(actor)
                .expect("scoped interest cleanup");
        }
        self.xml
            .orders
            .iter()
            .filter(|(_, o)| dead(&o.scope))
            .map(|(key, _)| *key)
            .collect()
    }
    pub(super) fn xml_withdraw_orders(&mut self, mut pending: Vec<(i32, u32)>) {
        while let Some((issuer, local)) = pending.pop() {
            if let Some(offer) = self.xml.orders.remove(&(issuer, local)) {
                pending.extend(self.xml_revoke_scopes(|s| {
                    s.issuer == issuer && s.order == Some(local) && s.generation == offer.generation
                }));
            }
        }
    }
    pub(super) fn xml_program(
        &self,
        scope: Scope,
    ) -> Option<&Arc<crate::content::game::Definition>> {
        if let Some(order) = scope.order {
            let published = self.xml.orders.get(&(scope.issuer, order))?;
            return (published.generation == scope.generation).then_some(&published.definition);
        }
        let kind = self.entities.get(&scope.issuer)?.kind;
        self.xml.definitions.get(&kind)
    }
    pub(super) fn xml_refresh_interest(&mut self, id: i32) -> Result<()> {
        let initial = if self.xml.unstarted.contains(&id) {
            self.entities
                .get(&id)
                .and_then(|e| self.xml_game().definitions.get(&e.kind))
                .is_some_and(|d| {
                    initial_states(d).iter().any(|state| {
                        d.states[*state as usize]
                            .behaviors
                            .iter()
                            .any(|b| b.kind == K::RemainVisible)
                    })
                })
        } else {
            false
        };
        let active = self.xml_activations(id).any(|(scope, states)| {
            self.xml_program(*scope).is_some_and(|d| {
                states.iter().any(|state| {
                    d.states[*state as usize]
                        .behaviors
                        .iter()
                        .any(|b| b.kind == K::RemainVisible)
                })
            })
        });
        if initial || active {
            self.xml.visible.insert(id);
        } else {
            self.xml.visible.remove(&id);
        }
        Ok(())
    }
}
