# Character slots

#927 / `access:character-slots`; inspected server `30f27d73`, 2026-10-29. Historical **partial**; no gameplay-policy change.

Selected123.5.1 `CharacterRectList` consumes server capacity/prices, requires registration/balance, and always draws Buy. `§_-0LC§.§_-0Gy§` sends credentials, not quotes. `§_-Rv§.§_-zL§` dispatches COMPLETE, not death or an explicit save. Neither supplies original capacity/prices/durability timing. Fresh JPEXS export: SHA256 `07ae4fc3ca021a5bf08bbf94f057f7620be18b85e01abd763b3a198382e8fa48`.

https://openrotmg.org/restoration/reference-data/client-123.5.1.swf

Kithio's compiled/registered purchase handler charges flat 100; its database starts capacity 1. Trapped's near-identical flat-100 handler is not independent corroboration. Modified 7.0's client includes a Fame-currency branch, but its server chooses Gold and configured `SlotCost`, not these tiers. These comparators cannot override target definitions or modern safety guards. No reference code imported.

https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/server/account/purchaseCharSlot.cs#L34-L51
https://github.com/moistosaurus/realm-cli/blob/a13c855dc5e09ee301fd0a66fcd303cfb5dff74e/src/kabam/rotmg/account/core/services/BuyCharacterSlotTask.as#L37-L62
https://github.com/moistosaurus/realm-src/blob/1855c017eab870d13f0b61b1cf0b9e66a01922ff/server/account/purchaseCharSlot.cs

Two additional checks: **2011-11-05** WordPress describes one free slot,600 Gold “currently”, usually1000; **2012-03-29** Arqade describes statue→Change→selection. Do not backdate its2015 Options answer. Publication dates are not target-continuity proof. The mutable Japanese guide describes an **August4,2012** offer with600+800 slots/cap14, not authenticated123.5.1 policy. Dated-backup requests returned403; the official-promotion archive returned429 or an unrelated forum index.

https://gamingwithme.wordpress.com/2011/11/05/realm-of-the-mad-god-permadeath-mmo/
https://gaming.stackexchange.com/a/59796
https://wikiwiki.jp/rmd/課金について#biginners

Retain PR154's reconstruction: initial1, cap14; second600/third800/rest1000 Gold. Cap quote1000 remains but purchase is refused. Transaction/ownership/replay fences are modern safeguards; no stale-quote guarantee. Reuse its revision-bound native purchase/creation/selections and refusal/restart receipts. Retired tests are not current-main guards.

https://github.com/OpenRotMG/Open-RotMG-Server/pull/154#issuecomment-6237587188
https://github.com/OpenRotMG/Open-RotMG-Server/pull/154#issuecomment-6238062835

Current departure stores progress before commitment/ownership retirement. The character-slots native case checks inventory/shot persistence at selection, same-character LOAD and continued play; executed evidence belongs in the PR. Prepared state is not earned progression, real payment or production acceptance.

**Unresolved:** independently dated full123.5.1 initial-capacity/cap/tier/sale schedule and original disconnect/autosave flush/ACK/crash timing. Preserve safety fences; historical acceptance remains partial.
