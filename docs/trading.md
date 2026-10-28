# Trading research: issue 907

Inspected server: `0c00c1baa91f5a6b9dc288ccf0fd787bded4b2be`; research only.

Selected 123.5.1 SHA256: `07ae4fc3ca021a5bf08bbf94f057f7620be18b85e01abd763b3a198382e8fa48`.
https://openrotmg.org/restoration/reference-data/client-123.5.1.swf

`com.company.assembleegameclient.ui.§_-KK§.reset/draw`: `§_-5f§=2999`ms elapsed `getTimer()` countdown; current server: 3000ms pause-excluding guard. `ui.§_-zg§.§_-55§` checks capacity/resets countdown; `ui.§_-eb§.§_-91§` counts backpack slots, follows server tradeability. Menu `§_-0DB§.§_-02p§` has no star/rank gate; server eligibility remains unknown. `net.§_-1f§.§_-Gr§` presents recipient invitations, not requester notices.

Modified 7.0 comparators, not historical authority: `TradePanel.checkTrade`, `TradeInventory.numEmpty`, `TradeButton.reset/draw`.

https://github.com/moistosaurus/realm-cli/blob/a13c855dc5e09ee301fd0a66fcd303cfb5dff74e/src/com/company/assembleegameclient/ui/TradePanel.as
https://github.com/moistosaurus/realm-cli/blob/a13c855dc5e09ee301fd0a66fcd303cfb5dff74e/src/com/company/assembleegameclient/ui/TradeInventory.as
https://github.com/moistosaurus/realm-cli/blob/a13c855dc5e09ee301fd0a66fcd303cfb5dff74e/src/com/company/assembleegameclient/ui/TradeButton.as

Kithio's compiled `Player.RequestTrade`/`World.GetUniqueNamedPlayer` path requires both `NameChosen`, case-insensitive/world-local; no inspected star/rank/distance gate. `Player.Tick` invokes `CheckTradeTimeout`, decrementing 20000ms by elapsed `RealmTime.thisTickTimes`; expires below zero with notice. Requester: `You have sent a trade request to <name>!`. Tradeable: nonempty non-Soulbound slots 4–11. Changes revoke acceptance; mutual acceptance has no settling guard. May auto-equip; lacks atomic durable two-account persistence.
https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/wServer.csproj
https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/realm/entities/player/Player.Trade.cs
https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/realm/World.cs
https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/networking/handlers/AcceptTradePacketHandler.cs

Trapped shares mechanisms, not independent corroboration:
https://github.com/trapped/rotmg_svr/blob/593e490803fe0ad168ead408cacd5287d778acfe/wServer/realm/entities/player/Player.Trade.cs

2012 captures: 10 tiles, registered names, overflow refusal/auto-equip; guest prohibition conflicts with current trades. Earlier 117.5 guide: 15 tiles. Capture dates do not establish 123.5.1 policy.
https://web.archive.org/web/20120216040435id_/http://forums.wildshadow.com/wiki/realm-mad-god/trade-system
https://web.archive.org/web/20120608005457id_/http://forums.wildshadow.com/wiki/realm-mad-god/trade-system
https://wikiwiki.jp/rmd/test/build117.5

Official announcements: July 12, 2011 (117.5) introduces trading/exploit testing; February 13, 2012 (122) improves offer visibility, archived November 9, 2012. Neither resolves range/timing.
https://web.archive.org/web/20110801000000id_/http://forums.wildshadow.com/node/4696
https://web.archive.org/web/20121109134315id_/https://forums.wildshadow.com/node/41768

Current `World::request_trade`: exact-case/world-local, guests allowed, silent 20000ms simulation-time expiry (50ms/step), no human distance limit; backpack-only, no auto-equip; atomic safety guards retained.

Prior revision-bound lifecycle/reconnect, elapsed-clock and requester-feedback receipts; no fresh acceptance:
https://github.com/OpenRotMG/Open-RotMG-Website/pull/39#issuecomment-6319796833
https://github.com/OpenRotMG/Open-RotMG-Server/pull/388#issuecomment-6372822683
https://github.com/OpenRotMG/Open-RotMG-Server/pull/684#issuecomment-6429367786

Unresolved 123.5.1: account/name/rank/case/distance, expiry/notice, item eligibility/auto-equip, server settling/requester wording. Historical acceptance remains partial.
