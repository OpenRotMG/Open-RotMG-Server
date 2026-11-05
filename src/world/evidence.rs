use super::*;

pub(super) type View = ([i32; 4], [i32; 8], i32);
impl Player {
    pub(super) fn audit_state(
        &mut self,
        id: i32,
        world: &str,
        now: u64,
        content: &Content,
        phase: &str,
    ) -> Result<()> {
        if self.controller != Controller::Human {
            return Ok(());
        }
        let equipment = self.character.items[..4]
            .try_into()
            .expect("four equipment slots");
        let stats = self.effective(content, now)?;
        let conditions = self.conditions(now);
        let view = (equipment, stats, conditions);
        if self.audit_view != Some(view) {
            self.audit_revision += 1;
            self.audit_fact(id,world,now,"state",||serde_json::json!({"phase":phase,"revision":self.audit_revision,"inventory_revision":self.character.revision,"equipment":equipment,"stats":stats,"conditions":conditions,"timers":self.statuses.iter().map(|(index,(since,until))|serde_json::json!([index,since,until])).collect::<Vec<_>>(),"boosts":self.boosts.iter().map(consumables::Boost::audit).collect::<Vec<_>>()}));
            self.audit_view = Some(view);
        }
        Ok(())
    }
    pub(super) fn audit_hp(&self, id: i32, world: &str, now: u64, reason: &str, before: i32) {
        if before != self.character.hp {
            self.audit_fact(id,world,now,"recovery",||serde_json::json!({"reason":reason,"hp_before":before,"hp_after":self.character.hp,"conditions":self.conditions(now)}));
        }
    }
}
