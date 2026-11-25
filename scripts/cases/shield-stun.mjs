import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import assert from 'node:assert/strict';
const root=process.argv[process.argv.indexOf('--run-dir')+1];
export const fixture={class:0x31e,level:20,seed:568,world:'undead-lair',worldSeed:12351,position:'spawn',equipment:[-1,0xa0c,-1,-1],backpack:Array(8).fill(-1),stats:[770,252,50,40,50,50,75,50]};
export async function run(game){
  const config=JSON.parse(await readFile(join(root,'native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const headers={Authorization:'Bearer '+token,'Content-Type':'application/json'};
  const initial=await game.inspect();
  const response=await fetch(config.origin+'/dev/map?player='+initial.diagnostic.player,{headers,signal:AbortSignal.timeout(10000)});assert(response.ok);
  const map=await response.json(),boss=map.entities.find(e=>e.kind===0xd90);assert(boss);
  const {worldSeed,...knight}=fixture;
  game=await game.join({...knight,world:initial.view.session.world,position:{x:boss.pos.x-2,y:boss.pos.y}});
  await game.act({keys:[],duration_ms:1000});
  // Septavius changes Triforce states every .2s, including vulnerable 4/5/6 (undeadLair.xml:3ff; actors.xml:9736-9781).
  // Snapshot HP/flags cannot prove the state at a later client hit. Keep the status and suppression checks independent of that phase.
  const before=await game.inspect();
  await game.act({keys:[' '],target:{entity:boss.id,camera_angle:7*Math.PI/4,fire:false},duration_ms:300});
  const hit=await game.inspect(),target=hit.diagnostic.entities.find(e=>e.id===boss.id);
  await game.act({keys:[],duration_ms:500});
  const during=await game.inspect(),current=during.diagnostic.entities.find(e=>e.id===boss.id);
  await writeFile(join(root,'shield-observations.json'),JSON.stringify({before,hit,during},null,2));
  game.check('native shield spends mana',hit.view.character.mp<before.view.character.mp);
  game.check('native shield applies Stunned to Septavius',Object.hasOwn(target.statuses,'7'));
  game.check('observed suppression window is inside the stun deadline',during.view.time_ms<target.statuses['7'][1]);
  game.check('stunned boss fires no new projectiles',current.bullet===target.bullet);
  // Sample beyond the observed Stunned deadline, not just the nominal input hold.
  await game.act({keys:[],duration_ms:target.statuses['7'][1]-during.view.time_ms+1000},{name:'shield-stun-recovery',when:v=>v.game_ms>target.statuses['7'][1]});
  const after=await game.inspect(),resumed=after.diagnostic.entities.find(e=>e.id===boss.id);
  await writeFile(join(root,'shield-observations.json'),JSON.stringify({before,hit,during,after},null,2));
  game.check('boss resumes attacks after stun expires',after.view.time_ms>=target.statuses['7'][1]&&resumed.bullet!==current.bullet);
  game.check('same living Knight',after.view.lifecycle.alive&&after.view.session.character===before.view.session.character);
}
