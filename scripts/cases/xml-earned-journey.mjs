import assert from 'node:assert/strict';
import {readFile,writeFile,appendFile} from 'node:fs/promises';
import {join} from 'node:path';
import {setTimeout as delay} from 'node:timers/promises';
import {connect} from '../../scripts/record-demo.mjs';

// Earned journey through Castle transfer, escape, and ordinary death/replacement. Guardian admission is a separate prepared-start case.
// After enrollment, only ordinary native input and reads: no protection, teleport, vitals or item cheats.
export const realmSeed=4; // world 1 is classic-world-11 with 25 natural Coil temples
const COIL=0xdc2,JUNGLE_PORTAL=0x733,MIXCOATL=0xdc3,REALM_PORTAL=0x704,ENTRANCE=0x712,WIZARD=0x30e;
const HOLY_WATER=0xc21,MAGIC=0xa23,HEALTH=0x707,CROWN=0xc26;
export const PRIEST={class:0x310,level:19,seed:4101,equipment:[0xaf8,0xc1e,0xb07,CROWN],backpack:[-1,HOLY_WATER,HEALTH,HEALTH,MAGIC,MAGIC,MAGIC,-1],stats:[670,385,50,25,50,50,40,75]};
const PEER_RING=0xac5,PEER={class:WIZARD,level:1,seed:4102,world:-2,position:'spawn',equipment:[-1,-1,-1,-1],backpack:[PEER_RING,-1,-1,-1,-1,-1,-1,-1],stats:null};
const out=()=>process.env.QA_RESULTS_DIR;
let prepared,api,headers,origin;

async function control(){
  const root=join(out(),'native');origin=JSON.parse(await readFile(join(root,'result.json'),'utf8')).origin;
  headers={Authorization:'Bearer '+(await readFile(join(root,'state','control.token'),'utf8')).trim(),'Content-Type':'application/json'};
  let serial=0;
  api=async command=>{const r=await fetch(origin+'/dev/testing',{method:'POST',headers,body:JSON.stringify({id:'journey-'+(++serial),command}),signal:AbortSignal.timeout(10000)});const s=await r.json();assert(r.ok&&s.status!=='failed',JSON.stringify(s));return s;};
}
const map=async player=>{const r=await fetch(origin+'/dev/map?player='+player,{headers,signal:AbortSignal.timeout(30000)});assert(r.ok);return r.json();};
const distance=(a,b)=>Math.hypot(a.x-b.x,a.y-b.y);
// Wand of Retribution reaches 10 tiles (objects.xml); Coil (actors.xml:10270) and Mixcoatl Stage1/Stage2 shoot or follow within 8.
const STANDOFF=9;
// Coil eggs hatch Great Temple Snakes (actors.xml 0x0dc0/0x0dc1) that follow within 8 and shoot within 6.
const SNAKE=0xdc0;

export async function prepare(access){
  await control();
  // Before enrollment: the shore Realm's last hero is a 1-HP Cyclops God and its entrance reads (0/1).
  const capacity=await api({op:'prepare_realm_capacity',limit:1}),closure=await api({op:'prepare_realm_closure',world:-3});
  const info=await access({op:'world_info',world:1}),[x,y]=info.spawn_points[0];
  const scout=await access({op:'spawn',spec:{...PRIEST,guid:'journey-scout',world:1,position:{x,y},controller:'direct',profile:null}});
  const realm=await map((await access({op:'inspect',actor:scout.result.actor})).diagnostic.player);
  const coils=realm.entities.filter(e=>e.kind===COIL);
  const near=c=>coils.map(o=>distance(o.pos,c.pos)).sort((a,b)=>a-b).slice(1,4).reduce((a,b)=>a+b,0);
  const first=coils.sort((a,b)=>near(a)-near(b))[0];
  const order=[first,...coils.filter(c=>c!==first).sort((a,b)=>distance(a.pos,first.pos)-distance(b.pos,first.pos)).slice(0,5)];
  const route=planner(realm),start=route.free({x:first.pos.x-9,y:first.pos.y});
  prepared={realmSeed,layout:info.realm_layout,capacity:capacity.result,closure:closure.result,coils:order.map(c=>({id:c.id,pos:c.pos})),start,priest:PRIEST,peer:PEER,
    scope:'Prepared: Realm capacity/closure (1-HP final Cyclops God), level-19 Priest with maxed base stats with explicit stats/gear/potions placed 9 tiles from a scout-selected natural Coil, and one Wizard trade peer. Earned: every kill, drop, portal, dungeon, reward, exit, reconnect, trade, equip, Realm closure, Castle transfer and escape, death and replacement.'};
  await writeFile(join(out(),'prepared-setup.json'),JSON.stringify(prepared,null,1));
  return {...PRIEST,world:1,position:start};
}

