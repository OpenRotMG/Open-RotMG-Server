import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';

export const fixture={class:0x30e,level:20,seed:926,world:-1,position:{x:12.5,y:164.5},equipment:[0xa97,0xa2e,-1,-1],backpack:[0xa1f,0xa1f,0x1200,0xa4f,-1,-1,-1,-1],stats:[300,220,74,5,20,0,0,20]};
export async function run(game){
  const out=process.env.QA_RESULTS_DIR,config=JSON.parse(await readFile(join(out,'native/native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:game.actor,change:{kind:'vitals',hp:150,mp:120}}})});
  const receipt=await response.json();assert(response.ok&&receipt.status==='succeeded',JSON.stringify(receipt));
  const before=await game.inspect();
  await game.act({keys:[],duration_ms:1000},{name:'persistence-before',when:v=>v.world==='Tutorial'});
  await game.act({keys:['1'],duration_ms:200});
  const grown=await game.observe();
  game.check('native stat potion commits the permanent gain and one consumed slot',grown.character.base_stats[2]===75&&grown.character.items[4]===-1&&grown.character.inventory_revision===before.view.character.inventory_revision+1);
  await game.act({keys:['2'],duration_ms:200});
  const refused=await game.observe();
  game.check('capped stat refusal does not consume the second potion or change revision',refused.character.items[5]===0xa1f&&refused.character.inventory_revision===grown.character.inventory_revision);
  await game.act({keys:['3'],duration_ms:200});
  await game.act({keys:[],duration_ms:350,drag:{from:{x:766,y:428},to:{x:766,y:385}}});
  await game.act({keys:['d'],duration_ms:500,mouse:{x:450,y:300}});
  await game.act({keys:[' '],duration_ms:200});
  const changed=await game.inspect(),ch=changed.diagnostic.character;
  game.check('ordinary cloth and ring input commits appearance and equipment',ch.tex1===0x04000000&&ch.tex2===0&&ch.items[6]===-1&&ch.items[7]===-1&&ch.items[3]===0xa4f);
  game.check('native fire records actual shots and spell leaves an entry mana deficit',ch.lifetime.shots>before.diagnostic.character.lifetime.shots&&ch.mp<120);
  // Saved fields are a client contract, not proof of server cadence: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Rotmg123.5.1/src/com/company/assembleegameclient/appengine/SavedCharacter.as
  await game.act({keys:['Insert'],duration_ms:1200});
  await game.act({keys:[],duration_ms:1500},{name:'persistence-reconnected',when:v=>v.world==='Nexus'});
  const after=await game.inspect(),saved=after.diagnostic.character;
  game.check('ordinary Nexus reconnect replaces player but retains account and character',after.view.map.name==='Nexus'&&after.view.session.player!==before.view.session.player&&after.view.session.account===before.view.session.account&&after.view.session.character===before.view.session.character&&after.view.lifecycle.alive);
  game.check('reconnect preserves class experience level permanent stats equipment and appearance',saved.class===ch.class&&saved.exp===ch.exp&&saved.level===ch.level&&JSON.stringify(saved.base)===JSON.stringify(ch.base)&&JSON.stringify(saved.items)===JSON.stringify(ch.items)&&saved.revision===ch.revision&&saved.tex1===ch.tex1&&saved.tex2===ch.tex2);
  game.check('reconnect retains accumulated records without granting full HP or MP',saved.lifetime.shots===ch.lifetime.shots&&saved.lifetime.tiles>=ch.lifetime.tiles&&saved.hp>=ch.hp&&saved.hp<300&&saved.mp>=ch.mp&&saved.mp<220);
  await game.act({keys:['d'],duration_ms:300});
  game.check('reconnected native character remains playable',(await game.observe()).position.x>after.view.position.x);
  await writeFile(join(out,'persistence-observations.json'),JSON.stringify({preparedLevelStatsItemsAndVitals:true,before,changed,after},null,2));
}
