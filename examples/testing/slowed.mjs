import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
const out=process.argv[process.argv.indexOf('--run-dir')+1];
export const fixture={class:0x31d,level:20,seed:937,world:'spider-den',worldSeed:12351,position:{x:60.5,y:34.5},equipment:[0xa00,0xa6b,-1,-1],backpack:[0xc21,-1,-1,-1,-1,-1,-1,-1],stats:[770,252,75,25,50,50,75,50]};
const has=(v,n)=>Boolean(v.character.conditions&(1<<(n-1)));
export async function run(observer){
  const config=JSON.parse(await readFile(join(out,'native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim(),records=[];
  async function prepare(actor,change){
    const r=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor,change}}),signal:AbortSignal.timeout(10000)});
    const result=await r.json();assert(r.ok&&result.status==='succeeded',JSON.stringify(result));
  }
  await prepare(observer.actor,{kind:'protection',condition:'invulnerable',duration_ms:60000});
  const start=await observer.observe(),{worldSeed,...spec}=fixture;
  const game=await observer.join({...spec,world:start.session.world,position:{x:75.5,y:16.5}});
  async function save(label){const s=await game.inspect();records.push({label,state:s});await writeFile(join(out,'slowed.json'),JSON.stringify(records,null,2));return s.view;}
  async function web(){
    for(let n=0;n<30;n++){await game.act({keys:[],duration_ms:500});if(has(await game.observe(),4))return await save('natural-web-hit');}
    throw Error('Natural Arachna web did not apply Slowed');
  }
  async function unprotected(){
    for(let n=0;n<15&&has(await game.observe(),24);n++)await game.act({keys:[],duration_ms:1000});
    assert(!has(await game.observe(),24),'Prepared Invincible did not expire');
  }
  const initial=await save('prepared');
  await web();
  await game.act({keys:[' '],duration_ms:250});
  const slow50=await game.act({keys:['d','s'],duration_ms:500},{name:'slowed-speedy',when:v=>has(v,4)&&has(v,15)});
  const first=await save('slowed-speed-50');
  game.check('natural web Slowed overlaps ordinary Golden Helm Speedy',has(first,4)&&has(first,15)&&slow50.effects.distance>.1);
  await prepare(game.actor,{kind:'stat_modifier',index:4,amount:20,duration_ms:60000});
  await prepare(game.actor,{kind:'protection',condition:'invincible',duration_ms:10000});
  const slow70=await game.act({keys:['a','w'],duration_ms:500});
  const boosted=await save('slowed-speed-70');
  // The selected client returns before the Speed/Speedy branches: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Rotmg123.5.1/src/com/company/assembleegameclient/objects/Player.as
  game.check('Slowed movement persists despite increased effective Speed and Speedy',boosted.character.stats[4]===70&&has(boosted,4)&&has(boosted,15)&&slow70.effects.distance>.1&&slow70.effects.distance<slow50.effects.distance*1.5);
  await game.act({keys:[],duration_ms:3300});
  const expired=await save('expired');
  game.check('three-second web Slowed expires while Invincible prevents new hits',!has(expired,4)&&has(expired,24));
  const recovered=await game.act({keys:['d','s'],duration_ms:500},{name:'slowed-expired',when:v=>!has(v,4)});
  game.check('actual native movement recovers after Slowed expires',recovered.effects.distance>slow70.effects.distance*1.4);
  await unprotected();await web();
  await prepare(game.actor,{kind:'protection',condition:'invincible',duration_ms:10000});
  const before=await game.observe(),step=(keys,duration_ms)=>({actor:game.actor,input:{keys,duration_ms}});
  const cleanse=await game.act([step([' '],250),step(['1'],250),step(['a','w'],500)],{name:'slowed-cleansed',when:v=>!has(v,4)&&has(v,15)&&v.character.items[4]===-1});
  const cast=cleanse.steps[0],cured=cleanse.steps[1];
  game.check('ordinary Holy Water removes fresh Slowed and preserves Speedy',has(cast,4)&&has(cast,15)&&!has(cured,4)&&has(cured,15));
  game.check('Holy Water consumes exactly one inventory revision',cured.character.items[4]===-1&&cured.character.inventory_revision===before.character.inventory_revision+1);
  game.check('native movement resumes after cleansing',cleanse.effects.distance>slow70.effects.distance*1.4);
  await save('cleansed');
  await unprotected();await web();
  await game.act({keys:['Insert'],duration_ms:500});
  for(let n=0;n<8&&(await game.observe()).world!=='Nexus';n++)await game.act({keys:[],duration_ms:500});
  const nexus=await save('nexus');
  game.check('ordinary Nexus escape clears Slowed on the same living character',nexus.map.name==='Nexus'&&!has(nexus,4)&&nexus.lifecycle.alive&&nexus.session.character===initial.session.character);
  records.push({slow50:slow50.effects,slow70:slow70.effects,recovered:recovered.effects,cleanse:cleanse.effects});await writeFile(join(out,'slowed.json'),JSON.stringify(records,null,2));
}
