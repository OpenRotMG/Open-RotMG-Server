# XML migration test map

This is a coverage map, not a receipt for the currently built executable. Predecessor replay receipts remain local.

| Invariant | Rust / native anchors |
|---|---|
| Compile and link selected data; reject unresolved objects, fields, embeds, worlds, and maps before admission | `src/content/game/tests.rs`, `production_tests.rs`; `xml-data-authority.mjs` invalid data must refuse startup with source context. |
| Actual XML authority without rebuilding | `xml-data-authority.mjs` source vs changed verifies Stheno cooldown, phases, offspring and loot, plus an ordinary Medusa portal roll and entry. `stheno.mjs` remains a standalone replay. |
| World-scope, lifecycle, clocks and ownership | `src/behavior/tests.rs`, `src/world/xml/{tests,movement_tests,admission_tests}.rs`; `xml_runtime_world_all_production_stamps_preserve_selected_layers` includes portal coordination and nonportal ownership. |
| Exact restart, durable once-only publication and pickup | `src/store/world_events/{tests,recovery_tests}.rs`, `src/world/xml/{journal_tests,review_tests,ab_review_tests}.rs`; `loot-ownership.mjs` verifies observed native isolation, pickup and expiry. |
| Portal and route closure, key refusal without spend | `xml_cutover_invalid_owned_key_route_does_not_reserve_or_spend`, `xml_cutover_linked_portal_routes_govern_publication_transfer_and_earned_exit`, `src/world/xml/{context_tests,admission_tests}.rs`; `realm-portals.mjs`, `dungeon-keys.mjs`. |
| Earned scope and publication | `xml_runtime_world_quest_defeat_credits_qualified_damagers_and_notifies_arrow_holders_once`, `xml_rewards_world_followers_carrying_the_quest_tag_never_credit_the_realm_line`, `xml_rewards_world_store_stheno_damage_loot_portal_pickup_lost_reply_reconnect_restart`; `quest.mjs`, scoped endpoints below. |

## Earned and protected endgame endpoint status

| Case | Scope of the verified predecessor (`7538ebf7`), not a new acceptance claim |
|---|---|
| `xml-earned-journey.mjs` | Prepared Realm start; earns Jungle reward and clear, then real closure transfers the same living character to Castle and ordinary Escape returns it. No Guardian fight. |
| `xml-earned-oryx.mjs` | Separate prepared Castle-approach party defeats both Guardians; the same three living characters enter one Chamber. No earned Oryx defeat or Cellar entry. |
| `horrific-creation-earned.mjs` | Prepared three-Priest party destroys ten coils and defeats Dr Terrible; living contributors earn one durable Lab clear each. Wall release and Creation defeat are not part of this endpoint. |
| `oryx-chamber.mjs`, `wine-cellar.mjs`, `horrific-creation.mjs` | Prepared/protected coverage for boss mechanics, route, reward and Creation wall/vat. Not earned endpoints; Beachzone kill and earned cocktail/clear also remain unverified. |
