use super::*;
impl World {
    pub(super) fn bind_ghost_ship(&mut self, ship: i32, anchor: i32) -> Result<()> {
        self.landmark_bind(ship)?;
        self.landmark_bind(anchor)?;
        // GhostAnchor MakeShip (actors.xml:11753) emits on entry and owns at most one ship: the reserved ship is that child.
        ensure!(self.entities.contains_key(&ship), "Ghost Ship absent");
        self.set_parent(ship, Some(anchor));
        Ok(())
    }
}
