import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';

export const fixture={class:0x323,level:20,seed:867,world:'oryx-chamber',position:w=>{const p=w.encounter_markers.find(e=>e.kind===0x1740).position;return {x:p.x-5,y:p.y};},equipment:[0xb08,0xae4,-1,-1],backpack:[0x722,0x722,-1,-1,-1,-1,-1,-1],stats:[670,385,60,25,60,50,40,75]};
export async function run(game){
  const out=process.env.QA_RESULTS_DIR,records=[];
  const config=JSON.parse(await readFile(join(out,'native/native.json'),'utf8')),token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const protectedClients=[];let until=0;
  function protectClient(g){
    const act=g.act;protectedClients.push({actor:g.actor,act});
    g.act=async(input,capture)=>{
      let remaining=input.duration_ms;
      while(remaining>0){
        let now=(await g.inspect()).view.time_ms;
        if(now>=until){
          for(const client of protectedClients){
            const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:client.actor,change:{kind:'protection',condition:'invincible',duration_ms:60000}}})});
            const result=await response.json();assert(response.ok&&result.status==='succeeded',JSON.stringify(result));
          }
          until=now+60000;
          for(const client of protectedClients)await client.act({keys:['a','d'],duration_ms:25});
          now=(await g.inspect()).view.time_ms;
        }
        const duration_ms=Math.min(remaining,until-now);remaining-=duration_ms;
        await act({...input,duration_ms},remaining?undefined:capture);
      }
    };
  }
  protectClient(game);
  async function save(name,g=game){const s=await g.inspect();records.push({name,...s});await writeFile(join(out,'chamber.json'),JSON.stringify(records));return s;}
  const start=await save('prepared'),boss=start.diagnostic.entities.find(e=>e.kind===0x1740);assert(boss);
  await game.act({keys:[],duration_ms:2200});
  await game.act({keys:[],duration_ms:800},{name:'chamber-before',when:()=>true});
  await game.act({keys:['2'],duration_ms:200});
  game.check('pre-victory Incantation is retained',(await game.observe()).character.items[5]===0x722);
  // XML Oryx only starts its attack rotation, MakeMinions first, below 97% HP (actors.xml:13641-13649,14227-14236).
  const phase=s=>s.diagnostic.xml[boss.id]?.states[1]?.name;
  for(let n=0;n<80&&phase(await game.inspect())!=='MakeMinions';n++)await game.act({keys:[],duration_ms:300,target:{entity:boss.id,camera_angle:-Math.PI/4,fire:true}});
  let minion;
  for(let n=0;n<20;n++){const s=await game.inspect();if(minion=s.diagnostic.entities.find(e=>e.kind===0x1749&&s.view.objects.some(o=>o.id===e.id)))break;await game.act({keys:[],duration_ms:200});}
  assert(minion,'MakeMinions summons actual Minions of Oryx');
  const cast=(await game.observe()).game_ms;
  // A wandering minion can leave the client view between inspection and input; retarget a visible one.
  for(let n=0;;n++){
    try{await game.act({keys:[' '],duration_ms:150,target:{entity:minion.id,camera_angle:-Math.PI/4,fire:false}});break;}
    catch(error){if(n>=5||!String(error.message).includes('not in the actual client neighborhood'))throw error;}
    const s=await game.inspect();minion=s.diagnostic.entities.find(e=>e.kind===0x1749&&s.view.objects.some(o=>o.id===e.id))??minion;
  }
  const frozen=await save('minion-stasis');
  // A loaded act can outlast the 3s Stasis, so check that this cast started it.
  game.check('ordinary orb applies Stasis to the selected minion',frozen.diagnostic.entities.find(e=>e.id===minion.id)?.statuses['22']?.[0]>=cast);
  // Minions Decay 15s after birth without XP (actors.xml Minion of Oryx), so hunt one outside its Stasis window (statuses, not the
  // condition mask: VPS 8dffb3b3 fired every volley at the frozen minion) at once with sustained fire.
  // Prepared +100 ATT: the staff alone took ~350 HP/s off a 4000-HP minion before it decayed (VPS 31a631b3) and ~100 HP/s off Oryx.
  let empowered=0;
  async function empower(){
    const now=(await game.observe()).game_ms;if(now<empowered)return;
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:game.actor,change:{kind:'stat_modifier',index:2,amount:100,duration_ms:60000}}})});
    const result=await response.json();assert(response.ok&&result.status==='succeeded',JSON.stringify(result));empowered=now+60000;
  }
  await empower();
  const xp=(await game.observe()).character.experience;let prey;
  const minionKilled=s=>prey&&!s.diagnostic.entities.some(e=>e.id===prey.id)&&s.view.character.experience>xp;
  for(let n=0;n<40;n++){
    const s=await game.inspect();if(minionKilled(s))break;
    const v=s.view.position,visible=s.diagnostic.entities.filter(e=>e.kind===0x1749&&!(e.statuses['22']?.[1]>s.diagnostic.time_ms)&&s.view.objects.some(o=>o.id===e.id)).sort((a,b)=>Math.hypot(a.pos.x-v.x,a.pos.y-v.y)-Math.hypot(b.pos.x-v.x,b.pos.y-v.y));
    if(!visible.length){await game.act({keys:[],duration_ms:350});continue;}
    // Entity-targeted volleys use the client's own projection (VPS b65daf33's hand-projected mouse missed ~50 volleys); a protection renewal can still let the wanderer leave view first.
    prey=visible.find(e=>e.id===prey?.id)??visible[0];
    try{await game.act({keys:[],duration_ms:1500,target:{entity:prey.id,camera_angle:-Math.PI/4,fire:true}});}
    catch(error){if(!String(error.message).includes('not in the actual client neighborhood'))throw error;}
  }
  const killed=await save('minion-defeated');
  game.check('native staff damage defeats a summoned minion and grants XP',minionKilled(killed));
  // The recovered chooser rotates tag-selected attack embeds (actors.xml:14227-14460), not a fixed timeline.
  const attacks=['CircleCombat','ArtifactDance','Hadouken','MinionToss','BulletDance'],phases=new Set(),kinds=new Set();
  const summonsObserved=()=> (!phases.has('CircleCombat')||kinds.has(0x1748))&&(!phases.has('ArtifactDance')||[0x174a,0x174b,0x174c,0x174d,0x174e].some(k=>kinds.has(k)));
  for(let n=0;n<80&&([...phases].filter(p=>attacks.includes(p)).length<3||!summonsObserved());n++){
    const s=await game.inspect();assert(s.diagnostic.entities.some(e=>e.id===boss.id),'Oryx must survive its attack rotation');
    phases.add(phase(s));for(const e of s.diagnostic.entities)if(e.parent===boss.id)kinds.add(e.kind);
    await game.act({keys:[],duration_ms:1500},n===8?{name:'oryx-rotation',when:()=>true}:undefined);
  }
  await save('rotation');await writeFile(join(out,'rotation.json'),JSON.stringify({phases:[...phases],ownedKinds:[...kinds]}));
  game.check('Oryx rotates through three recovered attack phases',[...phases].filter(p=>attacks.includes(p)).length>=3);
  game.check('chosen ring and artifact phases summon their elements',summonsObserved());
  // Unprepared, the finale spent the 256-receipt budget before AllIn (local f1baa0b5).
  let rage=false;
  for(let n=0;n<80;n++){
    const s=await game.inspect(),e=s.diagnostic.entities.find(e=>e.id===boss.id);if(!e)break;
    if(!rage&&phase(s)==='AllIn'){rage=true;await save('rage');}
    await empower();
    try{await game.act({keys:[],duration_ms:5000,target:{entity:boss.id,camera_angle:-Math.PI/4,fire:true}});}
    catch(error){
      if(!String(error.message).includes('not in the actual client neighborhood'))throw error;
      // A volley can finish the fight before the renewal-split tail resolves its target.
      const terminal=await game.inspect();
      if(terminal.diagnostic.entities.some(e=>e.id===boss.id)||terminal.diagnostic.character.lifetime.oryx_kills!==start.diagnostic.character.lifetime.oryx_kills+1)throw error;
      break;
    }
  }
  game.check('low-HP AllIn finale is reached before defeat',rage);
  const defeated=await save('oryx-defeated');
  game.check('native combat defeats Oryx and increments Oryx kill credit once',!defeated.diagnostic.entities.some(e=>e.id===boss.id)&&defeated.diagnostic.character.lifetime.oryx_kills===start.diagnostic.character.lifetime.oryx_kills+1);
  const locked=defeated.diagnostic.entities.find(e=>e.kind===0x721);assert(locked,'Defeat publishes locked Cellar');
  async function walk(g,p,reach=.7){
    for(let n=0;n<60;n++){
      const v=await g.observe(),dx=p.x-v.position.x,dy=p.y-v.position.y;if(Math.hypot(dx,dy)<reach)return;
      const horizontal=Math.abs(dx)>Math.abs(dy),delta=horizontal?dx:dy;
      await g.act({keys:horizontal?(delta>0?['d','s']:['a','w']):(delta>0?['a','s']:['d','w']),duration_ms:Math.max(80,Math.min(400,Math.round(Math.abs(delta)/8.5*1000)))});
    }assert.fail('Native walk did not reach Cellar');
  }
  // The observer tests the locked-Cellar race, not the attack rotation (actors.xml:14227–14460).
  const observer=await game.join({...fixture,world:defeated.view.session.world,position:start.view.position});
  await walk(game,locked.pos);
  await walk(observer,locked.pos);
  await observer.act({keys:[],duration_ms:100});
  await game.act({keys:['2'],duration_ms:200});
  await save('rejected-slot-retry');
  const before=await save('before-unlock');
  await game.act({keys:['1'],duration_ms:900},{name:'cellar-unlock',when:v=>v.character.items[4]===-1});
  const opened=await save('after-unlock'),portal=opened.diagnostic.entities.find(e=>e.kind===0x700);assert(portal);
  game.check('committed use consumes once and replaces the locked portal',opened.view.character.items[4]===-1&&opened.view.character.items[5]===0x722&&opened.view.character.inventory_revision===before.view.character.inventory_revision+1&&!opened.diagnostic.entities.some(e=>e.kind===0x721));
  await observer.act({keys:['1'],duration_ms:200});
  game.check('competing user retains their Incantation',(await observer.observe()).character.items[4]===0x722);
  for(const g of [observer,game]){
    // Oryx dies on the fighter, so its private bags share the portal tile and the client's interact panel shows the nearer bag
    // instead of Enter (local c5309d2b); stand on the portal's far side from them.
    const s=await g.inspect(),bags=s.diagnostic.entities.filter(e=>e.items?.some(i=>i!==-1)&&e.extra_stats.some(([k,a])=>k===54&&a===s.view.session.account)&&Math.hypot(e.pos.x-portal.pos.x,e.pos.y-portal.pos.y)<1.5);
    if(bags.length){const cx=bags.reduce((a,b)=>a+b.pos.x,0)/bags.length,cy=bags.reduce((a,b)=>a+b.pos.y,0)/bags.length,d=Math.hypot(portal.pos.x-cx,portal.pos.y-cy);assert(d>.01,'Bags centred on the portal leave no far side');
      await walk(g,{x:portal.pos.x+.55*(portal.pos.x-cx)/d,y:portal.pos.y+.55*(portal.pos.y-cy)/d},.45);}
    else await walk(g,portal.pos);await g.act({keys:[],mouse:{x:700,y:575},duration_ms:1700});await save(g===game?'owner-cellar':'observer-cellar',g);
  }
  const a=await game.inspect(),b=await observer.inspect();
  game.check('two native clients enter the same shared Cellar',a.view.map.name==='Wine Cellar'&&b.view.map.name==='Wine Cellar'&&a.view.session.world===b.view.session.world);
  await game.act({keys:['Insert'],duration_ms:1700});const returned=await save('returned');
  game.check('same living character exits to Nexus with its progress',returned.view.map.name==='Nexus'&&returned.view.lifecycle.alive&&returned.view.session.character===start.view.session.character&&returned.diagnostic.character.lifetime.oryx_kills===a.diagnostic.character.lifetime.oryx_kills);
  const report=JSON.parse(await readFile(join(out,'native/result.json'),'utf8'));
  await writeFile(join(out,'fixture-limits.json'),JSON.stringify({preparedStats:fixture.stats,preparedInvincible:true,preparedAttackModifier:true,preparedChamber:true,rejectedSlotRetryAccepted:false,earnedCastleRoute:false,origin:report.origin}));
}
