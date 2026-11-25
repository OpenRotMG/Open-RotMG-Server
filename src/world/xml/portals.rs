use super::*;
use crate::content::game::{PortalDestination, PortalRule};

#[derive(Clone, Debug, PartialEq, Serialize, serde::Deserialize)]
pub(super) struct Intent {
    pub destination: PortalDestination,
    pub earned_exit: bool,
    pub native_lifetime: Option<u64>,
}
pub(super) struct Published {
    pub intent: Intent,
    pub event: String,
    pub expires: Option<u64>,
}
impl World {
    pub(super) fn xml_is_portal(&self, kind: u16) -> bool {
        self.xml_game().properties[&kind].is_portal()
    }
    pub(in crate::world) fn xml_portal_route(&self, kind: u16) -> Result<&PortalRule> {
        self.xml_game()
            .portal_route(kind, self.xml.template.as_deref())
    }
    pub(super) fn xml_portal_intent(&self, kind: u16) -> Result<Intent> {
        let d = self.xml_portal_definition(kind, "journaled physical portal")?;
        let route = self.xml_portal_route(kind)?;
        ensure!(
            route.destination.can_publish(),
            "journaled portal requires a publishable route kind={kind:#x} from={:?}: {:?}",
            self.xml.template,
            route.destination
        );
        Ok(Intent {
            destination: route.destination.clone(),
            earned_exit: route.earned_exit,
            native_lifetime: (d.native_service == Some(NativeService::Portal))
                .then_some(DUNGEON_PORTAL_TICKS * 50),
        })
    }
    pub(super) fn xml_preflight_publication_kind_position(
        &self,
        kind: u16,
        pos: Pos,
    ) -> Result<()> {
        if self.xml_is_portal(kind) {
            let intent = self.xml_portal_intent(kind)?;
            self.xml_preflight_portal_program(kind)?;
            self.xml_preflight_position(kind, pos)?;
            if let Some(duration) = intent.native_lifetime {
                self.now
                    .checked_add(duration)
                    .context("portal publication lifetime overflow")?;
            }
        } else {
            self.xml_preflight_admission(kind, pos)?;
        }
        Ok(())
    }
    pub(super) fn xml_preflight_publication(
        &self,
        kind: u16,
        pos: Pos,
        ignored: &[i32],
        ground: Option<u8>,
    ) -> Result<()> {
        self.xml_preflight_publication_kind_position(kind, pos)?;
        ensure!(
            self.xml_placeable(kind, pos, ignored, ground)?,
            "journaled publication geometry blocked kind={kind:#x} at {},{}",
            pos.x,
            pos.y
        );
        Ok(())
    }
    pub(super) fn xml_insert_publication(
        &mut self,
        id: i32,
        kind: u16,
        pos: Pos,
        event: &str,
    ) -> Result<()> {
        if !self.xml_is_portal(kind) {
            self.insert_entity_with_id(id, kind, pos, vec![])?;
            return Ok(());
        }
        let intent = self.xml_portal_intent(kind)?;
        let expires = intent
            .native_lifetime
            .map(|duration| {
                self.now
                    .checked_add(duration)
                    .context("portal publication deadline overflow")
            })
            .transpose()?;
        self.insert_entity_data(id, kind, pos, vec![])?;
        self.xml.portals.insert(
            id,
            Published {
                intent,
                event: event.to_owned(),
                expires,
            },
        );
        Ok(())
    }
}
impl World {
    pub(super) fn xml_activate_publication(&mut self, id: i32) -> Result<()> {
        if self.xml.portals.contains_key(&id) {
            // Lifecycle tokens must capture the final parent/encounter, not the temporary insertion identity.
            self.xml_prepare_portal_actor(id)?;
            self.xml_activate_portal(id)?;
        }
        Ok(())
    }
}
impl Directory {
    pub(in crate::world) fn bind_journaled_portal(&mut self, origin: i32, id: i32) -> Result<()> {
        let w = &self.worlds[&origin];
        let Some(publication) = w.xml.portals.get(&id) else {
            return Ok(());
        };
        ensure!(
            publication.expires.is_none_or(|at| at > w.now),
            protocol("journaled portal expired")
        );
        let PortalDestination::Instance(template) = publication.intent.destination.clone() else {
            return Ok(());
        };
        if let Some(binding) = self.portals.get(&id) {
            ensure!(binding.origin == origin, "journaled portal origin changed");
            return Ok(());
        }
        self.check_portal_catalog(origin)?;
        let event = publication.event.clone();
        let expires = publication
            .expires
            .map(|at| self.tick + (at - w.now).div_ceil(50));
        let route = self.dungeon_return_from(origin)?;
        let destination=self.instance(&template,None,Some(route)).with_context(||format!("portal={id} event={event} template={template} destination construction failed; portal retained"))?;
        self.portals.insert(
            id,
            Portal {
                origin,
                destination: Some(destination),
                expires,
            },
        );
        tracing::info!(portal=id,%event,%template,destination,"journaled portal bound once on ordinary first use");
        Ok(())
    }
}
