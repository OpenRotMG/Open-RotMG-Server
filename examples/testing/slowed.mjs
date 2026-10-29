import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
const out=process.argv[process.argv.indexOf('--run-dir')+1];
export const fixture={class:0x31d,level:20,seed:937,world:'spider-den',worldSeed:12351,position:{x:60.5,y:34.5},equipment:[0xa00,0xa6b,-1,-1],backpack:[0xc21,-1,-1,-1,-1,-1,-1,-1],stats:[770,252,75,25,50,50,75,60]};
const has=(v,n)=>Boolean(v.character.conditions&(1<<(n-1)));
export async function run(game){
  const config=JSON.parse(await readFile(join(out,'native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim(),records=[];
  async function save(label){const s=await game.inspect();records.push({label,state:s});await writeFile(join(out,'slowed.json'),JSON.stringify(records,null,2));return s.view;}
  async function prepare(change){
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:game.actor,change}}),signal:AbortSignal.timeout(10000)});
    const result=await response.json();assert(response.ok&&result.status==='succeeded',JSON.stringify(result));
  }
  async function place(x,y){await prepare({kind:'teleport',position:{x,y}});await game.act({keys:[],duration_ms:150});}
  async function measure(label,expected){
    const before=await save(label+'-before');
    await game.act({keys:['d','s'],duration_ms:500},{name:label,when:()=>true});
    const after=await save(label+'-after'),distance=Math.hypot(after.position.x-before.position.x,after.position.y-before.position.y);
    records.push({label,distance,expected});await writeFile(join(out,'slowed.json'),JSON.stringify(records,null,2));
    game.check(label+' changes actual native position at the expected rate',Math.abs(distance-expected*.5)<.6);
    return after;
  }
  async function web(){
    await place(75.5,16.5);
    for(let n=0;n<16;n++){await game.act({keys:[],duration_ms:500});if(has(await game.observe(),4))return await save('natural-web-hit');}
    throw Error('Natural Arachna web did not apply Slowed');
  }
  const initial=await save('prepared');
  await prepare({kind:'protection',condition:'invulnerable',duration_ms:60000});
  const normal=4+5.6*50/75;
  await measure('normal-floor',normal);
  const hit=await web();
  game.check('natural zero-damage web applies Slowed through Invulnerable',has(hit,4)&&has(hit,25)&&hit.character.hp===initial.character.hp);
  await game.act({keys:[' '],duration_ms:250});
  game.check('ordinary Golden Helm overlaps Speedy with Slowed',has(await game.observe(),4)&&has(await game.observe(),15));
  await place(60.5,34.5);
  // Slowed returns before Speedy, but still multiplies terrain: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Rotmg123.5.1/src/com/company/assembleegameclient/objects/Player.as
  const slow=await measure('slowed-overrides-speedy',4);
  game.check('Slowed remains active throughout measured movement',has(slow,4)&&has(slow,15));
  await prepare({kind:'stat_modifier',index:4,amount:20,duration_ms:60000});await game.act({keys:[],duration_ms:150});
  const stat=await measure('slowed-overrides-speed-stat',4);
  game.check('prepared Speed change does not remove either condition',has(stat,4)&&has(stat,15));
  await game.act({keys:['1'],duration_ms:250});
  const cured=await save('holy-water');
  game.check('ordinary Holy Water consumes once, clears Slowed and preserves Speedy',cured.character.items[4]===-1&&!has(cured,4)&&has(cured,15));
  await measure('cleansed-speedy-floor',(4+5.6*70/75)*1.5);
  await web();await place(60.5,34.5);
  await game.act({keys:[],duration_ms:6500});
  const expired=await save('expired');
  game.check('Slowed and Speedy expire without cure away from new hits',!has(expired,4)&&!has(expired,15));
  await measure('expired-normal-floor',4+5.6*70/75);
  await web();
  await game.act({keys:['Insert'],duration_ms:500});
  for(let n=0;n<8&&(await game.observe()).world!=='Nexus';n++)await game.act({keys:[],duration_ms:500});
  const nexus=await save('nexus');
  game.check('ordinary Nexus escape clears transient Slowed on the same living character',nexus.map.name==='Nexus'&&!has(nexus,4)&&nexus.lifecycle.alive&&nexus.session.character===initial.session.character);
}
