import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
import assert from 'node:assert/strict';
const root=process.argv[process.argv.indexOf('--run-dir')+1];
export const fixture={class:0x324,level:20,seed:774,world:-2,position:'spawn',equipment:[0xa14,0xae3,-1,-1],backpack:[0xb23,0xc0d,0xc2a,-1,-1,-1,-1,-1],stats:[720,252,65,25,30,75,40,0]};
const distance=(a,b)=>Math.hypot(a.x-b.x,a.y-b.y);
export async function run(game){
  const config=JSON.parse(await readFile(join(root,'native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim(),observations=[];
  async function mana(mp){
    const view=await game.observe();
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:game.actor,change:{kind:'vitals',hp:view.character.hp,mp}}}),signal:AbortSignal.timeout(10000)});
    const receipt=await response.json();assert(response.ok&&receipt.status==='succeeded',JSON.stringify(receipt));
    await game.act({keys:[],duration_ms:600});
  }
  const entities=async kind=>(await game.inspect()).diagnostic.entities.filter(e=>e.kind===kind);
  async function equip(slot,item){
    await game.act({keys:[],duration_ms:350,drag:{from:{x:634+44*slot,y:428},to:{x:678,y:385}}});
    game.check('native swap equips '+item,(await game.observe()).character.items[1]===item);
  }
  async function cast(name,cost,kind,teleport){
    await mana(200);const before=await game.observe();
    await game.act({keys:[' '],mouse:{x:500,y:300},duration_ms:250},{name,when:view=>view.character.mp<before.character.mp-cost/2});
    const after=await game.observe(),objects=await entities(kind);
    observations.push({name,before,after,objects});
    game.check(name+' spends one mana cost',after.character.mp>=before.character.mp-cost&&after.character.mp<before.character.mp-cost+5);
    game.check(name+' teleport policy',teleport?distance(before.position,after.position)>3:distance(before.position,after.position)<0.05);
    game.check(name+' creates expected deployment',objects.length===(name==='dancing-swords'?2:1));
    return {before,after,objects};
  }
  const initial=await game.observe();
  const t0=await cast('t0-decoy',60,0x715,true);
  await game.act({keys:[' '],duration_ms:100});
  game.check('cooldown denies immediate recast',(await entities(0x715)).length===1&&(await game.observe()).character.mp>t0.after.character.mp-5);
  await game.act({keys:['a'],duration_ms:300});
  game.check('native movement resumes after teleport ACK',distance((await game.observe()).position,t0.after.position)>0.5);
  await game.act({keys:[],duration_ms:3000});
  game.check('T0 decoy expires',!(await entities(0x715)).length);
  await equip(0,0xb23);
  const t6=await cast('t6-decoy',85,0x715,true);
  game.check('T6 equip bonuses apply',t6.after.character.stats.vitality===42&&t6.after.character.stats.wisdom===2);
  await game.act({keys:[],duration_ms:3000});
  game.check('T6 decoy outlasts T0', (await entities(0x715)).some(e=>e.id===t6.objects[0].id));
  await game.act({keys:[],duration_ms:2800});
  game.check('T6 decoy expires',!(await entities(0x715)).length);
  await equip(1,0xc0d);
  const swords=await cast('dancing-swords',85,0x715,false);
  game.check('Dancing Swords equip bonuses apply',swords.after.character.stats.max_hp===760&&swords.after.character.stats.attack===69&&swords.after.character.stats.speed===34);
  await game.act({keys:[],duration_ms:800});
  const moving=await entities(0x715);
  game.check('Dancing Swords decoys move',moving.some(e=>distance(e.pos,swords.before.position)>0.5));
  await game.act({keys:[],duration_ms:4300});
  game.check('Dancing Swords decoys expire',!(await entities(0x715)).length);
  await equip(2,0xc2a);
  const bomb=await cast('ghostly-bomb',90,0x1409,true);
  game.check('Ghostly Prism creates no decoy',!(await entities(0x715)).length);
  game.check('Ghostly Prism bomb stays at caster origin',distance(bomb.objects[0].pos,bomb.before.position)<0.05);
  game.check('Ghostly Prism wisdom bonus applies',bomb.after.character.stats.wisdom===3);
  await game.act({keys:[],duration_ms:1000});
  game.check('bomb persists before detonation',(await entities(0x1409)).some(e=>e.id===bomb.objects[0].id));
  await game.act({keys:[],duration_ms:2200});
  game.check('bomb detonates and expires',!(await entities(0x1409)).length);
  await mana(0);const poor=await game.observe();
  await game.act({keys:[' '],duration_ms:250});const denied=await game.observe();
  game.check('insufficient mana cannot teleport or deploy',distance(poor.position,denied.position)<0.05&&!(await entities(0x1409)).length);
  game.check('same living Trickster retains equipment',denied.lifecycle.alive&&denied.session.character===initial.session.character&&denied.character.items[1]===0xc2a);
  await writeFile(join(root,'prism-observations.json'),JSON.stringify({preparedMana:true,observations,poor,denied},null,2));
}
