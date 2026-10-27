import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
import assert from 'node:assert/strict';
const root=process.argv[process.argv.indexOf('--run-dir')+1];
export const fixture={class:0x323,level:20,seed:773,world:'snake-pit',worldSeed:12351,position:'spawn',equipment:[-1,0xae4,-1,-1],backpack:[0xb2d,0xc0b,-1,-1,-1,-1,-1,-1],stats:[670,385,60,25,60,50,40,75]};
const bit=index=>1<<(index-1);
export async function run(game){
  const config=JSON.parse(await readFile(join(root,'native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const headers={Authorization:'Bearer '+token,'Content-Type':'application/json'},observations=[];
  const initial=await game.inspect();
  const response=await fetch(config.origin+'/dev/map?player='+initial.diagnostic.player,{headers,signal:AbortSignal.timeout(10000)});assert(response.ok);
  const map=await response.json(),snake=map.entities.find(e=>e.kind===0x228);assert(snake,'ordinary Greater Pit Snake exists');
  const {worldSeed,...mystic}=fixture;
  game=await game.join({...mystic,world:initial.view.session.world,position:snake.pos});
  async function cheat(change){
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers,body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:game.actor,change}}),signal:AbortSignal.timeout(10000)});
    const receipt=await response.json();assert(response.ok&&receipt.status==='succeeded',JSON.stringify(receipt));
  }
  await cheat({kind:'protection',condition:'invincible',duration_ms:90000});
  await game.act({keys:[],duration_ms:600},{name:'mystic-before',when:()=>true});
  const start=await game.observe();
  const target=state=>state.diagnostic.entities.find(e=>e.id===snake.id);
  async function save(name){const state=await game.inspect();observations.push({name,state});await writeFile(join(root,'orb-observations.json'),JSON.stringify({preparedProtection:true,preparedVitals:true,observations},null,2));return state;}
  async function cast(name,cost,duration){
    await cheat({kind:'vitals',hp:670,mp:385});
    await game.act({keys:[],duration_ms:600});
    const before=await save(name+'-before');assert(target(before));
    await game.act({keys:[' '],duration_ms:150,target:{entity:snake.id,camera_angle:-Math.PI/4,fire:false}});
    await game.act({keys:[],duration_ms:600},{name,when:()=>true});
    const hit=await save(name);
    game.check(name+' spends one mana cost',hit.view.character.mp>=before.view.character.mp-cost&&hit.view.character.mp<before.view.character.mp-cost+10);
    const timer=target(hit).statuses['22'];
    game.check(name+' applies exact Stasis timer and client condition',timer&&timer[1]-timer[0]===duration&&timer[1]>hit.view.time_ms&&(hit.view.objects.find(e=>e.id===snake.id)?.condition_mask&bit(22)));
    return hit;
  }
  const first=await cast('t0-stasis',60,3000),timer=target(first).statuses['22'];
  await game.act({keys:[],duration_ms:600});
  await game.act({keys:[' '],duration_ms:150,target:{entity:snake.id,camera_angle:-Math.PI/4,fire:false}});
  const repeated=await save('repeated');
  game.check('recasting cannot extend active Stasis',JSON.stringify(target(repeated).statuses['22'])===JSON.stringify(timer));
  game.check('Stasis preserves enemy position and HP without weapon fire',JSON.stringify(target(repeated).pos)===JSON.stringify(target(first).pos)&&target(repeated).hp===target(first).hp);
  await game.act({keys:[],duration_ms:Math.max(50,timer[1]-(await game.observe()).game_ms+200)});
  const immune=await save('temporary-immunity'),immunity=target(immune).statuses['23'];
  game.check('Stasis expires into exactly three seconds of immunity',immunity&&immunity[0]===timer[1]&&immunity[1]-immunity[0]===3000&&!(immune.view.objects.find(e=>e.id===snake.id)?.condition_mask&bit(22))&&(immune.view.objects.find(e=>e.id===snake.id)?.condition_mask&bit(23)));
  await game.act({keys:[' '],duration_ms:150,target:{entity:snake.id,camera_angle:-Math.PI/4,fire:false}});
  const blocked=await save('immune-cast');
  game.check('temporary immunity rejects new Stasis without refreshing immunity',!(blocked.view.objects.find(e=>e.id===snake.id)?.condition_mask&bit(22))&&JSON.stringify(target(blocked).statuses['23'])===JSON.stringify(immunity));
  await game.act({keys:[],duration_ms:Math.max(50,immunity[1]-(await game.observe()).game_ms+200)});
  for(const [slot,item,cost,duration,name] of [[0,0xb2d,85,8000,'t6-stasis'],[1,0xc0b,100,5000,'conflict-stasis']]){
    await game.act({keys:[],duration_ms:350,drag:{from:{x:634+44*slot,y:428},to:{x:678,y:385}}});
    game.check(name+' equips through native inventory',(await game.observe()).character.items[1]===item);
    const hit=await cast(name,cost,duration);
    game.check(name+' equipment bonuses match selected XML',JSON.stringify(hit.view.character.stats.slice(2))===JSON.stringify(item===0xb2d?[60,25,60,50,42,77]:[62,25,60,52,40,75]));
    if(item===0xc0b){
      game.check('Conflict grants Speedy and Damaging', (hit.view.character.conditions&(bit(15)|bit(19)))===(bit(15)|bit(19)));
      await game.act({keys:[],duration_ms:2800});
      const expired=await save('conflict-buffs-expired');
      game.check('Conflict buffs expire before Stasis',!(expired.view.character.conditions&(bit(15)|bit(19)))&&(expired.view.objects.find(e=>e.id===snake.id)?.condition_mask&bit(22)));
    }
    await game.act({keys:[],duration_ms:duration+3200});
  }
  await cheat({kind:'vitals',hp:670,mp:0});
  const poor=await save('zero-mana');
  await game.act({keys:[' '],duration_ms:150,target:{entity:snake.id,camera_angle:-Math.PI/4,fire:false}});
  const denied=await save('denied');
  game.check('insufficient mana applies neither Stasis nor self buffs',poor.view.character.mp===0&&denied.view.character.mp<100&&!(denied.view.objects.find(e=>e.id===snake.id)?.condition_mask&bit(22))&&!(denied.view.character.conditions&(bit(15)|bit(19))));
  game.check('same living Mystic retains Conflict',denied.view.lifecycle.alive&&denied.view.session.character===start.session.character&&denied.view.character.items[1]===0xc0b);
}
