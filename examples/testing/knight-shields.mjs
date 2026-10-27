import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
import assert from 'node:assert/strict';
const root=process.argv[process.argv.indexOf('--run-dir')+1];
export const fixture={class:0x31e,level:20,seed:768,world:'undead-lair',worldSeed:12351,position:'spawn',equipment:[-1,0xa08,-1,-1],backpack:[0xb22,0xa40,0xc0f,-1,-1,-1,-1,-1],stats:[770,252,50,40,50,50,75,50]};
export async function run(game){
  const config=JSON.parse(await readFile(join(root,'native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const headers={Authorization:'Bearer '+token,'Content-Type':'application/json'};
  const initial=await game.inspect();
  const response=await fetch(config.origin+'/dev/map?player='+initial.diagnostic.player,{headers,signal:AbortSignal.timeout(10000)});assert(response.ok);
  const boss=(await response.json()).entities.find(e=>e.kind===0xd90);assert(boss);
  const {worldSeed,...knight}=fixture;
  game=await game.join({...knight,world:initial.view.session.world,position:{x:boss.pos.x-2,y:boss.pos.y}});
  const observations=[];
  async function vitals(mp){
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers,body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:game.actor,change:{kind:'vitals',hp:770,mp}}}),signal:AbortSignal.timeout(10000)});
    const receipt=await response.json();assert(response.ok&&receipt.status==='succeeded',JSON.stringify(receipt));
  }
  await game.act({keys:[],duration_ms:600});
  const start=await game.observe();
  for(const [slot,item,name,cost,shots,effect,duration,stats] of [
    [-1,0xa08,'wooden-shield',80,1,7,3000,[770,42,50,75,50]],
    [0,0xb22,'colossus-shield',100,5,7,3000,[770,52,50,77,52]],
    [1,0xa40,'snake-skin-shield',100,4,7,3000,[770,47,53,75,50]],
    [2,0xc0f,'ogmur',100,5,27,4000,[830,54,50,75,50]],
  ]){
    if(slot>=0){
      await game.act({keys:[],duration_ms:350,drag:{from:{x:634+44*slot,y:428},to:{x:678,y:385}}});
      game.check(name+' equips through native inventory',(await game.observe()).character.items[1]===item);
    }
    await vitals(250);
    const before=await game.inspect(),old=before.diagnostic.entities.find(e=>e.id===boss.id);assert(old);
    await game.act({keys:[' '],target:{entity:boss.id,camera_angle:7*Math.PI/4,fire:false},duration_ms:300},{name,when:view=>view.character.mp<before.view.character.mp-cost/2});
    const after=await game.inspect(),target=after.diagnostic.entities.find(e=>e.id===boss.id);assert(target);
    observations.push({name,before,after});
    await writeFile(join(root,'shield-observations.json'),JSON.stringify({preparedVitals:true,observations},null,2));
    game.check(name+' spends one mana cost',after.view.character.mp>=before.view.character.mp-cost&&after.view.character.mp<before.view.character.mp-cost+10);
    game.check(name+' admits the selected projectile count',after.diagnostic.character.shots-before.diagnostic.character.shots===shots);
    game.check(name+' applies the selected condition',Object.hasOwn(target.statuses,String(effect)));
    game.check(name+' keeps the selected condition duration',target.statuses[effect][1]-target.statuses[effect][0]===duration);
    game.check(name+' applies equipment bonuses',[0,3,4,6,7].every((stat,i)=>after.view.character.stats[stat]===stats[i]));
    if(effect===27)game.check('Ogmur does not refresh Stunned',JSON.stringify(target.statuses[7])===JSON.stringify(old.statuses[7]));
    await game.act({keys:[],duration_ms:400});
  }
  await game.act({keys:[],duration_ms:4100});
  const expired=await game.inspect(),target=expired.diagnostic.entities.find(e=>e.id===boss.id);assert(target);
  game.check('Armor Broken expires after four seconds',expired.view.time_ms>=observations.at(-1).after.diagnostic.entities.find(e=>e.id===boss.id).statuses[27][1]&&!(expired.view.objects.find(e=>e.id===boss.id).condition_mask&(1<<26)));
  await vitals(0);const poor=await game.inspect();
  await game.act({keys:[' '],target:{entity:boss.id,camera_angle:7*Math.PI/4,fire:false},duration_ms:250});const denied=await game.inspect();
  game.check('insufficient mana admits no shield projectiles',denied.diagnostic.character.shots===poor.diagnostic.character.shots);
  game.check('same living Knight retains Ogmur',denied.view.lifecycle.alive&&denied.view.session.character===start.session.character&&denied.view.character.items[1]===0xc0f);
  await writeFile(join(root,'shield-observations.json'),JSON.stringify({preparedVitals:true,observations,expired,poor,denied},null,2));
}
