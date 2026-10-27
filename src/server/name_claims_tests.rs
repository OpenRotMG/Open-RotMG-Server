use super::*;

pub(crate) async fn departure_gate(a: &App, account: i32) -> Result<()> {
    let gate = {
        let mut gate = a.persona_departure_gate.lock().unwrap();
        if gate
            .as_ref()
            .is_some_and(|(target, _, _)| *target == account)
        {
            gate.take()
        } else {
            None
        }
    };
    if let Some((_, entered, release)) = gate {
        entered
            .send(())
            .map_err(|_| anyhow::anyhow!("departure test observer closed"))?;
        release.await.context("departure test gate closed")?;
    }
    Ok(())
}
