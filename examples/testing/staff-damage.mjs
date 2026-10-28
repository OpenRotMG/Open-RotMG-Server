import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import assert from 'node:assert/strict';
import {randomUUID} from 'node:crypto';
const root=process.argv[process.argv.indexOf('--run-dir')+1];
export const fixture={class:0x30e,level:20,seed:873,world:'undead-lair',worldSeed:12351,position:'spawn',equipment:[0xa97,-1,-1,-1],backpack:[0xb08,0xc03,0xc15,0xc24,-1,-1,-1,-1],stats:[600,200,50,25,30,50,40,30]};
export async function run(game){
  const config=JSON.parse(await readFile(join(root,'native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const initial=await game.inspect();
  const response=await fetch(config.origin+'/dev/map?player='+initial.diagnostic.player,{headers:{Authorization:'Bearer '+token},signal:AbortSignal.timeout(10000)});assert(response.ok);
  const boss=(await response.json()).entities.find(e=>e.kind===0xd90);assert(boss);
  const {worldSeed,...wizard}=fixture;
  game=await game.join({...wizard,world:initial.view.session.world,position:{x:boss.pos.x-1,y:boss.pos.y}});
  let protectedUntil=0;
  async function protect(){
    const now=(await game.observe()).game_ms;if(now<protectedUntil)return;
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}}),signal:AbortSignal.timeout(10000)});
    const receipt=await response.json();assert(response.ok&&receipt.status==='succeeded',JSON.stringify(receipt));protectedUntil=now+60000;
  }
  await protect();
  await game.act({keys:[],duration_ms:600});
  const start=await game.observe(),observations=[];
  for(const [i,item] of [0xa97,0xb08,0xc03,0xc15,0xc24].entries()){
    if(i){
      await game.act({keys:[],duration_ms:350,drag:{from:{x:634+44*(i-1),y:428},to:{x:634,y:385}}});
      game.check(item.toString(16)+' equips for native combat',(await game.observe()).character.items[0]===item);
    }
    await protect();
    let before=await game.inspect(),target=before.diagnostic.entities.find(e=>e.id===boss.id);assert(target);
    for(let wait=0;(before.view.objects.find(e=>e.id===boss.id).condition_mask&(1<<24))&&wait<12;wait++){
      await game.act({keys:[],duration_ms:500});before=await game.inspect();target=before.diagnostic.entities.find(e=>e.id===boss.id);assert(target);
    }
    let after,enemy;
    for(let aim=0;aim<12;aim++){
      await game.act({keys:[],target:{entity:boss.id,camera_angle:7*Math.PI/4,fire:true},duration_ms:aim===0?1800:500},aim===0?{name:'damage-'+item.toString(16),when:view=>view.verification.shots>before.diagnostic.character.shots}:undefined);
      after=await game.inspect();enemy=after.diagnostic.entities.find(e=>e.id===boss.id);assert(enemy);
      if(enemy.hp<target.hp)break;
    }
    observations.push({item,before,after});
    await writeFile(join(root,'staff-damage.json'),JSON.stringify(observations,null,2));
    game.check(item.toString(16)+' damages a natural enemy through native fire',enemy.hp<target.hp&&after.diagnostic.character.shots>before.diagnostic.character.shots);
    game.check(item.toString(16)+' leaves mana inventory and stats unchanged',after.view.character.mp===before.view.character.mp&&JSON.stringify(after.view.character.items)===JSON.stringify(before.view.character.items)&&JSON.stringify(after.view.character.stats)===JSON.stringify(before.view.character.stats));
    game.check(item.toString(16)+' retains the same living Wizard',after.view.lifecycle.alive&&after.view.session.character===start.session.character);
  }
}