// 8-direction least-cost search over /dev/map blocked cells; each straight run becomes one held key combination.
// Cells touching a blocked cell cost more, so routes keep off walls the timed presses would otherwise graze and stick to.
function planner(m){
  const W=m.width,H=m.height,blocked=new Uint8Array(W*H);for(const [x,y] of m.blocked)blocked[y*W+x]=1;
  const free=(x,y)=>x>=0&&y>=0&&x<W&&y<H&&!blocked[y*W+x];
  const hugs=(x,y)=>[-1,0,1].some(dy=>[-1,0,1].some(dx=>!free(x+dx,y+dy)));
  const marks=[];
  // nearest: end at the reachable cell closest to `to` (used to choose a refuge, never to excuse a missing route).
  const route=(from,to,margin=60,nearest=false)=>{
    const sx=Math.floor(from.x),sy=Math.floor(from.y),tx=Math.floor(to.x),ty=Math.floor(to.y);
    const x0=Math.max(0,Math.min(sx,tx)-margin),y0=Math.max(0,Math.min(sy,ty)-margin),x1=Math.min(W-1,Math.max(sx,tx)+margin),y1=Math.min(H-1,Math.max(sy,ty)+margin),w=x1-x0+1;
    const key=(x,y)=>(y-y0)*w+(x-x0),prev=new Int32Array(w*(y1-y0+1)).fill(-1),cost=new Int32Array(prev.length).fill(-1),buckets=[[key(sx,sy)]];prev[key(sx,sy)]=key(sx,sy);cost[key(sx,sy)]=0;
    let best=key(sx,sy),bestDistance=Infinity;
    search:for(let c=0;c<buckets.length;c++)for(const k of buckets[c]??[]){
      if(cost[k]!==c)continue;
      const x=x0+k%w,y=y0+Math.floor(k/w),d=Math.hypot(x-tx,y-ty);if(d<bestDistance){bestDistance=d;best=k;}if(x===tx&&y===ty)break search;
      for(const [dx,dy] of [[1,0],[-1,0],[0,1],[0,-1],[1,1],[1,-1],[-1,1],[-1,-1]]){
        const nx=x+dx,ny=y+dy,nk=key(nx,ny),nc=c+(hugs(nx,ny)?4:1);
        if(nx<x0||ny<y0||nx>x1||ny>y1||!free(nx,ny)||dx&&dy&&!(free(x+dx,y)&&free(x,y+dy))||cost[nk]!==-1&&cost[nk]<=nc)continue;
        prev[nk]=k;cost[nk]=nc;(buckets[nc]??=[]).push(nk);
      }
    }
    assert(nearest||prev[key(tx,ty)]!==-1,'No walkable route '+JSON.stringify({from,to,walker_marked:marks}));
    const cells=[];for(let k=nearest?best:key(tx,ty);;k=prev[k]){cells.unshift({x:x0+k%w,y:y0+Math.floor(k/w)});if(prev[k]===k)break;}
    return cells;
  };
  // Some scenery collides without appearing in /dev/map; walkers mark cells they could not enter.
  route.block=(x,y)=>{blocked[y*W+x]=1;marks.push([x,y]);};
  // An exact walkable point is kept; otherwise the nearest roomy cell center stands in for it.
  route.free=p=>{if(free(Math.floor(p.x),Math.floor(p.y)))return p;for(let r=0;r<6;r++)for(let dy=-r;dy<=r;dy++)for(let dx=-r;dx<=r;dx++){const x=Math.floor(p.x)+dx,y=Math.floor(p.y)+dy;if(free(x,y)&&free(x+1,y)&&free(x-1,y)&&free(x,y+1)&&free(x,y-1))return{x:x+.5,y:y+.5};}assert.fail('No open cell near '+JSON.stringify(p));};
  return route;
}
// Camera angle -pi/4: world axes are two-key screen diagonals, world diagonals are single keys.
const keysFor=(dx,dy)=>{const x=dx-dy,y=dx+dy;return [x>0?'d':x<0?'a':null,y>0?'s':y<0?'w':null].filter(Boolean);};
const maxHp=c=>Array.isArray(c.stats)?c.stats[0]:c.stats.max_hp;
const volley=v=>v.character.hp<maxHp(v.character)*.6?1000:2500;
// Only the direction matters; keep the cursor close to the player to avoid the HUD consuming a press.
const aim=(a,b)=>{const x=35.355*(b.x-a.x-b.y+a.y),y=35.355*(b.x-a.x+b.y-a.y),scale=Math.max(1,Math.hypot(x,y)/90);return {x:300+x/scale,y:325+y/scale};};

