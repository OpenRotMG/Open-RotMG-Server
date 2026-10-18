use super::*;

impl Store {
    pub async fn consume_xp_booster(
        &self,
        write: InventoryWrite,
        slot: Option<usize>,
        vault: Option<Vault>,
    ) -> Result<i32> {
        ensure!(
            write.items.len() == 12
                && write.items.iter().all(|item| (-1..=65535).contains(item))
                && slot.is_none_or(|slot| write.items.get(slot) == Some(&-1))
                && (0..i32::MAX).contains(&write.revision)
                && vault
                    .as_ref()
                    .is_none_or(|v| slot.is_none() && v.account == write.account),
            "invalid XP booster reservation"
        );
        self.call(move |db| {
            let tx = db.transaction()?;
            let (raw, items): (String, String) = tx.query_row(
                "SELECT stats,items FROM characters WHERE id=?1 AND account=?2 AND revision=?3 AND alive=1",
                params![write.id, write.account, write.revision], |r| Ok((r.get(0)?, r.get(1)?)))
                .context("XP booster conflict: dead, foreign or stale character")?;
            let saved: Character = serde_json::from_str(&raw)?;
            ensure!((1..20).contains(&saved.level) && !saved.xp_boost.active
                && (0..=saved.exp).contains(&saved.xp_boost.bonus_exp), "invalid or already active XP boost state");
            let mut previous: Vec<i32> = serde_json::from_str(&items)?;
            ensure!(previous.len() == 12, "invalid XP booster inventory");
            if let Some(slot) = slot {
                ensure!(previous[slot] == 0xc42, "XP booster item reservation changed");
                previous[slot] = -1;
            }
            ensure!(previous == write.items, "XP booster may only consume its reserved slot");
            let boost = XpBoost { active: true, ..saved.xp_boost };
            let n = tx.execute("UPDATE characters SET stats=json_set(stats,'$.xp_boost',json(?5)),items=?4,revision=revision+1 WHERE id=?1 AND account=?2 AND revision=?3 AND alive=1",
                params![write.id, write.account, write.revision, serde_json::to_string(&write.items)?, serde_json::to_string(&boost)?])?;
            ensure!(n == 1, "XP booster commit lost its reservation");
            if let Some(vault) = vault {
                let n = tx.execute("UPDATE vaults SET items=?3,revision=revision+1 WHERE account=?1 AND revision=?2",
                    params![vault.account, vault.revision, serde_json::to_string(&vault.items)?])?;
                ensure!(n == 1, "XP booster vault conflict");
            }
            tx.commit()?;
            Ok(write.revision + 1)
        }).await
    }

    pub async fn consume_dye(
        &self,
        write: InventoryWrite,
        slot: usize,
        kind: u16,
        expected: [u32; 2],
    ) -> Result<i32> {
        ensure!(
            write.items.len() == 12
                && slot < 12
                && write.items[slot] == -1
                && write.items.iter().all(|item| (-1..=65535).contains(item)),
            "invalid dye inventory"
        );
        ensure!(
            (0..i32::MAX).contains(&write.revision),
            "invalid dye revision"
        );
        let dyes = self.dyes.clone();
        self.call(move |db| {
            let tx = db.transaction()?;
            let (raw, items): (String, String) = tx.query_row(
                "SELECT stats,items FROM characters WHERE id=?1 AND account=?2 AND revision=?3 AND alive=1",
                params![write.id,write.account,write.revision], |r| Ok((r.get(0)?,r.get(1)?)))
                .context("dye conflict: dead, foreign or stale character")?;
            let mut previous: Vec<i32> = serde_json::from_str(&items)?;
            ensure!(previous.len() == 12 && previous[slot] == i32::from(kind), "dye item reservation changed");
            previous[slot] = -1;
            ensure!(previous == write.items, "dye may only consume its reserved slot");
            let textures = dyes.get(&kind).context("item is not a selected dye or cloth")?;
            let mut saved: Character = serde_json::from_str(&raw)?;
            if let Some(tex1) = textures[0] { saved.tex1 = tex1; }
            if let Some(tex2) = textures[1] { saved.tex2 = tex2; }
            ensure!(expected == [saved.tex1, saved.tex2], "dye appearance reservation changed");
            let n = tx.execute("UPDATE characters SET stats=json_set(stats,'$.tex1',?5,'$.tex2',?6),items=?4,revision=revision+1 WHERE id=?1 AND account=?2 AND revision=?3 AND alive=1",
                params![write.id,write.account,write.revision,serde_json::to_string(&write.items)?,saved.tex1,saved.tex2])?;
            ensure!(n == 1, "dye commit lost its reservation");
            tx.commit()?;
            Ok(write.revision + 1)
        }).await
    }

    pub async fn select_companion(
        &self,
        write: InventoryWrite,
        slot: usize,
        kind: u16,
        selected: Option<u16>,
        content: std::sync::Arc<Content>,
    ) -> Result<i32> {
        ensure!(
            write.items.len() == 12
                && slot < 12
                && write.items.iter().all(|item| (-1..=65535).contains(item)),
            "invalid companion inventory"
        );
        ensure!(
            (0..i32::MAX).contains(&write.revision),
            "invalid companion revision"
        );
        let item = content.get(kind)?;
        let metadata = item
            .companion
            .as_ref()
            .context("generator metadata missing")?;
        ensure!(
            metadata.permanent
                && item.class == "Equipment"
                && item.slot_type == 10
                && item.activations == 1,
            "item is not a companion generator"
        );
        let pet = content.companion_kind(&metadata.object)?;
        let consumable = item.consumable;
        self.call(move |db| {
            let tx = db.transaction()?;
            let (raw,items): (String,String) = tx.query_row(
                "SELECT stats,items FROM characters WHERE id=?1 AND account=?2 AND revision=?3 AND alive=1",
                params![write.id,write.account,write.revision],|r| Ok((r.get(0)?,r.get(1)?)))
                .context("companion conflict: dead, foreign or stale character")?;
            let saved: Character = serde_json::from_str(&raw)?;
            let mut previous: Vec<i32> = serde_json::from_str(&items)?;
            ensure!(previous.len() == 12 && previous[slot] == i32::from(kind), "generator item reservation changed");
            if consumable { previous[slot] = -1; }
            ensure!(previous == write.items,"generator may only mutate its reserved slot");
            let expected = if !consumable && saved.pet_kind == Some(pet) { None } else { Some(pet) };
            ensure!(expected == selected,"companion selection reservation changed");
            let n = tx.execute("UPDATE characters SET stats=json_set(stats,'$.pet_kind',?5),items=?4,revision=revision+1 WHERE id=?1 AND account=?2 AND revision=?3 AND alive=1",
                params![write.id,write.account,write.revision,serde_json::to_string(&write.items)?,selected])?;
            ensure!(n == 1,"companion selection lost its reservation");
            tx.commit()?;
            Ok(write.revision+1)
        }).await
    }
}
