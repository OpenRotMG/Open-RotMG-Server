import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';

export const fixture={class:0x323,level:20,seed:867,world:'oryx-chamber',position:w=>{const p=w.encounter_markers.find(e=>e.kind===0x1740).position;return {x:p.x-5,y:p.y};},equipment:[0xb08,0xae4,-1,-1],backpack:[0x722,-1,-1,-1,-1,-1,-1,-1],stats:[670,385,60,25,60,50,40,75]};
export async function run(game){
  const out=process.env.QA_RESULTS_DIR,records=[];
  const config=JSON.parse(await readFile(join(out,'native/native.json'),'utf8')),token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const protectedClients=[];let until=0;
  function protectClient(g){
    const act=g.act;protectedClients.push(g);
    g.act=async(input,capture)=>{
      let remaining=input.duration_ms;
      while(remaining>0){
        const now=(await g.inspect()).view.time_ms;
        if(now>=until){
          for(const client of protectedClients){
            const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:client.actor,change:{kind:'protection',condition:'invincible',duration_ms:60000}}})});
            const result=await response.json();assert(response.ok&&result.status==='succeeded',JSON.stringify(result));
          }
          until=now+60000;
        }
        const duration_ms=Math.min(remaining,until-now);remaining-=duration_ms;
        await act({...input,duration_ms},remaining?undefined:capture);
      }
    };
  }
  protectClient(game);
  async function save(name,g=game){const s=await g.inspect();records.push({name,...s});await writeFile(join(out,'chamber.json'),JSON.stringify(records));return s;}
  const start=await save('prepared'),boss=start.diagnostic.entities.find(e=>e.kind===0x1740);assert(boss);
  const observer=await game.join({...fixture,world:start.view.session.world,position:start.view.position});
  protectClient(observer);
  await game.act({keys:[],duration_ms:2200});
  await game.act({keys:[],duration_ms:800},{name:'chamber-before',when:()=>true});
  await game.act({keys:['1'],duration_ms:200});
  game.check('pre-victory Incantation is retained',(await game.observe()).character.items[4]===0x722);
  let minion=(await game.inspect()).diagnostic.entities.find(e=>e.kind===0x1749);assert(minion,'Opening summons actual Minions of Oryx');
  await game.act({keys:[' '],duration_ms:150,target:{entity:minion.id,camera_angle:-Math.PI/4,fire:false}});
  const frozen=await save('minion-stasis');
  game.check('ordinary orb applies Stasis to the selected minion',frozen.diagnostic.entities.find(e=>e.id===minion.id)?.statuses['22']?.[1]>frozen.view.time_ms);
  await game.act({keys:[],duration_ms:3300});
  const xp=(await game.observe()).character.experience,cohort=frozen.diagnostic.entities.filter(e=>e.kind===0x1749);
  const minionKilled=s=>s.view.time_ms<Math.min(...cohort.map(e=>e.expires))&&cohort.some(e=>!s.diagnostic.entities.some(alive=>alive.id===e.id))&&s.view.character.experience>xp;
  for(let n=0;n<40;n++){
    const s=await game.inspect();if(minionKilled(s))break;
    await game.act({keys:[],duration_ms:350,target:{entity:minion.id,camera_angle:-Math.PI/4,fire:true}});
  }
  const killed=await save('minion-defeated');
  game.check('native staff damage defeats an opening minion before expiry and grants XP',minionKilled(killed));
  const activation=killed.diagnostic.entities.find(e=>e.id===boss.id).activated;assert(Number.isInteger(activation));
  for(const [age,name,kinds,speed,lifetime] of [[22000,'dance',[0x1748],30,800],[47000,'artifacts',[0x174a,0x174b],55,2100],[72000,'gaze',[],100,800],[83000,'dance-two',[0x1748],40,2500]]){
    let s=await game.inspect();
    while(s.view.time_ms-activation<age){await game.act({keys:[],duration_ms:Math.min(4000,age-(s.view.time_ms-activation))});s=await game.inspect();}
    await game.act({keys:[],duration_ms:800},{name,when:()=>true});s=await save(name);
    game.check(name+' retains living Oryx and its expected summon population',s.diagnostic.entities.some(e=>e.id===boss.id)&&kinds.every(k=>s.diagnostic.entities.some(e=>e.kind===k)));
    let shot=false;
    for(let n=0;n<8;n++){
      shot=s.view.projectiles.some(p=>p.owner===boss.id&&p.speed===speed&&p.lifetime_ms===lifetime);if(shot)break;
      await game.act({keys:[],duration_ms:200});s=await game.inspect();
    }
    game.check(name+' emits its selected projectile',shot);
  }
  let rage=false;
  for(let n=0;n<160;n++){
    const s=await game.inspect(),e=s.diagnostic.entities.find(e=>e.id===boss.id);if(!e)break;
    if(!rage&&e.hp<=17000&&s.view.projectiles.some(p=>p.owner===boss.id&&p.speed===55&&p.lifetime_ms===3000)){rage=true;await save('rage');}
    await game.act({keys:[],duration_ms:4000,target:{entity:boss.id,camera_angle:-Math.PI/4,fire:true}});
  }
  game.check('low-HP rage emits the selected Superblast projectile',rage);
  const defeated=await save('oryx-defeated');
  game.check('native combat defeats Oryx and increments Oryx kill credit once',!defeated.diagnostic.entities.some(e=>e.id===boss.id)&&defeated.diagnostic.character.lifetime.oryx_kills===start.diagnostic.character.lifetime.oryx_kills+1);
  const locked=defeated.diagnostic.entities.find(e=>e.kind===0x721);assert(locked,'Defeat publishes locked Cellar');
  async function walk(g,p){
    for(let n=0;n<60;n++){
      const v=await g.observe(),dx=p.x-v.position.x,dy=p.y-v.position.y;if(Math.hypot(dx,dy)<.7)return;
      const horizontal=Math.abs(dx)>Math.abs(dy),delta=horizontal?dx:dy;
      await g.act({keys:horizontal?(delta>0?['d','s']:['a','w']):(delta>0?['a','s']:['d','w']),duration_ms:Math.max(80,Math.min(400,Math.round(Math.abs(delta)/8.5*1000)))});
    }assert.fail('Native walk did not reach Cellar');
  }
  await walk(game,locked.pos);
  await walk(observer,locked.pos);
  await observer.act({keys:[],duration_ms:100});
  const before=await save('before-unlock');
  await game.act({keys:['1'],duration_ms:900},{name:'cellar-unlock',when:v=>v.character.items[4]===-1});
  const opened=await save('after-unlock'),portal=opened.diagnostic.entities.find(e=>e.kind===0x700);assert(portal);
  game.check('committed use consumes once and replaces the locked portal',opened.view.character.items[4]===-1&&opened.view.character.inventory_revision===before.view.character.inventory_revision+1&&!opened.diagnostic.entities.some(e=>e.kind===0x721));
  await observer.act({keys:['1'],duration_ms:200});
  game.check('competing user retains their Incantation',(await observer.observe()).character.items[4]===0x722);
  for(const g of [observer,game]){
    await walk(g,portal.pos);await g.act({keys:[],mouse:{x:700,y:575},duration_ms:1700});await save(g===game?'owner-cellar':'observer-cellar',g);
  }
  const a=await game.inspect(),b=await observer.inspect();
  game.check('two native clients enter the same shared Cellar',a.view.map.name==='Wine Cellar'&&b.view.map.name==='Wine Cellar'&&a.view.session.world===b.view.session.world);
  await game.act({keys:['r'],duration_ms:1700});const returned=await save('returned');
  game.check('same living character exits to Nexus with its progress',returned.view.map.name==='Nexus'&&returned.view.lifecycle.alive&&returned.view.session.character===start.view.session.character&&returned.diagnostic.character.lifetime.oryx_kills===a.diagnostic.character.lifetime.oryx_kills);
  const report=JSON.parse(await readFile(join(out,'native/result.json'),'utf8'));
  await writeFile(join(out,'fixture-limits.json'),JSON.stringify({preparedStats:fixture.stats,preparedInvincible:true,preparedChamber:true,earnedCastleRoute:false,origin:report.origin}));
}
