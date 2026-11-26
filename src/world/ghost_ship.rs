use super::*;
impl World {
    pub(super) fn bind_ghost_ship(&mut self, ship: i32, anchor: i32) -> Result<()> {
        self.landmark_bind(ship)?;
        self.landmark_bind(anchor)?;
        // GhostAnchor MakeShip (actors.xml:11753) emits on entry and owns at most one ship: the reserved ship is that child.
        let e = self.entities.get_mut(&ship).context("Ghost Ship absent")?;
        e.parent.set(Some(anchor), ship, &mut self.timed_ids);
        Ok(())
    }
}
