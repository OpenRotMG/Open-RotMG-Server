import assert from 'node:assert/strict';
import {writeFile} from 'node:fs/promises';
import {join} from 'node:path';

export const fixture={class:0x300,level:20,seed:12351,world:'tomb-of-the-ancients',worldSeed:12,position:{x:144.1,y:112.9},equipment:[0xaff,0xae1,0xaf9,0xac8],backpack:Array(8).fill(-1),stats:[720,252,50,25,75,75,40,50]};
export async function run(game){
  const records={scope:'Supplied legal maxed Rogue/gear and initial Sarcophagus-room position, seed12. Ordinary equipped cloak and Nexus input. No attacks, NPC/HP/RNG edits, injected protection, refills or relocation. This is a prepared sentry census, not earned entry, traversal or a healing/combat test.'};
  const persist=()=>writeFile(join(process.env.QA_RESULTS_DIR,'tomb-sentries.json'),JSON.stringify(records,null,2));
  const before=await game.inspect();records.before=before;await persist();
  assert(before.paused&&before.view.time_ms===0&&before.view.map.name==='Tomb of the Ancients');
  const position=before.view.position,distance=(a,b)=>Math.hypot(a.x-b.x,a.y-b.y);
  const sarcs=before.view.objects.filter(o=>o.kind===0xd29&&distance(o.position,position)<2);
  assert.equal(sarcs.length,1,'Prepared position must identify exactly one original Sarcophagus');
  const sarc=sarcs[0],guards=view=>view.objects.filter(o=>(o.kind===0xd2a||o.kind===0xd2b)&&distance(o.position,sarc.position)<10);
  const initial=guards(before.view);assert(initial.length>=8&&initial.length<=12,'Unexpected initial sentry population');assert(initial.every(o=>o.hp===o.max_hp),'Sentry census must start at full health');
  records.original={sarc:sarc.id,guards:initial.map(o=>o.id)};await persist();
  await game.act({keys:[' '],duration_ms:1800},{name:'tomb-sarcophagus-sentries',when:v=>v.world==='Tomb of the Ancients'&&v.lifecycle.alive&&v.game_ms>=1500&&v.character.mp<before.view.character.mp&&(v.character.conditions&(1<<12))!==0});
  const during=await game.inspect();records.during=during;await persist();
  await game.act({keys:['Insert'],duration_ms:300});const after=await game.inspect();records.after=after;await persist();
  const present=guards(during.view),ids=present.map(o=>o.id).sort((a,b)=>a-b);
  game.check('ordinary cloak spends mana without primary shots',(during.view.character.conditions&(1<<12))!==0&&during.view.character.mp<before.view.character.mp&&after.diagnostic.character.shots===0);
  game.check('same living Rogue escapes without kills or Tomb credit',after.view.map.name==='Nexus'&&after.view.lifecycle.alive&&after.view.session.character===before.view.session.character&&after.diagnostic.character.kills===0&&after.diagnostic.character.tombs===0);
  game.check('original Sarcophagus retains eight full-health sentries',initial.length===8&&present.length===8&&JSON.stringify(ids)===JSON.stringify([...records.original.guards].sort((a,b)=>a-b))&&present.every(o=>o.hp===o.max_hp)&&during.view.objects.some(o=>o.id===sarc.id&&o.hp===15000));
}