async function tools(game){
  if(!api)await control();
  const log=async(label,client=game)=>{const s=await client.inspect();await appendFile(join(out(),'journey.jsonl'),JSON.stringify({label,actor:client.actor,...s})+'\n');return s;};
  const capture=(name,client=game)=>client.act({keys:[],duration_ms:700},{name,when:()=>true});
  const speed=v=>(4+5.6*(Array.isArray(v.character.stats)?v.character.stats[4]:v.character.stats.speed)/75)/1000;
  const events=async()=>(await readFile(join(out(),'native','server.log'),'utf8')).split('\n').filter(l=>l.startsWith('{')).map(l=>JSON.parse(l).fields??{});
  let plan,planWorld;
  async function routeFor(v){if(planWorld!==v.session.world){plan=planner(await map(v.session.player));planWorld=v.session.world;}return plan;}
  // Priest upkeep is an ordinary prefix: Holy Water for Sick/Quiet, Health Potion when low, Magic Potion when dry, Tome heal.
  function upkeep(v){
    const steps=[],c=v.character,slot=k=>c.items.findIndex((i,n)=>n>=4&&i===k),use=k=>steps.push({keys:[String(slot(k)-3)],duration_ms:150});
    if(c.conditions&(2|16|128|256|8192|33554432)&&slot(HOLY_WATER)>=4)use(HOLY_WATER);
    if(c.hp<maxHp(c)*.62&&slot(HEALTH)>=4&&(!(c.conditions&16)||steps.length))use(HEALTH);
    const ready=c.class===0x310&&v.time_ms>=c.ability_ready_ms&&(!(c.conditions&18)||slot(HOLY_WATER)>=4);
    if(ready&&c.hp<maxHp(c)*.82){
      if(c.mp<100&&slot(MAGIC)>=4)use(MAGIC);
      if(c.mp>=100||slot(MAGIC)>=4)steps.push({keys:[' '],duration_ms:200});
    }
    return steps;
  }
  const act=(steps,client=game)=>client.act(steps.map(input=>({actor:client.actor,input})));
  async function steer(v,target,{fire,margin,steps=[...upkeep(v)]}={}){
    const route=await routeFor(v),goal=route.free(target),cells=route(v.position,goal,margin),prefix=steps.length;let total=steps.reduce((sum,s)=>sum+s.duration_ms,0),at={x:Math.floor(v.position.x)+.5,y:Math.floor(v.position.y)+.5};
    if(distance(v.position,at)>.2){const d=distance(v.position,at);steps.push({keys:keysFor(Math.sign(Math.round((at.x-v.position.x)*4)),Math.sign(Math.round((at.y-v.position.y)*4))),duration_ms:Math.max(30,Math.round(d/speed(v)))});}
    let i=1;
    for(;i<cells.length&&steps.length<30&&total<8500;){
      const dx=cells[i].x-cells[i-1].x,dy=cells[i].y-cells[i-1].y;let j=i;
      while(j+1<cells.length&&cells[j+1].x-cells[j].x===dx&&cells[j+1].y-cells[j].y===dy)j++;
      const ms=Math.round((j-i+1)*Math.hypot(dx,dy)/speed(v));steps.push({keys:keysFor(dx,dy),duration_ms:Math.min(ms,9000-total),...(fire?{mouse:fire}:{})});total+=ms;i=j+1;
    }
    // Cell centers end within half a tile; finish on the exact point when the whole plan fits this act.
    const end={x:cells.at(-1).x+.5,y:cells.at(-1).y+.5},rest=distance(end,goal);
    if(i>=cells.length&&total<8500&&rest>.15)steps.push({keys:keysFor(Math.sign(Math.round((goal.x-end.x)*4)),Math.sign(Math.round((goal.y-end.y)*4))),duration_ms:Math.max(30,Math.round(rest/speed(v)))});
    steps.next=cells[1];steps.arrived=steps.length===prefix;
    return steps;
  }
  // permitDeath walks under the bridge's death-permitted act and returns its committed death receipt.
  async function walk(target,{tolerance=.7,client=game,fire,permitDeath,margin}={}){
    let last,next,centred=false;
    const press=async steps=>{if(!permitDeath)return void await act(steps,client);const r=await client.die(steps);if(r.status==='death_committed')return r;};
    for(let n=0;n<30;n++){
      const v=await client.observe();if(distance(v.position,target)<tolerance)return v;
      if(last&&next&&distance(v.position,last)<.3){
        // A timed press can stop just off a wall, where the client refuses sideways moves (job 11d62c73); recentre one axis
        // at a time before concluding the next cell is unmapped scenery.
        if(!centred){
          centred=true;const off=[[0,Math.floor(v.position.y)+.5-v.position.y],[Math.floor(v.position.x)+.5-v.position.x,0]].filter(([dx,dy])=>Math.abs(dx+dy)>.05);
          const r=off.length&&await press(off.map(([dx,dy])=>({keys:keysFor(Math.sign(dx),Math.sign(dy)),duration_ms:Math.max(40,Math.round(Math.abs(dx+dy)/speed(v)))})));if(r)return r;
          if(off.length)continue;
        }
        (await routeFor(v)).block(next.x,next.y);
      }else centred=false;
      const steps=await steer(v,target,{fire,margin});last=v.position;next=steps.next;
      // Already on the nearest walkable stand-in for a blocked target: nothing left to press.
      if(steps.arrived)return v;
      const r=await press(steps);if(r)return r;
    }
    assert.fail('Native route did not reach '+JSON.stringify(target));
  }
  // Hatched snakes guard the Coil's portal; fire at the nearest within `radius` of it until the game-time deadline.
  // Crowded snakes absorb shots aimed at neighbours, so the deadline, not a per-snake count, bounds the sweep.
  async function clearSnakes(center,radius,until){
    for(let n=0;n<24;n++){
      const s=await game.inspect();let v=s.view;if(v.time_ms>=until)return;
      const snake=s.diagnostic.entities.filter(e=>e.kind===SNAKE&&distance(e.pos,center)<radius).sort((a,b)=>distance(a.pos,v.position)-distance(b.pos,v.position))[0];
      if(!snake)return;
      const d=distance(snake.pos,v.position);
      if(d>8)v=await walk({x:snake.pos.x+(v.position.x-snake.pos.x)*7/d,y:snake.pos.y+(v.position.y-snake.pos.y)*7/d},{tolerance:1});
      await act([...upkeep(v),{keys:[],mouse:aim(v.position,snake.pos),duration_ms:800}]);
    }
  }
  // Ordinary wand fire at the target's latest position until the server no longer has it.
  async function defeat(id,label,range=STANDOFF,{rounds=40,shield=false,sample=false,northOf=Infinity}={}){
    for(let n=0;n<rounds;n++){
      const s=await game.inspect(),e=s.diagnostic.entities.find(x=>x.id===id);if(!e)return log(label);
      let v=s.view;const d=distance(e.pos,v.position);
      // A wall-side standoff ends on its walkable stand-in; fire from there rather than re-planning the same walk.
      if(d>range+.8||d<range-1.5){
        const goal={x:e.pos.x+(v.position.x-e.pos.x)*range/Math.max(d,.1),y:Math.min(northOf,e.pos.y+(v.position.y-e.pos.y)*range/Math.max(d,.1))};
        if(distance(goal,v.position)>1.2)v=await walk(goal,{tolerance:1});
      }
      const inputs=upkeep(v);
      if(shield&&(e.condition&((1<<23)|(1<<24))))inputs.push({keys:[],duration_ms:800});
      else inputs.push({keys:[],mouse:aim(v.position,e.pos),duration_ms:volley(v)});
      if(sample&&n%5===0)await log(label+'-combat');
      await act(inputs);
    }
    assert.fail('Native fire did not defeat '+label);
  }
  async function enter(client=game){
    const from=(await client.observe()).session.world;
    // The portal panel's Enter button can be absent on the first frame after arrival; click again if nothing started.
    for(let n=0;n<9&&(await client.observe()).session.world===from;n++)await client.act({keys:[],duration_ms:n%3?600:1200,...(n%3?{}:{mouse:{x:700,y:575}})});
    const s=await log('entered',client);assert.notEqual(s.view.session.world,from,'Portal interaction did not change world');return s;
  }
  return {log,capture,events,routeFor,upkeep,act,steer,walk,defeat,enter,clearSnakes};
}
export async function run(game){
  const {log,capture,events,routeFor,upkeep,act,steer,walk,defeat,enter,clearSnakes}=await tools(game);
  // Earned 1: ordinary kills of natural Coils until one actually drops its Forbidden Jungle portal.
  const peer=await game.join(PEER);
  const initial=await log('prepared-start');
  game.check('prepared Priest starts alive in the seeded natural Realm with no clears',initial.view.map.name==='Realm'&&initial.view.lifecycle.alive&&initial.diagnostic.character.jungles===0);
  let portal;const attempts=[];
  for(const coil of prepared.coils){
    const before=await game.inspect();
    if(!before.diagnostic.entities.some(e=>e.id===coil.id))await walk({x:coil.pos.x-STANDOFF,y:coil.pos.y},{tolerance:2});
    const killed=await defeat(coil.id,'coil-'+coil.id);
    portal=killed.diagnostic.entities.find(e=>e.kind===JUNGLE_PORTAL&&distance(e.pos,coil.pos)<3);
    attempts.push({coil:coil.id,xp:[before.view.character.experience,killed.view.character.experience],portal:portal?.id??null});
    await writeFile(join(out(),'coil-attempts.json'),JSON.stringify(attempts,null,1));
    if(portal)break;
  }
  assert(portal,'No natural Coil dropped its 40% Jungle portal in the bounded sample');
  const earned=await log('portal-earned');
  game.check('ordinary native kills earn XP from natural Coils',earned.view.character.experience>initial.view.character.experience&&earned.diagnostic.character.kills>initial.diagnostic.character.kills);
  // The portal decays 30 s after the kill (actors.xml Forbidden Jungle Portal); keep ~12 s to walk, capture and enter.
  await clearSnakes(portal.pos,6,earned.view.time_ms+18000);
  await walk(portal.pos,{tolerance:.5});await capture('earned-jungle-portal');
  const jungle=await enter();
  game.check('earned Coil portal enters Forbidden Jungle with the same living character',jungle.view.map.name==='Forbidden Jungle'&&jungle.view.session.character===initial.view.session.character&&jungle.view.lifecycle.alive);

  // Earned 2: generated route, ordinary room fights, Mixcoatl activation/phases, contributor bag, completion exit.
  await game.act({keys:[],duration_ms:1500});
  const generated=(await events()).findLast(e=>e.message==='generated branching Forbidden Jungle');assert(generated);
  const rooms=JSON.parse(generated.layout.replaceAll('(','[').replaceAll(')',']')),path=[],center=i=>({x:20+rooms[i][0]*20+.5,y:20+rooms[i][1]*20+.5});
  for(let i=generated.boss_room;i!==0;i=rooms[i][2])path.unshift(i);
  const hostile=[0xdd3,0xdd0,0xdcd,0xdcf,0xdd2,0xdd1,0xdce,0xdcc];
  for(const i of path.slice(0,-1)){
    await walk(center(i),{tolerance:3});
    for(let n=0;n<12;n++){
      const s=await game.inspect(),enemies=s.diagnostic.entities.filter(e=>hostile.includes(e.kind)&&distance(e.pos,s.view.position)<10).sort((a,b)=>distance(a.pos,s.view.position)-distance(b.pos,s.view.position));
      if(!enemies.length)break;
      await act([...upkeep(s.view),...enemies.slice(0,3).map(e=>({keys:[],mouse:aim(s.view.position,e.pos),duration_ms:1500}))]);
    }
    await log('room-'+i);
  }
  const arena=center(generated.boss_room);await walk({x:arena.x-STANDOFF,y:arena.y},{tolerance:2});
  await capture('mixcoatl-arrival');
  // Recovered Mixcoatl (actors.xml 0x0dc3): Stage1 attacks, then below 50% HP Rage into the circling Stage2.
  const stages=new Set(),boss=(await game.inspect()).diagnostic.entities.find(e=>e.kind===MIXCOATL);assert(boss,'Generated Mixcoatl missing');
  const phase=s=>(s.diagnostic.xml[boss.id]?.states??[]).map(x=>x.name).find(x=>['Ini','Stage1','SummonChoice','Rage','Summon','Stage2'].includes(x));
  let sampling,sampleError;const sampler=setInterval(()=>{if(!sampling)sampling=game.inspect().then(s=>{const p=phase(s);if(p)stages.add(p);}).catch(e=>{sampleError=e;}).finally(()=>{sampling=null;});},150);
  // Short volleys from the standoff keep each XML phase observable between inspections, even at full damage output.
  try{for(let n=0;n<150;n++){
    const s=await game.inspect(),e=s.diagnostic.entities.find(x=>x.id===boss.id);if(!e)break;
    const stage=phase(s);stages.add(stage);
    let v=s.view;const d=distance(e.pos,v.position);
    if(Math.abs(d-STANDOFF)>1)v=await walk({x:e.pos.x+(v.position.x-e.pos.x)*STANDOFF/d,y:e.pos.y+(v.position.y-e.pos.y)*STANDOFF/d},{tolerance:1});
    await act([...upkeep(v),{keys:[],mouse:aim(v.position,e.pos),duration_ms:600}]);
  }}finally{clearInterval(sampler);if(sampling)await sampling;}
  if(sampleError)throw sampleError;
  await appendFile(join(out(),'journey.jsonl'),JSON.stringify({label:'mixcoatl-stages',stages:[...stages]})+'\n');
  const cleared=await log('mixcoatl-defeated');
  game.check('ordinary native fire defeats Mixcoatl through its XML Stage1 and Stage2 phases',!cleared.diagnostic.entities.some(e=>e.id===boss.id)&&stages.has('Stage1')&&stages.has('Stage2'));
  game.check('one Jungle clear is committed',cleared.diagnostic.character.jungles===1);
  const mine=cleared.diagnostic.entities.filter(e=>e.items?.some(i=>i>=0)&&e.extra_stats?.some(([stat,owner])=>stat===54&&owner===cleared.view.session.account)&&distance(e.pos,arena)<14);
  const bag=mine[0];assert(bag,'No contributor-owned reward bag');
  await walk(bag.pos,{tolerance:.4});await capture('jungle-contributor-bag');
  const slot=bag.items.findIndex(i=>i>=0),reward=bag.items[slot],free=(await game.observe()).character.items.indexOf(-1);
  assert(free>=4,'No empty backpack slot for the earned reward');
  const beforePick=await game.observe();
  await game.act({keys:[],duration_ms:350,drag:{from:{x:634+(slot%4)*44,y:528+Math.floor(slot/4)*44},to:{x:634+((free-4)%4)*44,y:428+Math.floor((free-4)/4)*44}}});
  const picked=await log('reward-picked');
  game.check('native drag takes the contributor reward exactly once',picked.view.character.items[free]===reward&&picked.view.character.inventory_revision===beforePick.character.inventory_revision+1);
  const exit=picked.diagnostic.entities.find(e=>e.kind===REALM_PORTAL&&e.published);assert(exit,'Completion exit missing');
  await walk(exit.pos,{tolerance:.5});await capture('jungle-completion-exit');
  const returned=await enter();
  game.check('completion exit returns the same living character to its Realm with reward and one clear',returned.view.map.name==='Realm'&&returned.view.session.world===initial.view.session.world&&returned.view.session.character===initial.view.session.character&&returned.view.lifecycle.alive&&returned.view.character.items[free]===reward&&returned.diagnostic.character.jungles===1);
  game.check('earned Jungle XP survives exit',returned.view.character.experience>initial.view.character.experience);

  // Earned 3: ordinary escape saves and reconnects the same character in Nexus.
  await game.act({keys:['Insert'],duration_ms:1500});
  for(let n=0;n<8&&(await game.observe()).world!=='Nexus';n++)await game.act({keys:[],duration_ms:600});
  const nexus=await log('nexus-reconnect');
  game.check('escape reconnect persists level, XP, reward and inventory revision',nexus.view.map.name==='Nexus'&&nexus.view.session.character===initial.view.session.character&&nexus.view.session.player!==returned.view.session.player&&nexus.view.character.experience===returned.view.character.experience&&JSON.stringify(nexus.view.character.items)===JSON.stringify(returned.view.character.items)&&nexus.view.character.inventory_revision===returned.view.character.inventory_revision);

  // Earned 4: trade the earned reward to a prepared peer for a ring, then equip it through the inventory UI.
  game.check('equipped trade peer is nearby at the ordinary Nexus spawn',distance((await peer.observe()).position,nexus.view.position)<5);
  const config=JSON.parse(await readFile(join(out(),'native','native.json'),'utf8'));
  const pages=await Promise.all(config.native.map(b=>connect({...b,url:origin+'/',helper:config.browserTools})));
  const page=client=>pages[config.native.findIndex(b=>b.actor===client.actor)];
  async function chat(client,text){
    const action=client.act({keys:[],duration_ms:2500});
    const typing=(async()=>{
      for(let n=0;(await client.observe()).paused;n++){assert(n<250,'Chat action did not unpause');await delay(20);}
      const enter=async()=>{for(const type of ['keyDown','keyUp'])await page(client).call('Input.dispatchKeyEvent',{type,key:'Enter',code:'Enter',windowsVirtualKeyCode:13});};
      await enter();await page(client).call('Input.insertText',{text});await enter();
    })();
    for(const r of await Promise.allSettled([action,typing]))if(r.status==='rejected')throw r.reason;
  }
  const mineBefore=await log('trade-before'),peerBefore=await log('peer-before',peer);
  await chat(peer,'/trade '+mineBefore.diagnostic.character.name);await chat(game,'/trade '+peerBefore.diagnostic.character.name);
  // Trade panel: own backpack slot n at (634+(n%4)*44,310+row*44); Trade button (749,574).
  const own=n=>({x:634+((n-4)%4)*44,y:310+Math.floor((n-4)/4)*44});
  await game.act({keys:[],mouse:own(free),duration_ms:300});await peer.act({keys:[],mouse:own(4),duration_ms:3500});
  await capture('trade-offers');
  await game.act({keys:[],mouse:{x:749,y:574},duration_ms:300});await peer.act({keys:[],mouse:{x:749,y:574},duration_ms:1500});
  const traded=await log('trade-after'),peerAfter=await log('peer-after',peer);
  game.check('native trade swaps the earned reward for the peer ring atomically',traded.view.character.items[free]===PEER_RING&&peerAfter.view.character.items[4]===reward&&traded.view.character.inventory_revision>mineBefore.view.character.inventory_revision);
  const ring=traded.view.character.items[3];
  await game.act({keys:[],duration_ms:400,drag:{from:{x:634+((free-4)%4)*44,y:428+Math.floor((free-4)/4)*44},to:{x:766,y:384}}});
  const equipped=await log('ring-equipped');
  game.check('inventory drag equips the traded ring and keeps the previous ring',equipped.view.character.items[3]===PEER_RING&&equipped.view.character.items[free]===ring&&maxHp(equipped.view.character)!==maxHp(traded.view.character));
  // Switch back to the prepared Crown after verifying ordinary ring equip.
  await game.act({keys:[],duration_ms:400,drag:{from:{x:634+((free-4)%4)*44,y:428+Math.floor((free-4)/4)*44},to:{x:766,y:384}}});
  assert.equal((await game.observe()).character.items[3],CROWN);
  for(const p of pages)p.close();

  // Earned 5: Oryx progression. The prepared closure Realm's entrance; native kill of its last hero; real countdown.
  const hall=async()=>{for(let n=0;n<20;n++){const s=await game.inspect(),e=s.diagnostic.entities.filter(x=>x.kind===ENTRANCE);if(e.length===2)return s;await act([{keys:['d','w'],duration_ms:1500}]);}assert.fail('Two Realm entrances not revealed');};
  const lobby=await hall(),names=lobby.view.objects.filter(o=>o.kind===ENTRANCE);
  const closing=lobby.diagnostic.entities.find(e=>e.kind===ENTRANCE&&names.find(o=>o.id===e.id)?.name.endsWith('(0/1)'));assert(closing,'Prepared closure entrance missing');
  await walk(closing.pos,{tolerance:.5});
  const topped=await game.observe(),magic=topped.character.items.findIndex((k,i)=>i>=4&&k===MAGIC);
  if(topped.character.mp<topped.character.stats.max_mp-50&&magic>=4)await act([{keys:[String(magic-3)],duration_ms:150}]);
  const realm=await enter();
  game.check('ordinary entrance reaches the prepared closure Realm',realm.view.session.world===-3);
  // The prepared arrival stacks the 1-HP hero with its camp, which absorbs point-blank shots and lands 500-800 HP within
  // three seconds (jobs 5bc76709, f37898d0); break away toward the refuge first, then fire as the hero follows out of it.
  const here=realm.view.position,hero=prepared.closure.hero,refugeCell=(await routeFor(realm.view))(here,{x:here.x-25,y:here.y+25},60,true).at(-1),refuge={x:refugeCell.x+.5,y:refugeCell.y+.5};
  const killed=async()=>(await events()).some(e=>e.message==='enemy defeated'&&e.entity===hero);
  const bearing=distance(refuge,here);
  await act(await steer(realm.view,{x:here.x+(refuge.x-here.x)*9/bearing,y:here.y+(refuge.y-here.y)*9/bearing},{steps:[]}));
  // Camp minions stay within 5 tiles of spawn and shoot 3 tiles (actors.xml Cyclops*), so fire from 9 tiles out.
  for(let n=0;n<12&&!await killed();n++){
    const s=await game.inspect(),v=s.view,target=s.diagnostic.entities.find(e=>e.id===hero)?.pos??here,d=distance(v.position,here);
    const post={x:here.x+(v.position.x-here.x)*9/d,y:here.y+(v.position.y-here.y)*9/d};
    const moves=distance(v.position,post)>1.5?(await steer(v,post,{steps:[]})).slice(0,4):[];
    await act([...upkeep(v),...moves,{keys:[],mouse:aim(moves.length?post:v.position,target),duration_ms:volley(v)+1000}]);
  }
  const heroDefeat=await log('final-hero-defeated');
  game.check('native shot defeats the last Realm hero',await killed());
  // Survive the 120s countdown away from the Cyclops camp with ordinary movement and healing.
  for(let n=0;n<30&&(await game.observe()).world==='Realm';n++){
    const v=await game.observe();
    if(distance(v.position,refuge)>2)await walk(refuge,{tolerance:2});
    else {
      // Leave a complete action window across the earthquake and LOAD. A pause on the
      // transfer tick can strand the browser just before reconnect (job f7fe2952).
      const until=heroDefeat.view.time_ms+122000-v.game_ms;
      await act([...upkeep(v),{keys:[],duration_ms:until>500?Math.min(8000,until):9500}]);
    }
  }
  const castle=await log('castle-transfer');
  game.check('real closure countdown transfers the same living character to Oryx\'s Castle',castle.view.map.name==="Oryx's Castle"&&castle.view.session.character===initial.view.session.character&&castle.view.lifecycle.alive);
  await capture('oryx-castle-arrival');

  await game.act({keys:['Insert'],duration_ms:1800});
  const escaped=await log('castle-escape');
  game.check('ordinary Castle escape keeps the same living character',escaped.view.map.name==='Nexus'&&escaped.view.session.character===initial.view.session.character&&escaped.view.lifecycle.alive);

  // Earned 6: death-permitted trek into a natural god camp.
  let gate;for(let n=0;n<20&&!gate;n++){const s=await game.inspect();gate=s.diagnostic.entities.filter(e=>e.kind===ENTRANCE).sort((a,b)=>distance(a.pos,s.view.position)-distance(b.pos,s.view.position))[0];if(!gate)await act([{keys:['d','w'],duration_ms:1500}]);}
  assert(gate,'No open Realm entrance after closure');
  await walk(gate.pos,{tolerance:.5});const wild=await enter();
  game.check('an open Realm admits the same living character after closure',wild.view.map.name==='Realm'&&wild.view.session.world!==-3&&wild.view.lifecycle.alive);
  const gods=[0x654,0x659,0x67d,0x651],lethal=(await map(wild.view.session.player)).entities.filter(e=>gods.includes(e.kind)).sort((a,b)=>distance(a.pos,wild.view.position)-distance(b.pos,wild.view.position));
  assert(lethal.length,'No natural god camp in the open Realm');
  await writeFile(join(out(),'death-target.json'),JSON.stringify({from:wild.view.position,target:lethal[0]}));
  let death=await walk({x:lethal[0].pos.x-7,y:lethal[0].pos.y},{tolerance:2,permitDeath:true,margin:120});
  for(let n=0;n<12&&death?.status!=='death_committed';n++){
    const s=await game.inspect(),near=s.diagnostic.entities.filter(e=>gods.includes(e.kind)).sort((a,b)=>distance(a.pos,s.view.position)-distance(b.pos,s.view.position))[0];
    const goal=near?.pos??lethal[0].pos;
    death=await game.die([{keys:keysFor(Math.sign(Math.round(goal.x-s.view.position.x)),Math.sign(Math.round(goal.y-s.view.position.y))),mouse:aim(s.view.position,goal),duration_ms:1500},{keys:[],mouse:aim(s.view.position,goal),duration_ms:6000}]);
  }
  await writeFile(join(out(),'death.json'),JSON.stringify(death,null,1));
  game.check('the journey character dies permanently and the death is durably committed',death?.status==='death_committed'&&death.death.death_committed===true&&death.death.character===initial.view.session.character);
  const browser=config.native.find(b=>b.actor===initial.view.session.character);
  const ui=await connect({...browser,url:origin+'/',helper:config.browserTools});
  const click=async(name,x,y,wait=2500)=>{
    const r=await ui.eval('(()=>{const r=document.querySelector("ruffle-player").getBoundingClientRect();return{x:r.x,y:r.y}})()'),p={x:r.x+x,y:r.y+y,button:'left',clickCount:1};
    await writeFile(join(out(),name+'.png'),Buffer.from((await ui.call('Page.captureScreenshot',{format:'png'})).data,'base64'));
    await ui.call('Input.dispatchMouseEvent',{type:'mouseMoved',...p,buttons:0});await ui.call('Input.dispatchMouseEvent',{type:'mousePressed',buttons:1,...p});await ui.call('Input.dispatchMouseEvent',{type:'mouseReleased',buttons:0,...p});
    await delay(wait);
  };
  await delay(2000);
  await click('death-fame-continue',400,546);await click('death-legends-done',400,546);await click('death-character-select-play',400,546);await click('death-new-wizard',399,141,6000);
  ui.close();
  const rebound=await game.rebind();
  const replacement=await log('replacement');
  game.check('UI-created replacement is a fresh level-one Wizard on the same account',rebound.replacement!==initial.view.session.character&&replacement.view.session.account===initial.view.session.account&&replacement.view.session.character===rebound.replacement&&replacement.view.character.class===WIZARD&&replacement.view.character.level===1&&replacement.view.lifecycle.alive);
  const graves=await (await fetch(origin+'/legacy-api/fame/list',{method:'POST',body:new URLSearchParams({timespan:'all',accountId:String(initial.view.session.account),charId:String(initial.view.session.character)})})).text();
  game.check('the dead character is recorded in Legends',graves.includes(`charId="${initial.view.session.character}"`));
  await game.act({keys:['d','s'],duration_ms:800},{name:'replacement-moves',when:()=>true});
  const moved=await log('replacement-moved');
  game.check('rebound replacement moves under native control',distance(moved.view.position,replacement.view.position)>.3&&moved.view.lifecycle.alive);
  const lines=await events();
  await writeFile(join(out(),'journey-summary.json'),JSON.stringify({prepared,attempts,stages:[...stages],reward,ring:PEER_RING,death:death.death,rebound,fingerprint:lines.find(e=>e.message==='XML game data compiled and linked before store startup')?.fingerprint},null,1));
}

