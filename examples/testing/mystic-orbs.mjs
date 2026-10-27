import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
import assert from 'node:assert/strict';
const root=process.argv[process.argv.indexOf('--run-dir')+1];
export const fixture={class:0x323,level:20,seed:773,world:'snake-pit',worldSeed:12351,position:{x:92.5,y:24.5},equipment:[-1,0xae4,-1,-1],backpack:[0xb2d,0xc0b,-1,-1,-1,-1,-1,-1],stats:[670,385,60,25,60,50,40,75]};
const bit=index=>1<<(index-1);
export async function run(game){
  const config=JSON.parse(await readFile(join(root,'native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim(),observations=[];
  const initial=await game.inspect(),snake=initial.diagnostic.entities.find(e=>e.kind===0x228&&Math.hypot(e.pos.x-fixture.position.x,e.pos.y-fixture.position.y)<1);assert(snake,'ordinary seeded Greater Pit Snake exists');
  const target=state=>state.diagnostic.entities.find(e=>e.id===snake.id);
  const flags=state=>state.view.objects.find(e=>e.id===snake.id)?.condition_mask;
  async function save(name,state){observations.push({name,state});await writeFile(join(root,'orb-observations.json'),JSON.stringify({preparedProtection:true,preparedVitals:true,observations},null,2));return state;}
  async function cheat(change){
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:game.actor,change}}),signal:AbortSignal.timeout(10000)});
    const receipt=await response.json();assert(response.ok&&receipt.status==='succeeded',JSON.stringify(receipt));
  }
  async function clearImmunity(){
    const state=await game.inspect(),timer=target(state)?.statuses['23'];
    if(timer&&timer[1]>state.view.time_ms)await game.act({keys:[],duration_ms:timer[1]-state.view.time_ms+200});
    game.check('previous Stasis and immunity have expired',!(flags(await game.inspect())&(bit(22)|bit(23))));
  }
  await cheat({kind:'protection',condition:'invincible',duration_ms:60000});
  await game.act({keys:[],duration_ms:600},{name:'mystic-before',when:()=>true});
  for(const [slot,item,cost,duration,name] of [[-1,0xae4,60,3000,'t0-stasis'],[0,0xb2d,85,8000,'t6-stasis'],[1,0xc0b,100,5000,'conflict-stasis']]){
    await clearImmunity();
    if(slot>=0)await game.act({keys:[],duration_ms:350,drag:{from:{x:634+44*slot,y:428},to:{x:678,y:385}}});
    game.check(name+' equips through native inventory',(await game.observe()).character.items[1]===item);
    await cheat({kind:'vitals',hp:670,mp:385});
    await game.act({keys:[],duration_ms:600});
    const before=await save(name+'-before',await game.inspect());let complete=false;
    const action=game.act([{actor:game.actor,input:{keys:[' '],duration_ms:150,target:{entity:snake.id,camera_angle:-Math.PI/4,fire:false}}},{actor:game.actor,input:{keys:[],duration_ms:duration+500}}],name==='t0-stasis'?undefined:{name,when:v=>v.character.mp<before.view.character.mp-cost/2}).finally(()=>complete=true);
    const sampling=(async()=>{
      async function until(predicate){
        while(!complete){const state=await game.inspect();if(predicate(state))return state;await new Promise(ok=>setTimeout(ok,50));}
        assert.fail(name+' ended before the required observed condition');
      }
      const hit=await save(name,await until(s=>flags(s)&bit(22))),timer=target(hit).statuses['22'];
      const regeneration=Math.ceil((hit.view.time_ms-before.view.time_ms)*0.006)+2;
      game.check(name+' spends one mana cost',hit.view.character.mp>=before.view.character.mp-cost&&hit.view.character.mp<=before.view.character.mp-cost+regeneration);
      game.check(name+' applies exact Stasis duration',timer[1]-timer[0]===duration&&timer[1]>hit.view.time_ms);
      game.check(name+' equipment bonuses match selected XML',JSON.stringify(hit.view.character.stats.slice(2))===JSON.stringify(item===0xb2d?[60,25,60,50,42,77]:item===0xc0b?[62,25,60,52,40,75]:[60,25,60,50,40,75]));
      if(item===0xc0b)game.check('Conflict grants Speedy and Damaging',(hit.view.character.conditions&(bit(15)|bit(19)))===(bit(15)|bit(19)));
      const immune=await save(name+'-immunity',await until(s=>!(flags(s)&bit(22))&&(flags(s)&bit(23)))),immunity=target(immune).statuses['23'];
      game.check(name+' expires into exactly three seconds of immunity',immunity[0]===timer[1]&&immunity[1]-immunity[0]===3000);
      if(item===0xc0b)game.check('Conflict self buffs expire',!(immune.view.character.conditions&(bit(15)|bit(19))));
    })();
    const results=await Promise.allSettled([action,sampling]);for(const result of results)if(result.status==='rejected')throw result.reason;
  }
  await clearImmunity();
  await cheat({kind:'vitals',hp:670,mp:0});const poor=await save('zero-mana',await game.inspect());
  await game.act({keys:[' '],duration_ms:150,target:{entity:snake.id,camera_angle:-Math.PI/4,fire:false}});
  const denied=await save('denied',await game.inspect());
  game.check('insufficient mana applies neither Stasis nor self buffs',poor.view.character.mp===0&&denied.view.character.mp<100&&!(flags(denied)&bit(22))&&!(denied.view.character.conditions&(bit(15)|bit(19))));
  game.check('same living Mystic retains Conflict',denied.view.lifecycle.alive&&denied.view.session.character===initial.view.session.character&&denied.view.character.items[1]===0xc0b);
}
