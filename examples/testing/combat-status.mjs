import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
const root=process.argv[process.argv.indexOf('--run-dir')+1];
export const fixture={class:0x30e,level:20,seed:789,world:'spider-den',worldSeed:12351,position:{x:75.5,y:16.5},equipment:[0xa97,-1,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,75,25,50,75,40,60]};
const bit=index=>1<<(index-1);
export async function run(game){
  const config=JSON.parse(await readFile(join(root,'native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim(),observations=[];
  async function save(name){const state=await game.inspect();observations.push({name,state});await writeFile(join(root,'combat-observations.json'),JSON.stringify(observations,null,2));return state;}
  async function prepare(change){
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:game.actor,change}}),signal:AbortSignal.timeout(10000)});
    const receipt=await response.json();assert(response.ok&&receipt.status==='succeeded',JSON.stringify(receipt));
  }
  const initial=await save('prepared'),boss=initial.diagnostic.entities.find(e=>e.kind===0x936);assert(boss);
  await prepare({kind:'protection',condition:'invulnerable',duration_ms:15000});
  await game.act({keys:[],duration_ms:2800});
  await game.act({keys:[],duration_ms:1500,target:{entity:boss.id,camera_angle:-Math.PI/4,fire:true}},{name:'combat-invulnerable',when:()=>true});
  const protectedHit=await save('invulnerable-hit'),character=protectedHit.view.character;
  game.check('native Invulnerable blocks HP loss',character.hp===670&&(character.conditions&bit(25)));
  game.check('actual web hits still apply Slowed through Invulnerable',character.conditions&bit(4));
  game.check('ordinary staff fire admits paired projectiles',protectedHit.diagnostic.character.shots-initial.diagnostic.character.shots>=4&&(protectedHit.diagnostic.character.shots-initial.diagnostic.character.shots)%2===0);
  await game.act({keys:[],duration_ms:10000},{name:'combat-damage',when:v=>v.character.hp<670});
  const damaged=await save('unprotected-hit');
  game.check('actual hostile fire reduces HP after protection expires',damaged.view.character.hp<670&&!(damaged.view.character.conditions&bit(25)));
  game.check('ordinary boss Weak overlaps web Slowed',(damaged.view.character.conditions&(bit(3)|bit(4)))===(bit(3)|bit(4)));
  await prepare({kind:'protection',condition:'invincible',duration_ms:10000});
  await game.act({keys:[],duration_ms:6000},{name:'combat-expired',when:v=>!(v.character.conditions&(bit(3)|bit(4)|bit(25)))});
  const expired=await save('expired');
  game.check('overlapping Weak and Slowed expire while Invincible prevents new hits',(expired.view.character.conditions&bit(24))&&!(expired.view.character.conditions&(bit(3)|bit(4)|bit(25))));
  game.check('combat preserves the same living Wizard and staff',expired.view.lifecycle.alive&&expired.view.session.character===initial.view.session.character&&expired.view.character.items[0]===0xa97);
}
