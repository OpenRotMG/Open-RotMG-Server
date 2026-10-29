# Change password (`access:change-password`, #925)

**Selected123.5.1:** current/new passwords ≥5 characters; confirmation matches. POST: `guid`, `password`, `newPassword`. Only Success replaces cached credentials; Error re-enables unchanged.
https://github.com/OpenRotMG/Open-RotMG-Client/blob/0282b3a48ad0c73abd683572ece4e6c71b0cb055/client.swf
https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Rotmg123.5.1/src/_Q_A_/_ag.as
https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Rotmg123.5.1/src/_00g/WebAccount.as

**Compiled/routed Kithio:** verify→update same account→Success; no handler invalidation. Two additional resources: `Database.cs::Verify` separately SELECTs; `Program.cs::Worker` has five workers, with only a queue lock. Concurrent old-password verification is possible—not observed original behavior.
https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/server/account/changePassword.cs
https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/server/server.csproj
https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/server/IRequestHandler.cs
https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/db/Database.cs
https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/server/Program.cs

**Trapped2013:** related verify/update comparator, not independent corroboration.
https://github.com/trapped/rotmg_svr/blob/593e490803fe0ad168ead408cacd5287d778acfe/server/account/changePassword.cs

**Modified7:** same client gates/fields/success-only update; server verifies→changes. Later/private comparators, not original-backend specifications.
https://github.com/moistosaurus/realm-cli/blob/a13c855dc5e09ee301fd0a66fcd303cfb5dff74e/src/kabam/rotmg/account/web/services/WebChangePasswordTask.as
https://github.com/moistosaurus/realm-cli/blob/a13c855dc5e09ee301fd0a66fcd303cfb5dff74e/src/kabam/rotmg/account/web/view/WebChangePasswordMediator.as
https://github.com/moistosaurus/realm-src/blob/1855c017eab870d13f0b61b1cf0b9e66a01922ff/server/account/changePassword.cs

**Elsewhere:** the2013-10-09 API catalogue lists the same fields plus `ignore`; the support mirror says passwords can change under Accounts, but its claim date is unverified. Neither resolves concurrency/invalidation.
https://github.com/rotmg-tools/rotmg-apidoc/blob/c0e1b52fdfca579854928cd88a31cfd953283544/README.md
https://wikiwiki.jp/rmd/Customer%20Support#support5-1

**2012-01-14 FAQ capture:** logout clears saved logins—not password-change policy. **History partial:** original-server invalidation/concurrency remain unresolved.
https://web.archive.org/web/20120114014404id_/http://forums.wildshadow.com/wiki/realm-mad-god/faq

Serialized authentication, active/pending/fenced ownership refusal, atomic Argon2 replacement and grant revocation are **modern safety**, not recovered history. Identity/Discord persist; no email-change implication.

**PR156:** native success, old-password refusal, preserved state, actual restart/native reload; reused at its own revision, not freshly repeated.
https://github.com/OpenRotMG/Open-RotMG-Server/pull/156#issuecomment-6238787224

**New QA:** `examples/testing/change-password.mjs` exercises native validation/refusal/success and cached-password LOAD; active ownership refusal is an HTTP probe. Prepared progress is not earned progression; API probes are not native login. Production/mail/authority/death/ACK/lease rules unchanged. Exact receipts: #925 and the restoration ledger.
