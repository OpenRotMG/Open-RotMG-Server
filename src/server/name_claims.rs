use super::*;

// Callers hold name_claims through Store commit/publication; autonomous admission takes the same lock.
pub(super) async fn prepare(
    a: &App,
    account: i32,
    name: String,
    allow_paid: bool,
) -> Result<std::result::Result<Vec<i32>, &'static str>> {
    let stopped = match a
        .store
        .prepare_name_choice(account, name, allow_paid)
        .await?
    {
        Ok(bots) => bots,
        Err(reason) => return Ok(Err(reason)),
    };
    for bot in &stopped {
        a.stop_persona(*bot).await.map_err(|error| {
            anyhow::anyhow!(
                "Could not release persona account {bot}; name was not claimed: {error:#}"
            )
        })?;
    }
    Ok(Ok(stopped))
}
