import assert from 'node:assert/strict';
import {nativeMap} from './native-map.mjs';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {DatabaseSync} from 'node:sqlite';
import {walk as nativeWalk} from './native-route.mjs';

// Prepared only at enrollment: one capped Priest at the coil approach and two south of Dr Terrible's room.
// Ordinary play ends after ten coils,
// Dr Terrible and one journaled clear. Wall release, vat-window Creation defeat and Defense pickup belong to the
// separately prepared/protected horrific-creation.mjs mechanics case.
// Short input windows for dodge readback; all per-action and job deadlines are unchanged.
export const nativeReceiptLimit=448;
export const fixture={class:0x310,level:20,seed:12351,world:'mad-lab',worldSeed:12351,position:{x:88.5,y:168.5},equipment:[0xaf8,0xc1e,0xb07,0xc26],backpack:[-1,0x707,0xc21,0xc21,0xa23,0xa23,0xa23,0xb3f],stats:[670,385,50,25,50,50,40,75]};
const COIL=0x188a,BOSS=0x976,TURRET=0x12aa,GREEN_POTION=0x978,WALL=0x18c2,CREATION=0x5e1c,SHIELDED=(1<<23)|(1<<24),HEXED=1<<27;
export async function run(game){
  const output=name=>join(process.env.QA_RESULTS_DIR,name),samples=[];
  const report=JSON.parse(await readFile(output('native/result.json'),'utf8'));
  const token=(await readFile(output('native/state/control.token'),'utf8')).trim();
  const distance=(a,b)=>Math.hypot(a.x-b.x,a.y-b.y);
  async function save(name){const state=await game.inspect();samples.push({name,state});await writeFile(output('creation-journey.json'),JSON.stringify(samples));return state;}
  const supports=[];
  const act=async(input,capture,together=[])=>{
    const {character:c,game_ms:now}=await game.observe(),steps=[];
    const use=k=>{const slot=c.items.findIndex((item,i)=>i>=4&&item===k);if(slot>=4)steps.push({keys:[String(slot-3)],duration_ms:150});};
    if(c.conditions&(HEXED|(1<<26)|(1<<13)|(1<<6)|(1<<4)|2))use(0xc21);
    if(c.hp<c.stats.max_hp*.58)use(0x707);
    if(now>=c.ability_ready_ms&&c.hp<c.stats.max_hp*.86){if(c.mp<100)use(0xa23);steps.push({keys:[' '],duration_ms:150});}
    if(!steps.length&&!together.length)return game.act(input,capture);
    const main=[input].flat().map(i=>i.input?i:{actor:game.actor,input:i});
    return game.act([...steps.map(input=>({actor:game.actor,input})),[main[0],...together],...main.slice(1)],capture);
  };
  const scene=async()=>nativeMap(report.origin,{Authorization:'Bearer '+token},(await game.observe()).session.player);
  // A Hexxer's range-2 aura Stuns and Hexes (actors.xml Hexxer, madLab.xml:2045): route beside, never through, its pool.
  const hexed=m=>new Set(m.entities.filter(e=>e.kind===0x1e15).flatMap(e=>Array.from({length:49},(_,i)=>({x:Math.floor(e.pos.x)+i%7-3,y:Math.floor(e.pos.y)+Math.floor(i/7)-3}))
    .filter(t=>Math.hypot(t.x+.5-e.pos.x,t.y+.5-e.pos.y)<=2.75).map(t=>t.y*m.width+t.x)));
  const acid=new Set(),tile=(p,m)=>Math.floor(p.y)*m.width+Math.floor(p.x);
  const remember=(v,m)=>{for(const t of v.tiles)if(t.ground===0xa8)acid.add(tile(t,m));};
  let refuge;
  const walk=(to,reach,screen=true,standoff=false)=>nativeWalk({client:{actor:game.actor,observe:()=>game.observe(),act},map:scene,to,ready:async()=>{
    if(!screen)return 3600;
    return await cover();
  },
    options:m=>({reach,standoff,soft:new Map(m.entities.filter(e=>e.kind===0x188b||e.kind===COIL).map(e=>[Math.floor(e.pos.y)*m.width+Math.floor(e.pos.x),e])),hazard:new Set([...hexed(m),...acid]),clearBlockedApproach:true})});
  async function dehex(){
    const v=await game.observe();if(!(v.character.conditions&HEXED))return false;
    const blue=(await scene()).entities.filter(p=>p.kind===0x1e16).sort((a,b)=>distance(a.pos,v.position)-distance(b.pos,v.position))[0];
    // DeHexxer clears Hexed within 2 tiles (madLab.xml:2058).
    assert(blue,'No DeHexxer pool for the hexed fighter');await walk(blue.pos,1.5,false);return true;
  }
  const fire=(e,v,duration_ms)=>{
    // Keep the cursor near the player; the pinned client dropped stationary presses around y=464.
    const dx=e.pos.x-v.position.x,dy=e.pos.y-v.position.y,scale=90/Math.max(1,Math.hypot(dx,dy)*50);
    return {keys:[],duration_ms,mouse:{x:300+(dx-dy)*35.355*scale,y:325+(dx+dy)*35.355*scale}};
  };
  function collision(m){
    const blocked=new Set(m.blocked.map(([x,y])=>y*m.width+x));
    const line=(p,q)=>{
      const n=Math.ceil(distance(p,q)*3);
      for(let i=1;i<n;i++)if(blocked.has(tile({x:p.x+(q.x-p.x)*i/n,y:p.y+(q.y-p.y)*i/n},m)))return false;
      return true;
    };
    return {blocked,line};
  }
  const keysFor=(x,y)=>[x-y>0?'d':x-y<0?'a':null,x+y>0?'s':x+y<0?'w':null].filter(Boolean);
  const directions=[0,-1,1].flatMap(x=>[0,-1,1].map(y=>({x,y})));
  const speed=v=>v.character.conditions&(1<<13)?0:v.character.conditions&8?4:4+5.6*v.character.stats[4]/75;
  const bossHazards=(m,p)=>m.entities.filter(e=>[TURRET,GREEN_POTION].includes(e.kind)&&distance(e.pos,p)<10);
  async function fleePotion(v,m,hazards){
    const p=v.position,bombs=hazards.filter(e=>e.kind===GREEN_POTION);
    const separation=q=>Math.min(...bombs.map(e=>distance(q,e.pos)));
    if(separation(p)>=3.5||!speed(v))return false;
    // The toss lands on the player. Retreat before spending time on a cast or requiring line of sight on the boss.
    const {blocked,line}=collision(m),ms=450,step=speed(v)*ms/1000;
    const lane=directions.filter(q=>q.x||q.y).map(({x,y})=>{
      const len=Math.hypot(x,y),q={x:p.x+x*step/len,y:p.y+y*step/len};
      return {x,y,q,clearance:separation(q)};
    }).filter(({q})=>q.x>=1&&q.y>=1&&q.x<m.width-1&&q.y<m.height-1&&!acid.has(tile(q,m))&&[-.4,0,.4].every(x=>[-.4,0,.4].every(y=>{
      const end={x:q.x+x,y:q.y+y};return !blocked.has(tile(end,m))&&line(p,end);
    }))).sort((a,b)=>b.clearance-a.clearance)[0];
    if(!lane||lane.clearance<=separation(p))return false;
    await game.act({keys:keysFor(lane.x,lane.y),duration_ms:ms});return true;
  }
  async function supportingFire(boss,m,ms){
    const {blocked,line}=collision(m);
    return Promise.all(supports.map(async ally=>{
      const {view:v,diagnostic}=await ally.inspect(),p=v.position;
      const shots=v.projectiles.filter(b=>b.damage>0&&distance(p,b.position)<8&&b.owner!==v.session.player);
      const bombs=bossHazards(m,p).filter(e=>e.kind===GREEN_POTION);
      const c=v.character,step=speed(v)*.35;
      const lanes=directions.map(({x,y})=>{
        const norm=Math.hypot(x,y)||1,dx=step*x/norm,dy=step*y/norm,q={x:p.x+dx,y:p.y+dy};
        const risk=shots.reduce((cost,b)=>{
          const rx=p.x-b.position.x,ry=p.y-b.position.y,ux=dx-b.speed*.035*Math.cos(b.angle),uy=dy-b.speed*.035*Math.sin(b.angle);
          const t=Math.max(0,Math.min(1,-(rx*ux+ry*uy)/Math.max(.0001,ux*ux+uy*uy)));
          return cost+(Math.hypot(rx+t*ux,ry+t*uy)<.85?b.damage/20:0);
        },0);
        const gap=distance(q,boss.pos),cost=risk+Math.abs(gap-7.6)+Math.max(0,6.7-gap)*8+
          bombs.reduce((n,b)=>n+Math.max(0,4-distance(q,b.pos))*8,0)+(line(q,boss.pos)?0:2)+Math.hypot(x,y)*.25;
        return {x,y,cost,q};
      }).filter(({q})=>q.x>=1&&q.y>=1&&q.x<m.width-1&&q.y<m.height-1&&line(p,q)&&!blocked.has(tile(q,m))&&!acid.has(tile(q,m)));
      assert(lanes.length,'No safe input lane for Lab support '+ally.actor);
      const lane=lanes.sort((a,b)=>a.cost-b.cost)[0],keys=keysFor(lane.x,lane.y);
      const use=id=>{const slot=c.items.findIndex((item,i)=>i>=4&&item===id);if(slot>=4)keys.push(String(slot-3));};
      if(c.conditions&(HEXED|(1<<26)|(1<<13)|(1<<6)|(1<<4)|2))use(0xc21);
      if(c.hp<c.stats[0]*.58)use(0x707);
      if(v.time_ms>=c.ability_ready_ms&&c.hp<c.stats[0]*.84){if(c.mp>=100)keys.push(' ');else use(0xa23);}
      const duration_ms=Math.min(ms,lane.x||lane.y?350:1600);
      const target=boss.condition&SHIELDED?diagnostic.entities.find(e=>[TURRET,0x1e23,0x1e24,0x1e25].includes(e.kind)&&distance(p,e.pos)<8.3):boss;
      return {actor:ally.actor,input:{...(target&&line(p,target.pos)?fire(target,v,duration_ms):{duration_ms}),keys}};
    }));
  }
  async function volley(hostile,v,m,range,{quietMs=1250,firing=true,hazards=[],rangeSlack=0,boss}={}){
    if(await fleePotion(v,m,hazards))return;
    const p=v.position,{blocked,line}=collision(m),ms=500,step=speed(v)*ms/1000;
    const shots=v.projectiles.filter(b=>b.damage>0&&b.owner!==v.session.player&&distance(b.position,p)<9);
    const risk=(dx,dy)=>shots.reduce((sum,b)=>{
      const vx=dx-b.speed*ms/10000*Math.cos(b.angle),vy=dy-b.speed*ms/10000*Math.sin(b.angle);
      const px=p.x-b.position.x,py=p.y-b.position.y;
      const t=Math.max(0,Math.min(1,-(px*vx+py*vy)/Math.max(.0001,vx*vx+vy*vy)));
      return sum+(Math.hypot(px+t*vx,py+t*vy)<.85?b.damage:0);
    },0);
    const safe=directions.map(({x,y})=>{
      const norm=Math.hypot(x,y)||1,dx=step*x/norm,dy=step*y/norm,q={x:p.x+dx,y:p.y+dy};
      const cost=risk(dx,dy)/15+Math.max(0,Math.abs(distance(q,hostile.pos)-range)-rangeSlack)+Math.hypot(x,y)*.3
        +hazards.reduce((sum,e)=>sum+Math.max(0,4-distance(q,e.pos))*4,0);
      return {x,y,q,cost};
    }).filter(({q})=>q.x>=1&&q.y>=1&&q.x<m.width-1&&q.y<m.height-1&&line(p,q)&&line(q,hostile.pos)&&!acid.has(tile(q,m))&&!blocked.has(tile(q,m))).sort((a,b)=>a.cost-b.cost)[0];
    const keys=safe?keysFor(safe.x,safe.y):[];
    const duration_ms=keys.length||hazards.some(e=>e.kind===GREEN_POTION)||risk(0,0)>0?ms:quietMs;
    if(firing&&hostile.kind===BOSS&&hostile.hp>7500&&v.character.hp>v.character.stats[0]*.9&&duration_ms>=1800&&distance(p,hostile.pos)<7.5&&v.objects.some(o=>o.id===hostile.id&&o.hp>0)){
      // Re-read the native target between short bursts without reopening the controller lease for each one.
      const input={keys:[],duration_ms:950,target:{entity:hostile.id,camera_angle:-Math.PI/4,fire:true}};
      return act([{actor:game.actor,input},{actor:game.actor,input},{actor:game.actor,input}],undefined,boss?await supportingFire(boss,m,input.duration_ms):[]);
    }
    const target=hostile.kind===BOSS&&hostile.last_step?{pos:{x:hostile.pos.x+hostile.last_step.x*15,y:hostile.pos.y+hostile.last_step.y*15}}:hostile;
    await act(firing?{...fire(target,v,duration_ms),keys}:{keys,duration_ms},undefined,boss?await supportingFire(boss,m,duration_ms):[]);
  }
  // Enforcer spreads outrange most weapons and apply Stun. Stop before advancing into their range,
  // clear exposed shooters first, and spend ordinary regeneration time out of their line of fire.
  async function cover(){
    const m=await scene(),{blocked,line}=collision(m);
    for(let n=0;n<36;n++){
      const {view:v,diagnostic}=await game.inspect(),p=v.position;
      remember(v,m);
      // Liquid Evil is damaging ground, not a shooter. Keep moving until dry.
      if(v.character.conditions&HEXED){await dehex();return 550;}
      if(acid.has(tile(p,m)))return 1050;
      const nearby=diagnostic.entities.filter(e=>[0x979,0x980,0x981,0x982,0x983].includes(e.kind)&&distance(p,e.pos)<16);
      const hostile=nearby.filter(e=>distance(p,e.pos)<9.8&&line(p,e.pos)).sort((a,b)=>distance(a.pos,p)-distance(b.pos,p))[0];
      if(hostile){
        // Give ground before healing when the first volley turns into crossfire.
        if(v.character.hp<v.character.stats[0]*.54||distance(p,hostile.pos)<3){
          const away=[[-1,0],[1,0],[0,-1],[0,1]].map(([dx,dy])=>({x:p.x+dx*5,y:p.y+dy*5}))
            .filter(q=>line(p,q)&&!blocked.has(Math.floor(q.y)*m.width+Math.floor(q.x))&&distance(q,hostile.pos)>distance(p,hostile.pos)+2&&!acid.has(tile(q,m)))
            .sort((a,b)=>distance(b,hostile.pos)-distance(a,hostile.pos));
          const safe=away[0]??(refuge&&distance(p,refuge)<8&&line(p,refuge)&&distance(refuge,hostile.pos)>distance(p,hostile.pos)+2?refuge:null);
          if(safe){await walk(safe,1.1,false);continue;}
        }
        await volley(hostile,v,m,8.9);continue;
      }
      refuge=p;
      if(!nearby.length&&v.character.mp<270){await act({keys:[],duration_ms:8500});continue;}
      return nearby.length?550:3000;
    }
    const v=await game.observe();assert(v.character.hp>v.character.stats.max_hp*.6,'Could not secure the Lab approach');return 400;
  }
  async function defeat(id,kind){
    const before=(await game.inspect()).diagnostic.character.kills;let hp=0,stalled=0;
    await save('target-'+id);
    for(let n=0;n<(kind===BOSS?320:100);n++){
      // Local inspection only lists nearby entities; the development map lists the whole world.
      const state=await game.inspect(),m=await scene(),e=m.entities.find(e=>e.id===id);
      remember(state.view,m);
      // Tesla Coils are XpMult 0 scenery (actors.xml:15037): no kill credit, and only players can damage them.
      if(!e){assert(kind===COIL||state.diagnostic.character.kills>before,'Enemy left the world without observed kill credit: '+id);return;}
      // A target can stand within range behind a wall: close in once three volleys land nothing.
      stalled=e.hp<hp?0:stalled+1;hp=e.hp;
      if(await dehex())continue;
      if(kind===COIL)await cover();
      if(kind===BOSS){
        const v=state.view,p=v.position,{line}=collision(m),d=distance(e.pos,p);
        const hazards=bossHazards(m,p);
        const turret=hazards.find(x=>x.kind===TURRET&&distance(x.pos,p)<9&&line(p,x.pos));
        if(n===0&&d>8.7&&!turret&&!hazards.length){await walk(e.pos,7.7,false,true);continue;}
        const shielded=Boolean(e.condition&SHIELDED);
        const add=state.diagnostic.entities.filter(x=>[0x979,0x980,0x981,0x982,0x983,0x1e23,0x1e24,0x1e25].includes(x.kind)&&distance(x.pos,p)<9.6&&line(p,x.pos))
          .sort((a,b)=>distance(a.pos,p)-distance(b.pos,p))[0];
        if(n%12===0)await save('dr-terrible-'+n);
        const spawned=m.entities.filter(x=>[0x1e23,0x1e24,0x1e25].includes(x.kind)&&distance(x.pos,p)<16);
        // SummonMonster can put an Experiment in firing range between observations; do not idle through this transition.
        const summoning=state.diagnostic.xml[id]?.states.some(s=>s.name==='SummonMonster');
        const closeAdd=add&&(shielded||distance(add.pos,p)<8.5);
        const threatened=spawned.some(x=>distance(x.pos,p)<9.6&&line(p,x.pos));
        // Keep a firing lane while Dr Terrible wanders. Repositioning for each fraction of a tile wastes the damage window.
        const focusAdd=add&&(shielded||distance(add.pos,p)<5);
        const target=turret??(focusAdd?add:e);
        const quietMs=shielded?(summoning||threatened?850:3200):d>=6.4&&!closeAdd?1800:900;
        // Crystal Wand pierces the experiments intercepting single-hit shots across Dr Terrible's room.
        await volley(target,v,m,7,{quietMs,rangeSlack:.7,firing:target!==e||!shielded,hazards,boss:e});
        continue;
      }
      const v=await game.observe(),blocked=stalled>3;
      if(blocked||acid.has(tile(v.position,m))||distance(e.pos,v.position)>9.6){stalled=0;await walk(e.pos,blocked?2:8,true,!blocked);}
      else if(!state.view.objects.some(o=>o.id===id&&o.hp>0))await act({keys:[],duration_ms:300});
      else {
        const live=await game.inspect(),hazards=bossHazards(m,live.view.position);
        if(hazards.length||m.entities.some(x=>x.kind===BOSS&&distance(x.pos,live.view.position)<7))await volley(e,live.view,m,8.5,{quietMs:650,hazards});
        else await act(fire(e,v,2100));
      }
    }
    await save('failed-fight');throw Error('Native fight exceeded its bound: '+id);
  }
  const initial=await scene(),bossStart=initial.entities.find(e=>e.kind===BOSS);
  assert(bossStart,'Generated Lab has no Dr Terrible');
  const {blocked}=collision(initial),world=(await game.observe()).session.world;
  const supportFixtures=[-1,1].map((side,i)=>{
    const positions=Array.from({length:9},(_,j)=>({x:bossStart.home.x+side*(2+j%3),y:bossStart.home.y+12+Math.floor(j/3)}));
    const position=positions.find(p=>!blocked.has(tile(p,initial)));
    assert(position,'No dry staging lane south of Dr Terrible for Lab support '+i);
    const {worldSeed,...kit}=fixture;
    return {...kit,world,position,seed:fixture.seed+i+1,equipment:[0xb3f,...fixture.equipment.slice(1)],backpack:[...fixture.backpack.slice(0,7),-1]};
  });
  await writeFile(output('creation-prepared.json'),JSON.stringify({scope:'Enrollment fixtures only; no runtime cheats',fixture,supportFixtures}));
  for(const spec of supportFixtures)supports.push(await game.join(spec));
  const entered=await save('entered'),enrolled=await Promise.all(supports.map(c=>c.inspect()));
  game.check('Lab party starts on distinct fresh characters in the same Lab',new Set([entered,...enrolled].map(s=>s.view.session.account)).size===3&&enrolled.every(s=>s.view.lifecycle.alive&&s.view.session.world===world));
  const creation=initial.entities.find(e=>e.kind===CREATION);
  assert(creation,'Generated Lab has no Horrific Creation');
  game.check('Creation is invulnerable before the Priest approaches',Boolean(creation.condition&(1<<24)));
  game.check('generated Lab holds the authored ten Tesla Coils',initial.entities.filter(e=>e.kind===COIL).length===10);
  const walls=initial.entities.filter(e=>e.kind===WALL).sort((a,b)=>a.pos.x-b.pos.x);assert.equal(walls.length,4);
  // The coils guard the last rooms on the generated route (src/dungeon/lab.rs): follow it to Dr Terrible, clearing them.
  const nearest=async()=>{const at=(await game.observe()).position;return (await scene()).entities.filter(e=>e.kind===COIL).sort((a,b)=>distance(a.pos,at)-distance(b.pos,at))[0];};
  for(let coil;(coil=await nearest());)await defeat(coil.id,COIL);
  const coiled=await save('coils-destroyed');
  game.check('native shots destroy every Tesla Coil while the Emergency Walls stay sealed',!coiled.diagnostic.entities.some(e=>e.kind===COIL)&&(await scene()).entities.filter(e=>e.kind===WALL).length===4);
  const doctor=(await scene()).entities.find(e=>e.kind===BOSS);
  assert(doctor,'No Dr Terrible after the coil sweep');
  await act({keys:[],duration_ms:350,drag:{from:{x:766,y:472},to:{x:634,y:384}}});
  game.check('ordinary equipment drag switches to the prepared piercing wand for Dr Terrible',(await game.observe()).character.items[0]===0xb3f);
  await defeat(doctor.id,BOSS);
  const cleared=await save('boss-defeated');
  const db=new DatabaseSync(output('native/state/game.db'),{readOnly:true});
  let rows;
  try{rows=db.prepare("SELECT event,payload FROM world_events WHERE kind='terminal' AND json_extract(payload,'$.payload.data.defeat.actor')=?").all(doctor.id);}
  finally{db.close();}
  assert.equal(rows.length,1,'Dr Terrible defeat must have exactly one durable terminal event');
  const {defeat:journaledDefeat,actions,ledger}=JSON.parse(rows[0].payload).payload.data;
  const {account,character}=cleared.view.session,before=entered.diagnostic.character.mad_labs,after=cleared.diagnostic.character.mad_labs;
  const progress=actions.progress.filter(([c])=>c.account===account&&c.id===character);
  game.check('Dr Terrible defeat has one journaled Mad Lab clear for the same living Priest',journaledDefeat.kind===BOSS&&actions.publication.reason==='Defeated'&&actions.publication.template==='mad-lab'&&actions.publication.clear_registration===true&&progress.length===1&&progress[0][0].mad_labs===before+1&&after===before+1&&account===entered.view.session.account&&character===entered.view.session.character&&cleared.view.lifecycle.alive);
  const participants=[entered,...enrolled],finished=[cleared,...await Promise.all(supports.map(c=>c.inspect()))];
  const damage=participants.map((s,i)=>{
    const {account,character}=s.view.session,v=finished[i].view;
    assert(v.lifecycle.alive&&v.session.account===account&&v.session.character===character,'Lab party identity changed');
    return {account,character,damage:ledger.find(r=>r.character===character)?.effective_damage??0,
      journaledClear:actions.progress.find(([c])=>c.account===account&&c.id===character)?.[0].mad_labs??null};
  });
  await writeFile(output('lab-terminal.json'),JSON.stringify({event:rows[0].event,actor:doctor.id,before,after,damage}));
  game.check('each living enrolled Lab Priest contributes native damage and receives one clear in the same terminal event',damage.every((r,i)=>r.damage>0&&r.journaledClear===participants[i].diagnostic.character.mad_labs+1&&finished[i].diagnostic.character.mad_labs===r.journaledClear));
}
