import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
import assert from 'node:assert/strict';
const root=process.argv[process.argv.indexOf('--run-dir')+1];
export const fixture={class:0x322,level:20,seed:772,world:'snake-pit',worldSeed:12351,position:'spawn',equipment:[-1,0xab1,-1,-1],backpack:[0xab6,0xb2c,0xc1c,-1,-1,-1,-1,-1],stats:[700,252,75,25,50,50,40,50]};
export async function run(game){
  const config=JSON.parse(await readFile(join(root,'native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const headers={Authorization:'Bearer '+token,'Content-Type':'application/json'},observations=[];
  const initial=await game.inspect();
  const response=await fetch(config.origin+'/dev/map?player='+initial.diagnostic.player,{headers,signal:AbortSignal.timeout(10000)});assert(response.ok);
  const map=await response.json(),boss=map.entities.find(e=>e.kind===0x917);assert(boss);
  const {worldSeed,...hunter}=fixture;
  game=await game.join({...hunter,world:initial.view.session.world,position:{x:boss.pos.x-3,y:boss.pos.y}});
  async function cheat(change){
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers,body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:game.actor,change}}),signal:AbortSignal.timeout(10000)});
    const result=await response.json();assert(response.ok&&result.status==='succeeded',JSON.stringify(result));
  }
  await cheat({kind:'protection',condition:'invincible',duration_ms:60000});
  await game.act({keys:[],duration_ms:2000});
  await game.act({keys:[],duration_ms:600},{name:'huntress-before',when:()=>true});
  const start=await game.observe();
  for(const [index,item,effect,duration,cost] of [[-1,0xab1,4,3000,40],[0,0xab6,4,5500,140],[1,0xb2c,4,6000,140],[2,0xc1c,14,3500,130]]){
    if(index>=0)await game.act({keys:[],duration_ms:350,drag:{from:{x:634+44*index,y:428},to:{x:678,y:385}}});
    const equipped=(await game.inspect()).view.character;
    game.check('native inventory equips '+item,equipped.items[1]===item);
    game.check(item+' applies only its equipment stat bonuses',JSON.stringify(equipped.stats.slice(4))===JSON.stringify(item===0xb2c?[50,50,42,52]:item===0xc1c?[52,52,40,50]:[50,50,40,50]));
    await cheat({kind:'vitals',hp:700,mp:252});
    await game.act({keys:[],duration_ms:600});
    const before=await game.inspect();
    await game.act({keys:[' '],duration_ms:100,target:{entity:boss.id,camera_angle:-Math.PI/4,fire:false}});
    const thrown=await game.inspect();
    game.check(item+' spends one mana cost',thrown.view.character.mp>=before.view.character.mp-cost&&thrown.view.character.mp<before.view.character.mp-cost+10);
    await game.act({keys:[],duration_ms:2100},{name:'trap-'+item,when:view=>view.game_ms>=thrown.view.time_ms+1700});
    const hit=await game.inspect(),target=hit.diagnostic.entities.find(e=>e.id===boss.id),status=target?.statuses[String(effect)];
    game.check(item+' applies the selected condition and exact duration',status&&status[1]-status[0]===duration&&status[1]>hit.view.time_ms&&(hit.view.objects.find(e=>e.id===boss.id)?.condition_mask&(1<<(effect-1))));
    game.check(item+' detonates and removes its placed trap',!hit.diagnostic.entities.some(e=>e.kind===0x711));
    await game.act({keys:[],duration_ms:duration+100});
    const expired=await game.inspect(),after=expired.view.objects.find(e=>e.id===boss.id);
    game.check(item+' condition expires',after&&expired.view.time_ms>=status[1]&&!(after.condition_mask&(1<<(effect-1))));
    observations.push({item,before,thrown,hit,expired});
    await writeFile(join(root,'trap-observations.json'),JSON.stringify({preparedProtection:true,preparedVitals:true,observations},null,2));
  }
  await cheat({kind:'vitals',hp:700,mp:0});
  const poor=await game.observe();
  await game.act({keys:[' '],duration_ms:200,target:{entity:boss.id,camera_angle:-Math.PI/4,fire:false}});
  await game.act({keys:[],duration_ms:1800});
  const denied=await game.inspect(),target=denied.view.objects.find(e=>e.id===boss.id);
  await writeFile(join(root,'trap-observations.json'),JSON.stringify({preparedProtection:true,preparedVitals:true,observations,poor,denied},null,2));
  game.check('insufficient mana creates no trap or Paralyzed effect',poor.character.mp===0&&denied.view.character.mp<130&&target&&!(target.condition_mask&(1<<13))&&!denied.diagnostic.entities.some(e=>e.kind===0x711));
  game.check('same living Huntress retains the equipped Coral trap',denied.view.lifecycle.alive&&denied.view.session.character===start.session.character&&denied.view.character.items[1]===0xc1c);
}
