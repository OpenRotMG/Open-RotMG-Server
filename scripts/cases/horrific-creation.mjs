import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
import {route,walk as nativeWalk} from './native-route.mjs';

// Protected mechanics case: fixed generated Lab, geared level-20 archer, renewable player-only protection, attack and vitals.
// No opponent, wall, reward or progress is prepared; kills and pickup come from native shots and drag.
export const fixture={class:0x307,level:20,seed:12351,world:'mad-lab',worldSeed:12351,position:'spawn',equipment:[0xc02,0xb28,0xaf9,0xacd],backpack:Array(8).fill(-1),stats:[700,252,75,25,50,50,40,50]};
const COIL=0x188a,BOSS=0x976,WALL=0x18c2,CREATION=0x5e1c,DEFENSE=0xa20,SHIELDED=(1<<23)|(1<<24),HEXED=1<<27;
export async function run(game){
  const output=name=>join(process.env.QA_RESULTS_DIR,name),samples=[],trace=[],prepared=[];
  const report=JSON.parse(await readFile(output('native/result.json'),'utf8'));
  const token=(await readFile(output('native/state/control.token'),'utf8')).trim();
  let protectedUntil=0,modifierUntil=0;
  const distance=(a,b)=>Math.hypot(a.x-b.x,a.y-b.y);
  async function save(name){const state=await game.inspect();samples.push({name,state});await writeFile(output('creation-journey.json'),JSON.stringify(samples));return state;}
  async function api(command){
    const response=await fetch(report.origin+'/dev/testing',{method:'POST',signal:AbortSignal.timeout(10000),headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command})});
    const result=await response.json();assert(response.ok&&result.status!=='failed',JSON.stringify(result));return result;
  }
  async function prepare(change){
    const {game_ms}=await game.observe();
    await api({op:'cheat',actor:game.actor,change});
    prepared.push({provenance:'prepared/protected',game_ms,actor:game.actor,change});
    await writeFile(output('creation-prepared.json'),JSON.stringify({count:prepared.length,operations:prepared}));
  }
  async function protect(){
    const v=await game.observe(),now=v.game_ms;
    if(!(v.character.conditions&(1<<23))){await prepare({kind:'protection',condition:'invincible',duration_ms:60000});protectedUntil=now+60000;}
    return protectedUntil-now;
  }
  // Keep armor through the real expiry, when accumulated fire can hit before the next paused observation.
  const act=async(input,capture)=>{
    const ms=[input].flat().reduce((t,i)=>t+(i.input??i).duration_ms,0);
    for(let left;(left=await protect())<2*ms+5000;){
      const {game_ms:now,character:c}=await game.observe();
      if(left<=6000){
        if(now>=modifierUntil){await prepare({kind:'stat_modifier',index:3,amount:100,duration_ms:12000});modifierUntil=now+12000;}
        if(c.hp<c.stats.max_hp)await prepare({kind:'vitals',hp:c.stats.max_hp,mp:c.stats.max_mp});
      }
      const step=left>6000?Math.min(10000,left-5000):left>1200?left-1000:left;
      await game.act({keys:[],duration_ms:Math.max(50,step)});
    }
    return game.act(input,capture);
  };
  async function scene(){const v=await game.observe(),r=await fetch(report.origin+'/dev/map?player='+v.session.player,{headers:{Authorization:'Bearer '+token}});assert(r.ok);return r.json();}
  // A Hexxer's range-2 aura Stuns and Hexes (actors.xml Hexxer, madLab.xml:2045): route beside, never through, its pool.
  const hexed=m=>new Set(m.entities.filter(e=>e.kind===0x1e15).flatMap(e=>Array.from({length:49},(_,i)=>({x:Math.floor(e.pos.x)+i%7-3,y:Math.floor(e.pos.y)+Math.floor(i/7)-3}))
    .filter(t=>Math.hypot(t.x+.5-e.pos.x,t.y+.5-e.pos.y)<=2.75).map(t=>t.y*m.width+t.x)));
  const walk=(to,reach)=>nativeWalk({client:{actor:game.actor,observe:()=>game.observe(),act},map:scene,to,ready:async()=>((await protect())-5000)/2,
    options:m=>({reach,soft:new Map(m.entities.filter(e=>e.kind===0x188b||e.kind===COIL).map(e=>[Math.floor(e.pos.y)*m.width+Math.floor(e.pos.x),e])),hazard:hexed(m)})});
  async function dehex(){
    const v=await game.observe();if(!(v.character.conditions&HEXED))return false;
    const blue=(await scene()).entities.filter(p=>p.kind===0x1e16).sort((a,b)=>distance(a.pos,v.position)-distance(b.pos,v.position))[0];
    // DeHexxer clears Hexed within 2 tiles (madLab.xml:2058).
    assert(blue,'No DeHexxer pool for the hexed fighter');await walk(blue.pos,1.5);return true;
  }
  async function restoreVitals(c){
    if(c.hp<c.stats.max_hp*.8||c.mp<c.stats.max_mp*.8)await prepare({kind:'vitals',hp:c.stats.max_hp,mp:c.stats.max_mp});
  }
  async function combatVitals(v){
    const duration_ms=protectedUntil-v.game_ms-18000;
    if(v.game_ms>=modifierUntil&&duration_ms>=5000){await prepare({kind:'stat_modifier',index:2,amount:100,duration_ms});modifierUntil=v.game_ms+duration_ms;}
    await restoreVitals(v.character);
  }
  // Re-aim moving targets in one receipt, but never schedule a second aim after a possible lethal volley.
  const fire=(id,ms,volleys=1)=>[...Array.from({length:volleys},(_,i)=>({actor:game.actor,input:{keys:i?[]:[' '],duration_ms:ms,target:{entity:id,camera_angle:-Math.PI/4,fire:true}}})),{actor:game.actor,input:{keys:[],duration_ms:600}}];
  async function defeat(id,kind){
    const before=(await game.inspect()).diagnostic.character.kills;let hp=0,stalled=0;
    for(let n=0;n<100;n++){
      // Local inspection only lists nearby entities; the development map lists the whole world.
      const state=await game.inspect(),e=(await scene()).entities.find(e=>e.id===id);
      // Tesla Coils are XpMult 0 scenery (actors.xml:15037): no kill credit, and only players can damage them.
      if(!e){assert(kind===COIL||state.diagnostic.character.kills>before,'Enemy left the world without observed kill credit: '+id);return;}
      // A target can stand within range behind a wall: close in once three volleys land nothing.
      stalled=e.hp<hp?0:stalled+1;hp=e.hp;
      if(await dehex())continue;
      const v=await game.observe(),blocked=kind===COIL&&stalled>3;
      if(blocked||distance(e.pos,v.position)>6.5){stalled=0;await walk(e.pos,blocked?2:4.5);}
      // Dr Terrible's Phase2 bubble is Invulnerable (actors.xml:5096).
      else if(e.condition&SHIELDED)await act({keys:[],duration_ms:kind===BOSS?4500:2000});
      else if(!state.view.objects.some(o=>o.id===id&&o.hp>0))await act({keys:[],duration_ms:300});
      else{await combatVitals(v);await act(fire(id,kind===BOSS?4500:2000));}
    }
    await save('failed-fight');throw Error('Native fight exceeded its bound: '+id);
  }
  const entered=await save('entered'),initial=await scene();
  const creation=initial.entities.find(e=>e.kind===CREATION);
  assert(creation,'Generated Lab has no Horrific Creation');
  game.check('Creation is invulnerable before the archer approaches',Boolean(creation.condition&(1<<24)));
  game.check('generated Lab holds the authored ten Tesla Coils',initial.entities.filter(e=>e.kind===COIL).length===10);
  const walls=initial.entities.filter(e=>e.kind===WALL).sort((a,b)=>a.pos.x-b.pos.x);assert.equal(walls.length,4);
  // data/lab-templates.json boss: the Creation spawns 13 tiles north of the second Emergency Wall; its Hexxer vats lie west.
  const home={x:walls[1].pos.x,y:walls[1].pos.y-13};
  // The coils guard the last rooms on the generated route (src/dungeon/lab.rs): follow it to Dr Terrible, clearing them.
  const nearest=async()=>{const at=(await game.observe()).position;return (await scene()).entities.filter(e=>e.kind===COIL).sort((a,b)=>distance(a.pos,at)-distance(b.pos,at))[0];};
  const path=route(initial,entered.view.position,initial.entities.find(e=>e.kind===BOSS).pos,{reach:9}).points;
  for(let i=1;i<path.length;i++){
    if(distance(path[i],path[i-1])<6&&i<path.length-1)continue;
    await walk(path[i],1);await dehex();
    for(let coil;(coil=await nearest())&&distance(coil.pos,path[i])<10;)await defeat(coil.id,COIL);
  }
  for(let coil;(coil=await nearest());)await defeat(coil.id,COIL);
  const coiled=await save('coils-destroyed');
  game.check('native shots destroy every Tesla Coil while the Emergency Walls stay sealed',!coiled.diagnostic.entities.some(e=>e.kind===COIL)&&(await scene()).entities.filter(e=>e.kind===WALL).length===4);
  await defeat((await scene()).entities.find(e=>e.kind===BOSS).id,BOSS);
  for(let n=0;n<10&&(await scene()).entities.some(e=>e.kind===WALL);n++)await act({keys:[],duration_ms:500});
  const cleared=await save('boss-defeated');
  // Recovered Mad Lab Open Wall (madLab.xml:1145) deletes the walls once Dr Terrible and every coil are gone.
  game.check('native coil and Dr Terrible kills open the Emergency Walls and commit one clear',!(await scene()).entities.some(e=>e.kind===WALL)&&cleared.diagnostic.character.mad_labs===entered.diagnostic.character.mad_labs+1);
  // Draw the Charge through the southwest vat from the dry strip beyond it, not from the Hexxer aura itself.
  const lure={x:home.x-7.5,y:home.y+7};
  const rail=(c,r)=>{
    if(distance(c,lure)<=r)return lure;
    const dy=lure.y-c.y;
    if(Math.abs(dy)>r-.5)return {x:c.x,y:c.y+Math.sign(dy)*(r-.5)};
    return {x:c.x-Math.sqrt(r*r-dy*dy),y:lure.y};
  };
  await walk({x:home.x,y:home.y+4.5},.6);
  for(let n=0;n<600;n++){
    const s=await game.inspect(),v=s.view,c=s.diagnostic.entities.find(e=>e.id===creation.id);
    if(!c){assert(!(await scene()).entities.some(e=>e.id===creation.id),'Creation left the local fight without dying');break;}
    const phase=s.diagnostic.xml[c.id]?.states[1]?.name;
    const previous=trace.at(-1);
    trace.push({game_ms:s.diagnostic.time_ms,phase,hp:c.hp,condition:c.condition,pos:c.pos,player:v.position});
    if(n%10===0)await writeFile(output('creation-trace.json'),JSON.stringify(trace));
    if(!(c.condition&SHIELDED)){
      assert(phase==='RandomTexture'||/^AltTex[2-7]$/.test(phase),'Creation lost its shield in '+phase);
      if(await dehex())continue;
      if(distance(c.pos,v.position)>6.5){await walk(c.pos,6);continue;}
      if(!v.objects.some(o=>o.id===c.id&&o.hp>0)){await act({keys:[],duration_ms:300});continue;}
      await combatVitals(await game.observe());await act(c.hp>20000?fire(c.id,950,4):fire(c.id,4200));continue;
    }
    // Recover a Charge wedged at the entrance before leading it back across the vat.
    if(phase==='Charge'&&c.pos.y>home.y+6&&previous?.phase==='Charge'&&distance(c.pos,previous.pos)<.08){await walk({x:home.x,y:home.y+1.5},.6);continue;}
    const blasting=phase==='ShootBlast',goal=rail(c.pos,blasting?6.25:9);
    if(blasting&&distance(c.pos,v.position)<6.75){await act({keys:[],duration_ms:8000});continue;}
    // Only replan when the observed range matters; brief polls spent a receipt without changing the lure.
    if(distance(goal,v.position)>1.4)await walk(goal,.8);else await act({keys:[],duration_ms:phase==='Charge'?1300:2400});
  }
  await writeFile(output('creation-trace.json'),JSON.stringify(trace));
  const final=await save('creation-defeated'),account=final.view.session.account;
  game.check('native shots in the Creation’s vat windows defeat it and grant kill and XP credit',!(await scene()).entities.some(e=>e.id===creation.id)&&final.diagnostic.character.kills>cleared.diagnostic.character.kills&&final.view.character.experience>cleared.view.character.experience);
  game.check('optional Creation defeat does not add a second Mad Lab clear',final.diagnostic.character.mad_labs===cleared.diagnostic.character.mad_labs);
  const bag=final.diagnostic.entities.find(e=>distance(e.pos,trace.at(-1).pos)<10&&e.items?.includes(DEFENSE));
  game.check('Creation guarantees a private Potion of Defense (min 1, actors.xml:15302)',bag&&bag.extra_stats.some(([stat,owner])=>stat===54&&owner===account));
  await walk(bag.pos,.6);await act({keys:[],duration_ms:900},{name:'creation-loot',when:()=>true});
  const slot=bag.items.indexOf(DEFENSE),from={x:634+44*(slot%4),y:528+44*Math.floor(slot/4)},before=await game.observe();
  await act({keys:[],duration_ms:350,drag:{from,to:{x:634,y:428}}});
  const picked=await save('defense-picked');
  game.check('native drag commits the dropped Defense potion once',picked.view.character.items[4]===DEFENSE&&picked.view.character.inventory_revision===before.character.inventory_revision+1);
  await act({keys:[],duration_ms:350,drag:{from,to:{x:678,y:428}}});
  game.check('dragging the emptied bag slot cannot duplicate Defense',(await game.observe()).character.inventory_revision===picked.view.character.inventory_revision);
}
