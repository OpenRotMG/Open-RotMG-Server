import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import assert from 'node:assert/strict';
import {randomUUID} from 'node:crypto';
const root=process.argv[process.argv.indexOf('--run-dir')+1];
export const fixture={class:0x31e,level:20,seed:869,world:'undead-lair',worldSeed:12351,position:'spawn',equipment:[0xa00,-1,-1,-1],backpack:[0xb0b,0xa03,0xc01,0xc05,-1,-1,-1,-1],stats:[770,252,50,40,50,50,75,50]};
export async function run(game){
  const config=JSON.parse(await readFile(join(root,'native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const initial=await game.inspect();
  const response=await fetch(config.origin+'/dev/map?player='+initial.diagnostic.player,{headers:{Authorization:'Bearer '+token},signal:AbortSignal.timeout(10000)});assert(response.ok);
  const boss=(await response.json()).entities.find(e=>e.kind===0xd90);assert(boss);
  const {worldSeed,...knight}=fixture;
  game=await game.join({...knight,world:initial.view.session.world,position:{x:boss.pos.x-1,y:boss.pos.y}});
  let protectedUntil=0;
  async function protect(){
    const now=(await game.observe()).game_ms;if(now<protectedUntil)return;
    const protection=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}}),signal:AbortSignal.timeout(10000)});
    const receipt=await protection.json();assert(protection.ok&&receipt.status==='succeeded',JSON.stringify(receipt));protectedUntil=now+60000;
  }
  // Protection cannot be reapplied while active: finish its window idle, then renew it before the next action.
  async function act(input,capture){
    const now=(await game.observe()).game_ms;
    if(now<protectedUntil&&protectedUntil-now<input.duration_ms)await game.act({keys:[],duration_ms:Math.max(50,protectedUntil-now)});
    await protect();const result=await game.act(input,capture);
    // Septavius Pestilence projectile1 inflicts Quiet, which drains mana (objects.xml); remember it per item.
    quiet||=Boolean((await game.observe()).character.conditions&2);return result;
  }
  let quiet=false;
  // XML Septavius (actors.xml:9725) wanders and cycles Invulnerable substates, so follow it and fire across windows.
  async function close(range){
    for(let n=0;n<30;n++){
      const s=await game.inspect(),e=s.diagnostic.entities.find(e=>e.id===boss.id);assert(e);
      const dx=e.pos.x-s.view.position.x,dy=e.pos.y-s.view.position.y;if(Math.hypot(dx,dy)<=range)return;
      const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2;
      await act({keys:[Math.abs(x)>.15?(x>0?'d':'a'):null,Math.abs(y)>.15?(y>0?'s':'w'):null].filter(Boolean),duration_ms:150});
    }
    throw Error('Native approach to the wandering Septavius failed');
  }
  await protect();
  await act({keys:[],duration_ms:600});
  const start=await game.observe(),observations=[];
  for(const [i,item] of [0xa00,0xb0b,0xa03,0xc01,0xc05].entries()){
    if(i){
      await act({keys:[],duration_ms:350,drag:{from:{x:634+44*(i-1),y:428},to:{x:634,y:385}}});
      game.check(item.toString(16)+' equips for native combat',(await game.observe()).character.items[0]===item);
      await protect();
    }
    quiet=false;let before=await game.inspect(),target=before.diagnostic.entities.find(e=>e.id===boss.id);assert(target);
    for(let wait=0;(before.view.objects.find(e=>e.id===boss.id).condition_mask&(1<<24))&&wait<12;wait++){
      await act({keys:[],duration_ms:500});before=await game.inspect();target=before.diagnostic.entities.find(e=>e.id===boss.id);assert(target);
    }
    let after,enemy;
    for(let aim=0;aim<24;aim++){
      await close(2);
      await act({keys:[],target:{entity:boss.id,camera_angle:7*Math.PI/4,fire:true},duration_ms:500},item===0xc01&&aim===0?{name:'demon-damage',when:view=>view.verification.shots>before.diagnostic.character.shots}:undefined);
      after=await game.inspect();enemy=after.diagnostic.entities.find(e=>e.id===boss.id);assert(enemy);
      if(enemy.hp<target.hp)break;
    }
    observations.push({item,before,after,quiet});
    await writeFile(join(root,'sword-damage.json'),JSON.stringify(observations,null,2));
    game.check(item.toString(16)+' damages a natural enemy through native fire',enemy.hp<target.hp&&after.diagnostic.character.shots>before.diagnostic.character.shots);
    // Quiet drains MP (world.rs::apply_condition); passive Wisdom regeneration can increase it after the drain.
    game.check(item.toString(16)+' spends no mana outside Quiet and leaves inventory unchanged',
      (quiet||(before.view.character.conditions&2)!==0||after.view.character.mp>=before.view.character.mp)&&JSON.stringify(after.view.character.items)===JSON.stringify(before.view.character.items));
    game.check(item.toString(16)+' retains the same living Knight',after.view.lifecycle.alive&&after.view.session.character===start.session.character);
  }
}
