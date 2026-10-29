# Character deletion: issue 928

`access:character-deletion`, reconciled against `30f27d7390058d44a2691f6dae4b02d1b3748aa5`: verification/research, no demonstrated runtime defect or runtime change.

## Sources and semantics

Selected 123.5.1 `§_-0LC§.§_-0DW§`/`§_-02b§.§_-0FA§`/`appengine.§_-2n§`: named Cancel/Delete confirmation; two retries; `charId`, `reason=1`, credentials; success/error handling. These establish UI/protocol, not original server policy. The bundled reconstruction agrees but is neither independent corroboration nor authenticated official source.
https://openrotmg.org/restoration/reference-data/client-123.5.1.swf
https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Rotmg123.5.1/src/com/company/assembleegameclient/appengine/_2n.as
https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Rotmg123.5.1/src/_0L_C_/_0D_W_.as
https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Rotmg123.5.1/src/_02b/_0F_A_.as

Related comparators, not independent historical votes: Kithio compiles/registers `/char/delete`, authenticates and account-scopes SQL deletion; missing/repeated rows error, without a session guard in that handler or death/Fame awards. Trapped also hard-deletes; its second ExecuteNonQuery/-1 branch differs. Modified 7.0 retains the client flow, but its server uses tokenized account locks (Account in Use on failure), unawaited deletion/alive/dead removals and unconditional Success without an existence check; separate Death awards Fame/Legends. This is not a 123.5.1 specification; asynchronous durability/weaker checks are not imported. No code copied; source priority is not license clearance.
https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/server/server.csproj
https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/server/IRequestHandler.cs
https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/server/char/delete.cs
https://github.com/trapped/rotmg_svr/blob/593e490803fe0ad168ead408cacd5287d778acfe/server/char/delete.cs
https://github.com/moistosaurus/realm-cli/blob/a13c855dc5e09ee301fd0a66fcd303cfb5dff74e/src/kabam/rotmg/characters/deletion/service/DeleteCharacterTask.as
https://github.com/moistosaurus/realm-cli/blob/a13c855dc5e09ee301fd0a66fcd303cfb5dff74e/src/kabam/rotmg/characters/deletion/view/ConfirmDeleteCharacterDialog.as
https://github.com/moistosaurus/realm-src/blob/1855c017eab870d13f0b61b1cf0b9e66a01922ff/server/char/delete.cs
https://github.com/moistosaurus/realm-src/blob/1855c017eab870d13f0b61b1cf0b9e66a01922ff/common/Database.cs#L970-L1025

Two added independent period checks:
1. January 6, 2012 Wild Shadow FAQ supports deleting to free a slot, not persistence/retry/active/Fame policy; its no-Fame paragraph concerns anti-cheat erasure. Earlier than target, so continuity is qualified.
https://web.archive.org/web/20120106105406id_/http://forums.wildshadow.com/wiki/realm-mad-god/faq
2. InsaneFirebat's August 17, 2013 Steam reply describes XP→living Fame, not deletion; it postdates target. Search summaries, mixed-era Japanese FAQ and inaccessible Fandom FAQ did not resolve policy.
https://steamcommunity.com/app/200210/discussions/0/846961716419955523/

## Reconstruction and acceptance

Current authenticated/account-claimed Store deletion retires an owned living row, clears twelve items, advances revision and retains class history; no death receipt/Fame. Missing/dead/foreign/repeated IDs and active sessions refuse: reconstruction/safety policies, not recovered internals.

PR83's October16 native receipt covers Cancel/delete/free slot/replacement movement/active refusal at `9d43d7b7eb74ec3ab804f74a1ba811676b8f2ec7`, before compatibility `64671054`/merge `a59a3842`, not fresh-main acceptance. Prior deletion suites (ownership, stale writes, atomicity, Vault/Fame/history, reopened persistence) were retired by PR258, not current guards.
https://github.com/OpenRotMG/Open-RotMG-Server/pull/83#issuecomment-6218058660

New isolated-account HTTP regression covers credential/ID/ownership/active refusals, simultaneous deletion and repeated/dead/foreign refusals, stale progress/death rejection, Store reopen, preserved Vault/death/Fame/class history and usable replacement. Prepared progression/death/wallet state is not earned gameplay; Store reopen is not cold-process/native restart acceptance. No real-account deletion, mail, hosting or gameplay change.

**Unresolved original 123.5.1 policy:** retry/idempotence responses, active-session enforcement, deleted-character/death/Fame storage/retention. Keep this remainder on the restoration row; comparator disagreement, native acceptance and functional acceptance do not establish historical acceptance.
