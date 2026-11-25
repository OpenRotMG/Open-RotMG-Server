import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import assert from 'node:assert/strict';
import {randomUUID} from 'node:crypto';
const root=process.argv[process.argv.indexOf('--run-dir')+1];
export const fixture={class:0x30e,level:20,seed:873,world:'undead-lair',worldSeed:12351,position:{x:14.5,y:120.5},equipment:[0xa97,-1,-1,-1],backpack:[0xb08,0xc03,0xc15,0xc24,-1,-1,-1,-1],stats:[600,200,50,25,30,50,40,30]};
export async function run(game){
  const config=JSON.parse(await readFile(join(root,'native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const initial=await game.inspect();
  const boss=initial.diagnostic.entities.find(e=>e.kind===0xd90);assert(boss);
  let protectedUntil=0;
  async function protect(){
    const now=(await game.observe()).game_ms;if(now<protectedUntil)return;
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}}),signal:AbortSignal.timeout(10000)});
    const receipt=await response.json();assert(response.ok&&receipt.status==='succeeded',JSON.stringify(receipt));protectedUntil=now+60000;
  }
  // Protection cannot be reapplied while active: finish its window idle, then renew it before the next action.
  async function act(input,capture){
    const now=(await game.observe()).game_ms;
    if(now<protectedUntil&&protectedUntil-now<input.duration_ms)await game.act({keys:[],duration_ms:Math.max(50,protectedUntil-now)});
    await protect();return game.act(input,capture);
  }
  // XML Septavius (actors.xml:9725) wanders and cycles Invulnerable substates, so stay close (slow Anatis shots) and fire across windows.
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
  for(const [i,item] of [0xa97,0xb08,0xc03,0xc15,0xc24].entries()){
    if(i){
      await act({keys:[],duration_ms:350,drag:{from:{x:634+44*(i-1),y:428},to:{x:634,y:385}}});
      game.check(item.toString(16)+' equips for native combat',(await game.observe()).character.items[0]===item);
    }
    await protect();
    let before=await game.inspect(),target=before.diagnostic.entities.find(e=>e.id===boss.id);assert(target);
    for(let wait=0;(before.view.objects.find(e=>e.id===boss.id).condition_mask&(1<<24))&&wait<12;wait++){
      await act({keys:[],duration_ms:500});before=await game.inspect();target=before.diagnostic.entities.find(e=>e.id===boss.id);assert(target);
    }
    let after,enemy;
    for(let aim=0;aim<24;aim++){
      await close(3);
      await act({keys:[],target:{entity:boss.id,camera_angle:7*Math.PI/4,fire:true},duration_ms:aim===0?1800:500},aim===0?{name:'damage-'+item.toString(16),when:view=>view.verification.shots>before.diagnostic.character.shots}:undefined);
      after=await game.inspect();enemy=after.diagnostic.entities.find(e=>e.id===boss.id);assert(enemy);
      if(enemy.hp<target.hp)break;
    }
    observations.push({item,before,after});
    await writeFile(join(root,'staff-damage.json'),JSON.stringify(observations,null,2));
    game.check(item.toString(16)+' damages a natural enemy through native fire',enemy.hp<target.hp&&after.diagnostic.character.shots>before.diagnostic.character.shots);
    game.check(item.toString(16)+' leaves inventory and stats unchanged',JSON.stringify(after.view.character.items)===JSON.stringify(before.view.character.items)&&JSON.stringify(after.view.character.stats)===JSON.stringify(before.view.character.stats));
    game.check(item.toString(16)+' retains the same living Wizard',after.view.lifecycle.alive&&after.view.session.character===start.session.character);
  }
}
