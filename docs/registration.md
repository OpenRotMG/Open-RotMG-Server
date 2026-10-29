# Registration and email confirmation

#920: `access:registration` remains historical **partial**; `access:email-verification`, **known-differences**. Server inspected: `f59986f6`, 2026-10-29. Research only; no new gameplay pass or historical acceptance.

## Selected client

Freshly downloaded 123.5.1 SWF; JPEXS 26.3.0 exports (QA `d41bfcfa` / `1bf6e0cc`). SHA-256: `07ae4fc3ca021a5bf08bbf94f057f7620be18b85e01abd763b3a198382e8fa48`.

https://openrotmg.org/restoration/reference-data/client-123.5.1.swf

- `§_-QA§.§_-0Lh§`: email, five-character-minimum password, matching retype, Terms; sends `register` with guest `guid`, email `newGUID`, `newPassword`, `entrytag`. Credentials change only on success; errors re-enable the form. Client validation is not backend-policy proof.
- `§_-QA§.§_-ak§`: subsequent `verify` authenticates/retrieves the account, not an email token. Neither step requests `sendVerifyEmail`.
- `§_-QA§.§_-0Gd§`: direct-child `VerifiedEmail` presence controls the resend offer; Continue remains available. Authenticated `sendVerifyEmail` sets “Sent...” immediately, not proof of delivery. External confirmation is absent from the SWF.

## Comparators and period evidence

Kithio compiles/routes its resend stub (`<Error>Nope.</Error>`); registration converts guests using customized **username plus separate email**. This is not evidence historical verification was absent. Trapped's matching stub is not independent. Modified7.0 likewise submits the guest GUID and changes credentials only on success: a later comparator, not target mail policy. No implementation imported.

https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/server/account/sendVerifyEmail.cs#L13-L19
https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/server/account/register.cs#L62-L120
https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/server/server.csproj#L83-L86
https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/server/Program.cs#L100-L119
https://github.com/moistosaurus/realm-cli/blob/a13c855dc5e09ee301fd0a66fcd303cfb5dff74e/src/kabam/rotmg/account/web/services/WebRegisterAccountTask.as#L37-L60

Japanese guide revisions **2012-03-22 / 2012-10-09** describe initial email, link-only “Account verified”, and apparent retention/play without confirmation. Revision dates are not authenticated deployment dates; conflicting guest-retention advice is not adopted. **2016-09-10** adds CAPTCHA: do not backdate it, birthday or stronger-password wording. Link-only instructions cannot exclude server challenges. Official FAQ capture **2012-01-14 01:44:04** supplies no initial-email/token policy.

https://wikiwiki.jp/rmd/::cmd/backup?page=アカウント登録&age=6&action=source
https://wikiwiki.jp/rmd/::cmd/backup?page=アカウント登録&age=7&action=source
https://wikiwiki.jp/rmd/::cmd/backup?page=アカウント登録&age=11&action=source
https://web.archive.org/web/20120114014404id_/http://forums.wildshadow.com/wiki/realm-mad-god/faq

## Reconstruction and unresolved #920

`Store::register` transactionally converts the guest without sending mail. PR745's **explicit-only authenticated sending**, 24-hour single-use hashed tokens, rate/budget reservations and explicit browser confirmation without a play gate are modern policy; PR1050 separately adds password recovery. Do not add speculative automatic sending/quota changes.

Existing native refusal/conversion receipts at `eb8598893b49e6379aee908c443e0a5471fb7f83` include later failed Nexus transfers; delivery receipt: `475cd4575be495661d5a1de8f9b3b7c47d7db7f4`. These remain revision-bound, not current-main acceptance; this research creates no missing native check.

https://github.com/OpenRotMG/Open-RotMG-Website/pull/39#issuecomment-6317293824
https://github.com/OpenRotMG/Open-RotMG-Server/pull/745#issuecomment-6441393581
https://github.com/OpenRotMG/Open-RotMG-Server/pull/1050

Unresolved: exact 123.5.1 automatic initial-send/queue/retry/delivery rules; external endpoint, token generation/expiry/replay/challenge policy; continuity of March/October observations into the selected build. No hosting or gameplay change.
