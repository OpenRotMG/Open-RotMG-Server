# Guest retention: issue 919

Research at `f59986f6446a629e704ad4ccc1ac3cf72d22932f`, scoped to `access:guest-accounts`; no behavior change or historical acceptance.

## Client and comparators

Selected 123.5.1 SWF SHA256: `07ae4fc3ca021a5bf08bbf94f057f7620be18b85e01abd763b3a198382e8fa48` (JPEXS26.3.0). `§_-00g§.WebAccount.internalLoad` reads GUID/password from `RotMG` SharedObject; `modify` flushes them, `clear` replaces identity, `password` maps null to empty, `isRegistered` tests nonempty password. `§_-QA§.§_-0Lh§` submits guid/newGUID/newPassword. These establish local identity, not character storage or server TTL; blocked/cleared storage defeats access. Bundled source agrees but neither independently corroborates nor authenticates the SWF.
https://openrotmg.org/restoration/reference-data/client-123.5.1.swf
https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Rotmg123.5.1/src/_00g/WebAccount.as

Kithio compiles/registers char/list and account/register handlers and compiles HELLO: unknown char/list yields transient AccountId0; HELLO creates missing guests. Database.Register persists guest/regTime; Verify has no age predicate. Registration preserves a valid guest's account ID, otherwise creates an account; its protocol differs. No inspected TTL is not proof against external cleanup or official policy.
https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/server/server.csproj
https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/server/IRequestHandler.cs
https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/db/Database.cs
https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/networking/handlers/HelloPacketHandler.cs
https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/server/account/register.cs

Trapped's related UUID/password and guest/regTime code is not independent evidence.
https://github.com/trapped/rotmg_svr/blob/593e490803fe0ad168ead408cacd5287d778acfe/db/Database.cs

Modified 7.0 uses `OSRotMG`, not `RotMG`; identity persistence/registration does not establish 2012 lifetimes.
https://github.com/moistosaurus/realm-cli/blob/a13c855dc5e09ee301fd0a66fcd303cfb5dff74e/src/kabam/rotmg/account/web/WebAccount.as
https://github.com/moistosaurus/realm-cli/blob/a13c855dc5e09ee301fd0a66fcd303cfb5dff74e/src/kabam/rotmg/account/web/services/WebLoadAccountTask.as
https://github.com/moistosaurus/realm-cli/blob/a13c855dc5e09ee301fd0a66fcd303cfb5dff74e/src/kabam/rotmg/account/web/services/WebRegisterAccountTask.as

## Period evidence

Build124's secondary closure-loss claim concerns October 5, 2012 JST, not its April 1, 2013 modification date (which cannot date the note). All-existing-data loss is conjecture; reproduced official announcement omits guest loss. Not 123.5.1 policy. Current Japanese FAQ mixes eras and labels server-storage/local-GUID architecture supposition.
https://wikiwiki.jp/rmd/version/build124.0
https://wikiwiki.jp/rmd/よくある質問

Two additional checks corroborate cookie-dependent access, not TTL or continuity through 123.5.1:

- January 6, 2012 FAQ capture: autosave/return, cleared/blocked Flash cookies prevent return, registration enables cross-computer play. “Saved in your Flash cookie” is not database architecture.
https://web.archive.org/web/20120106105406id_/http://forums.wildshadow.com/wiki/realm-mad-god/faq
- kinkevin's Bahamut reply: April 18, 2012 00:25:36+08:00, not April 17 thread opening; clearing cookies loses unregistered access, registration preserves warehouse access. Steam-linking question does not establish anonymous Steam semantics.
https://forum.gamer.com.tw/C.php?bsn=20618&snA=76

## Reconstruction and retained evidence

Current `Store::characters/list`→`accounts::guest` retains durable accounts without age checks; `Store::register` atomically adds credentials to the same ID, revoking guest lookup without replacing progress. Reconstruction, not original guarantees; no expiry/cleanup/authentication weakening introduced.

Reuse five original native receipts: tutorial `96b2b3f962c7a481f3abe9a58e64ecc7ed809871`; refusal/conversion/login/trade/reconnect `eb8598893b49e6379aee908c443e0a5471fb7f83`. Earned progress/profile reloads observed; later Nexus transfers and Kitchen entry failed. Preserve fixture/source/runtime limits; guest-auth revocation was source/log-observed, not independently runtime-certified. No fresh-main rerun.
https://github.com/OpenRotMG/Open-RotMG-Server/pull/123#issuecomment-6315882866
https://github.com/OpenRotMG/Open-RotMG-Website/pull/39#issuecomment-6317293824
https://github.com/OpenRotMG/Open-RotMG-Website/pull/39#issuecomment-6319796833

Unresolved: original 123.5.1 server inactivity/expiry/deletion schedule and guarantees; whether build124 closure-loss meant identity replacement or deletion. Client/comparator evidence cannot resolve this. Historical partial; gameplay passed with retained failures, not newly accepted. Track the remainder in the restoration row.
