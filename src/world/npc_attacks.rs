use super::*;

impl World {
    pub(super) fn npc_flash(&self, id: i32, color: u32, period: f32, duration: f32) {
        let mut out = Writer::new();
        out.u8(15)
            .i32(id)
            .f32(period)
            .f32((duration / period).round())
            .f32(0.)
            .f32(0.)
            .u32(color);
        for p in self.players.values().filter(|p| {
            p.known.contains(&id)
                && self
                    .entities
                    .get(&id)
                    .is_some_and(|e| e.accessible(p.character.account))
        }) {
            send(p, wire::SHOW_EFFECT, out.0.clone());
        }
    }
}