export async function earnChamberAdmission(game,initial,allies){
  const {log,capture,upkeep,walk,enter}=await tools(game);
  const same=v=>v.lifecycle.alive&&v.session.account===initial.view.session.account&&v.session.character===initial.view.session.character;
  const population=async()=>map((await game.observe()).session.player);
  const castleMap=await population(),guards=castleMap.entities.filter(e=>[0xd78,0xd79].includes(e.kind));
  assert.equal(guards.length,2,'Missing Castle Guardian pair');
  assert(castleMap.entities.some(e=>e.kind===0xd7a),'No Guardian TaskMaster');
  await walk({x:guards[0].pos.x,y:guards[0].pos.y+2.8},{tolerance:.6});
  await capture('earned-guardians');
  async function fight(e,label,limit,north){
    for(let n=0;n<limit;n++){
      const clients=[game,...allies];
      const [s,...others]=await Promise.all(clients.map(client=>client.inspect())),p=s.view,boss=s.diagnostic.entities.find(x=>x.id===e.id);
      if(!boss){assert(!(await population()).entities.some(x=>x.id===e.id),'Boss left inspection range');return log(label);}
      const swordWait=(s.diagnostic.xml[boss.id]?.states??[]).some(row=>row.name==='tsPart4');
      // A Sword can land south of the default retreat line, leaving the Knight out of range while the Guardian is invulnerable.
      const sword=swordWait&&s.diagnostic.entities.find(x=>x.kind===0xd7c);
      const tower=swordWait&&s.diagnostic.entities.find(x=>x.kind===0xd87&&distance(x.pos,{x:108.5,y:21.5})<6);
      const priority=sword||tower||boss;
      const boundary=sword?Math.min(21,Math.max(18,sword.pos.y-1.8)):swordWait?18:north;
      const away=await routeForFight(p),views=[p,...others.map(s=>s.view)];
      const upkeepByStep=[],attacks=clients.map((client,i)=>{
        const v=views[i],steps=upkeep(v),knight=v.character.class===0x31e;
        const neighbors=views.filter(w=>w!==v&&distance(w.position,v.position)<5.7);
        const nearby=neighbors.some(w=>w.character.hp<maxHp(w.character)*.79);
        const debuffed=v.character.items[1]===0xc09&&[v,...neighbors].some(w=>w.character.conditions&(2|4|8|16|32|64|(1<<14)|(1<<26)));
        if(!knight&&!steps.length&&(nearby||debuffed)&&v.character.mp>=100&&v.time_ms>=v.character.ability_ready_ms)steps.push({keys:[' '],duration_ms:200});
        const shield=(1<<23)|(1<<24);
        // Armors on the west flank can charge across the Guardian retreat lane before the next healing cast.
        const armors=([s,...others][i].diagnostic.entities).filter(x=>x.kind===0xd7e&&x.hp>0&&distance(x.pos,v.position)<6&&!(x.condition&shield));
        const armor=armors.sort((a,b)=>distance(a.pos,v.position)-distance(b.pos,v.position))[0];
        const target=armor??priority,d=distance(target.pos,v.position);
        const canStun=knight&&v.character.mp>=100&&v.time_ms>=v.character.ability_ready_ms;
        const stunned=Boolean(target.condition&64),shielded=Boolean(target.condition&shield);
        const emergency=v.character.hp<maxHp(v.character)*.55;
        const retreat=v.character.hp<maxHp(v.character)*.76;
        const reach=emergency?15:retreat?9.4:knight?(canStun||stunned?3:4.1):8.6;
        const duration=420,speed=(v.character.conditions&(1<<13)?0:v.character.conditions&8?4:4+5.6*v.character.stats[4]/75)*duration/1000;
        const projectiles=v.projectiles.filter(b=>b.damage>0&&!views.some(w=>w.session.player===b.owner)&&distance(b.position,v.position)<8);
        const risk=(x,y)=>projectiles.reduce((sum,b)=>{
          const ux=x*speed-b.speed*duration/10000*Math.cos(b.angle),uy=y*speed-b.speed*duration/10000*Math.sin(b.angle);
          const px=v.position.x-b.position.x,py=v.position.y-b.position.y;
          const t=Math.max(0,Math.min(1,-(px*ux+py*uy)/Math.max(.0001,ux*ux+uy*uy)));
          return sum+(Math.hypot(px+t*ux,py+t*uy)<.83?b.damage:0);
        },0);
        const choices=[0,-1,1].flatMap(x=>[0,-1,1].map(y=>({x,y}))).map(({x,y})=>{
          const norm=Math.hypot(x,y)||1,point={x:v.position.x+speed*x/norm,y:v.position.y+speed*y/norm};
          const gap=distance(point,target.pos),spacing=Math.min(...views.filter(w=>w!==v).map(w=>distance(point,w.position)));
          const armorCost=armors.filter(a=>a!==target||!knight||retreat).reduce((cost,a)=>cost+Math.max(0,4-distance(point,a.pos))*4,0);
          const cost=risk(x/norm,y/norm)/20+armorCost+Math.max(0,Math.abs(gap-reach)-.65)+Math.hypot(x,y)*.2+Math.max(0,spacing-5.5)*1.5+Math.max(0,point.y-boundary)*2;
          return {x,y,point,cost};
        }).filter(q=>q.point.y<=Math.max(boundary,v.position.y)&&away(q.point));
        const q=choices.sort((l,r)=>l.cost-r.cost)[0];
        const cast=canStun&&d<3.25&&!stunned&&!shielded&&!retreat;
        if(cast)steps.push({keys:[' '],mouse:aim(v.position,target.pos),duration_ms:150});
        const keys=q?keysFor(q.x,q.y):[];
        // A wounded character cannot afford to stand still for the potion or tome's input window.
        if(emergency&&keys.length&&steps.length)steps[0]={...steps[0],keys:[...keys,...steps[0].keys]};
        steps.forEach((input,k)=>(upkeepByStep[k]??=[]).push({actor:client.actor,input}));
        const firing=!emergency&&(!knight||d<4.8);
        const from=q?{x:(v.position.x+q.point.x)/2,y:(v.position.y+q.point.y)/2}:v.position;
        return {actor:client.actor,input:{keys,...(firing?{mouse:aim(from,target.pos)}:{}),duration_ms:q&&(q.x||q.y)||risk(0,0)>0?duration:stunned?1450:900}};
      });
      if(n%8===0)for(const client of clients)await log(label+'-party',client);
      await game.act([...upkeepByStep,attacks]);
    }
    await log(label+'-bound');
    assert.fail('Native party did not defeat '+label);
  }
  let floor,world;
  async function routeForFight(v){
    if(world!==v.session.world){const m=await population(),blocked=new Set(m.blocked.map(([x,y])=>y*m.width+x));world=v.session.world;floor=p=>p.x>=1&&p.x<m.width-1&&p.y>=1&&p.y<m.height-1&&!blocked.has(Math.floor(p.y)*m.width+Math.floor(p.x));}
    return floor;
  }
  // Keep a finite reserve for a second Sword cycle without shortening the dodge or retreat windows.
  for(const [i,guard] of guards.entries())await fight(guard,'guardian-'+guard.id,i===0?165:235,10);
  let chamberGate;
  for(let n=0;n<8&&!chamberGate;n++){
    chamberGate=(await population()).entities.find(e=>e.kind===0xd7b&&e.published);
    if(!chamberGate)await game.act({keys:[],duration_ms:500});
  }
  const castleClear=await log('guardians-defeated');
  game.check('native Guardian defeats earn Chamber admission',chamberGate&&guards.every(g=>!castleClear.diagnostic.entities.some(e=>e.id===g.id))&&castleClear.view.character.experience>initial.view.character.experience);
  if(initial.view.character.level===19)game.check('earned kills reach level 20 without any level grant',castleClear.view.character.level===20);
  await walk(chamberGate.pos,{tolerance:.45});
  const admitted=[];
  for(const [i,ally] of allies.entries()){
    const enrolled=await ally.observe();
    if(i===0){
      assert.equal(enrolled.character.items[4],0xc09,'The enrolled Priest must carry the Tome of Purification');
      await ally.act({keys:[],duration_ms:350,drag:{from:{x:634,y:428},to:{x:678,y:384}}});
      const equipped=await ally.observe();
      game.check('native drag equips the Priest’s prepared Tome of Purification before Chamber entry',equipped.character.items[1]===0xc09&&equipped.character.items[4]===enrolled.character.items[1]&&equipped.character.inventory_revision===enrolled.character.inventory_revision+1);
    }
    await walk(chamberGate.pos,{tolerance:.5,client:ally});
    const arrived=await enter(ally);
    game.check('enrolled Priest keeps their account and character through the earned Chamber portal',arrived.view.map.name==="Oryx's Chamber"&&arrived.view.session.character===enrolled.session.character&&arrived.view.session.account===enrolled.session.account);
    admitted.push(arrived);
  }
  const chamber=await enter();
  if(admitted.length){
    const party=[chamber,...admitted].map(s=>s.view.session);
    game.check('the party enters one Chamber instance on distinct accounts',party.every(s=>s.world===chamber.view.session.world)&&new Set(party.map(s=>s.account)).size===party.length);
  }
  game.check('Guardian portal admits the same character to Oryx’s Chamber',chamber.view.map.name==="Oryx's Chamber"&&same(chamber.view));

}
