# Release 7.0 server-behavior reference

Thread: https://www.mpgh.net/forum/thread/1236772-private-things-that-should-be-public

Original attachment (sign-in required when recovered): https://www.mpgh.net/forum/attachment.php?attachmentid=351937&d=1489452985

SlickEashy's March 14, 2017 post claims an earlier accidental Kabam Release 7.0.0 XML/debug-client release, separately attributing C++ files to Swatsec. Provenance and version remain unverified.

## Fingerprint and inspection

- ZIP: `supper priv8 things!!!! (please dont share!!)_mpgh.net.zip`
- Size: **5,958,568 bytes**
- SHA-256: `ddd9597910741725c1fe4425748314fdee33fdb3bdb43e6c453ac2905ddd78b5`

The hash matches the post's original VirusTotal URL: attachment continuity, not authentication or safety. ZIP/inner-RAR integrity checks passed.

The ZIP contains `All of You All.rar`, `behavior.cc` and `behavior.h`. The RAR contains **52 XML files; 35 contain behaviors**, plus documents, client SWF and Windows shortcut. Only XML/C++ text was inspected; **nothing was executed**.

Inspected `dungeons/snakePit.xml`, `dungeons/abyssOfDemons.xml` and `realm/{shore,low,mid,high,mountains}.xml` contain server behaviors—not merely ordinary client data—including nested states, transitions, attacks, movement, summons, invulnerability and loot. C++ registers behavior types; its revision relationship to the XML is unresolved.

## Limits and next evidence

Target **123.5.1** and selected-client/Kithio source priority remain unchanged; Release 7.0 values are not automatically applicable. RealmEye's retrospective provides context, not archive authentication: https://www.realmeye.com/forum/t/guide-to-loot-and-drop-rates/60740

Raw artifacts are **not mirrored**. No redistribution grant is established; repository licensing does not relicense them. Keep originals private pending rights clearance.

DECA's publicly accessible **2017 XML scripting documentation** is a separate, later reference—not proof of 7.0/123.5.1 semantics: https://sites.google.com/decagames.com/xml-documentation/home

Prioritized follow-up:
1. Inspect `BehaviorsAndTransitions.docx` and condition-effect documentation; report findings with internal anchors.
2. Locate contemporaneous announcements/version markers distinguishing release from reupload.
3. Compare representative 123.5.1 enemies against XML and period evidence.
4. Resolve engine semantics: state/transition ordering, cooldowns, loot eligibility/guarantees/caps.
5. Obtain explicit redistribution permission before mirroring.
