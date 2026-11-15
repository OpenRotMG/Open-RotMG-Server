import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';

export const fixture={class:0x30e,level:20,seed:862,world:'manor-of-the-immortals',worldSeed:12351,position:info=>{const boss=info.encounter_markers.find(e=>e.kind===0x1720);assert(boss);return {x:boss.position.x+3,y:boss.position.y};},equipment:[0xb08,-1,0xb05,0xac5],backpack:Array(8).fill(-1),stats:[670,385,75,25,50,75,40,60]};
export async function run(game){
  const output=name=>join(process.env.QA_RESULTS_DIR,name),samples=[];
  async function sample(name){const s=await game.inspect();samples.push({name,state:s});await writeFile(output('leech.json'),JSON.stringify(samples));return s;}
  const first=await sample('arrival'),boss=first.diagnostic.entities.find(e=>e.kind===0x1720);assert(boss);
  for(let n=0;n<12;n++){
    const s=await game.inspect(),e=s.diagnostic.entities.find(e=>e.id===boss.id);assert(e);
    if(e.hp<15000)break;
    await game.act({keys:[],duration_ms:100,target:{entity:boss.id,camera_angle:-Math.PI/4,fire:true}});
  }
  await game.act({keys:[],duration_ms:100});
  const wounded=await sample('wounded');assert(wounded.diagnostic.entities.find(e=>e.id===boss.id).hp<15000,'Native staff must wound Ruthven');
  let healed=false;
  for(let n=0;n<30;n++){
    const before=await game.inspect();assert(before.view.character.hp>150,'Prepared Wizard must survive the bounded leech check');
    await game.act({keys:[],duration_ms:100});
    const after=await sample('native-hit-'+n),old=before.diagnostic.entities.find(e=>e.id===boss.id),now=after.diagnostic.entities.find(e=>e.id===boss.id);
    assert(old&&now);
    if(now.hp>old.hp&&after.view.character.hp<before.view.character.hp){healed=true;break;}
  }
  game.check('native acknowledged Ruthven damage heals a previously staff-wounded living owner',healed);
  const report=JSON.parse(await readFile(output('native/result.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(output('native/state/control.token'),'utf8')).trim(),'Content-Type':'application/json'};
  const response=await fetch(report.origin+'/dev/testing',{method:'POST',headers,body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:10000}}})});
  const receipt=await response.json();assert(response.ok&&receipt.status==='succeeded',JSON.stringify(receipt));await writeFile(output('protection.json'),JSON.stringify(receipt));
  for(let n=0;n<30&&(await game.inspect()).diagnostic.own_projectiles.length;n++)await game.act({keys:[],duration_ms:100});
  assert.equal((await game.inspect()).diagnostic.own_projectiles.length,0,'Player shots must retire before the blocked-hit observation');
  const protectedState=await sample('protected');
  await game.act({keys:[],duration_ms:3000},{name:'ruthven-leech',when:()=>true});
  const blocked=await sample('blocked');
  game.check('blocked native attacks neither damage the Wizard nor heal Ruthven',blocked.view.character.hp>=protectedState.view.character.hp&&blocked.diagnostic.entities.find(e=>e.id===boss.id).hp===protectedState.diagnostic.entities.find(e=>e.id===boss.id).hp);
  game.check('leech observation preserves the same living character without clear credit',blocked.view.lifecycle.alive&&blocked.view.session.character===first.view.session.character&&blocked.diagnostic.character.manors===0);
}
