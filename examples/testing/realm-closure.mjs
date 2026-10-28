import assert from 'node:assert/strict';
import {appendFile,readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
import {DatabaseSync} from 'node:sqlite';

const out=process.env.QA_RESULTS_DIR,attackBonus=100;
const spec={class:0x307,level:20,seed:787,world:1,equipment:[0xc02,0xade,0xaf9,0xacd],backpack:Array(8).fill(-1),stats:[700,252,75,25,50,50,40,50]};
const roots=new Set([0x668,0x67d,0x91f,0x91b,0x928,0x675,0x678,0xd56,0xd59,0xd5f,0xd50,0xd54,0xd61,0xe37]);
const leaders=new Set([...roots,0x920,0x91c,0x92d,0x6c1,0x6c2]);
let scout,spawnPoints;
export async function prepare(api){
  const world=await api({op:'world_info',world:1}),boss=world.encounter_markers.find(e=>e.kind===0x67d);
  assert(boss,'An ordinarily populated Cyclops encounter is required');
  assert(world.spawn_points.length,'Ordinary Realm shore spawns are required');spawnPoints=world.spawn_points;
  scout=(await api({op:'spawn',spec:{...spec,guid:'closure-scout-'+randomUUID(),position:{x:Math.floor(boss.position.x)+.5,y:Math.floor(boss.position.y)+.5},controller:'direct',profile:null}})).result;
  await writeFile(join(out,'scout-preparation.json'),JSON.stringify(scout));
  return {...spec,position:boss.position};
}
export async function run(game){
  const config=JSON.parse(await readFile(join(out,'native/native.json'),'utf8')),token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const headers={Authorization:'Bearer '+token,'Content-Type':'application/json'},protection=new Map(),attack=new Map(),jumps=new Map(),records=[],stalled=new Map();
  let blocked,peer;
  async function api(command){
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers,body:JSON.stringify({id:randomUUID(),command}),signal:AbortSignal.timeout(10000)}),value=await response.json();
    const valid=command.op==='observe'?value.actor===command.actor:command.op==='inspect'?value.view?.session.character===command.actor:value.status==='succeeded';
    assert(response.ok&&valid,JSON.stringify(value));return value;
  }
  const cheat=(actor,change)=>api({op:'cheat',actor,change});
  async function events(){return (await readFile(join(out,'native/server.log'),'utf8')).split('\n').filter(l=>l.startsWith('{')).map(l=>JSON.parse(l).fields);}
  async function map(){
    const response=await fetch(config.origin+'/dev/map?player='+(await game.observe()).session.player,{headers,signal:AbortSignal.timeout(30000)});
    assert(response.ok,'Privileged map observation failed');const value=await response.json();
    if(blocked){blocked.fill(0);for(const [x,y] of value.blocked)blocked[y*value.width+x]=1;}
    return value;
  }
  async function save(label){const value={label,main:await game.observe(),detail:await game.inspect(),peer:await peer.observe(),scout:await api({op:'observe',actor:scout.actor})};records.push(value);await writeFile(join(out,'closure.json'),JSON.stringify(records));return value;}
  const initial=await game.observe(),origin=initial.session.world,opening=await map(),width=opening.width;
  blocked=new Uint8Array(width*opening.height);for(const [x,y] of opening.blocked)blocked[y*width+x]=1;
  const original=opening.entities.filter(e=>roots.has(e.kind)&&(e.quest===e.id||[0x668,0x67d].includes(e.kind)));
  assert(original.length>20,'Full ordinary roster required, not a prepared final encounter');
  const distance=p=>Math.min(...original.map(e=>Math.hypot(p.x-e.pos.x,p.y-e.pos.y)));
  const restPoint=[...spawnPoints].sort((a,b)=>distance(b)-distance(a))[0];
  assert(distance(restPoint)>30,'Protection renewal requires a shore spawn away from counted encounters');
  const scoutStart=await api({op:'observe',actor:scout.actor}),peerPoints=[];
  for(let y=-15;y<=15;y++)for(let x=-15;x<=15;x++){
    const p={x:Math.floor(initial.position.x)+x+.5,y:Math.floor(initial.position.y)+y+.5},d=Math.hypot(p.x-initial.position.x,p.y-initial.position.y);
    if(d>6&&d<20&&!blocked[Math.floor(p.y)*width+Math.floor(p.x)]&&Math.hypot(p.x-scoutStart.position.x,p.y-scoutStart.position.y)+.2<d)peerPoints.push(p);
  }
  assert(peerPoints.length,'No legal initial partner position with the scout nearest');
  peer=await game.join({...spec,seed:788,world:origin,position:peerPoints[0]});const peerInitial=await peer.observe();
  const clients=[game,peer];
  async function ready(){
    const now=(await game.observe()).game_ms;
    for(const actor of [game.actor,peer.actor,scout.actor])if(now>=(protection.get(actor)??0)){
      await cheat(actor,{kind:'protection',condition:'invincible',duration_ms:60000});protection.set(actor,now+60000);
    }
    for(const client of clients)if(now>=(attack.get(client.actor)??0)){
      await cheat(client.actor,{kind:'stat_modifier',index:2,amount:attackBonus,duration_ms:60000});attack.set(client.actor,now+60000);
    }
    return Math.min(...protection.values())-now;
  }
  async function act(client,input,capture){
    if(Array.isArray(input)){
      const duration=input.flat().reduce((sum,row)=>sum+row.input.duration_ms,0),budget=await ready();
      if(duration>budget){await act(game,{keys:[],duration_ms:budget});return act(client,input,capture);}
      return client.act(input,capture);
    }
    let remaining=input.duration_ms,result;
    while(remaining>0){const duration_ms=Math.min(remaining,await ready());assert(duration_ms>0,'Protection deadline must advance');result=await client.act({...input,duration_ms},capture);remaining-=duration_ms;capture=undefined;}
    return result;
  }
  function locked(account){
    const db=new DatabaseSync(join(config.state,'game.db'),{readOnly:true});
    try{return db.prepare('SELECT count(*) AS n FROM account_lists WHERE account=? AND list_id=0 AND target=?').get(account,scout.account).n===1;}finally{db.close();}
  }
  const menu=(client,y)=>[{actor:client.actor,input:{keys:[],mouse:{x:650,y:520},duration_ms:250}},{actor:client.actor,input:{keys:[],mouse:{x:550,y},duration_ms:350}}];
  const menus=y=>[0,1].map(i=>clients.map(client=>menu(client,y)[i]));
  await act(game,{keys:[],duration_ms:800});await act(game,menus(432));
  for(const client of clients)game.check('native party lock persists for scout and account '+(await client.observe()).session.account,locked((await client.observe()).session.account));
  await writeFile(join(out,'opening.json'),JSON.stringify({original,initial,peerInitial,scout,assistance:{attack_bonus:attackBonus,protection_ms:60000,resting_spawn:restPoint}}));
  opening.blocked.length=0;opening.entities.length=0;
  await act(game,{keys:[],duration_ms:500},{name:'realm-opening',when:()=>true});await save('opening');
  async function clearPartyPanel(){
    for(let step=0;step<4;step++){
      const states=await Promise.all(clients.map(c=>c.inspect()));
      const objects=states.flatMap(s=>{
        const portals=new Set(s.diagnostic.entities.filter(e=>e.extra_stats.some(([stat])=>stat===37)).map(e=>e.id));
        return s.view.objects.filter(e=>e.contents.length>0||portals.has(e.id));
      });
      if(states.every(s=>objects.every(e=>Math.hypot(s.view.position.x-e.position.x,s.view.position.y-e.position.y)>1.4)))return;
      const position=states[0].view.position,points=[];
      for(const dy of [-3,0,3])for(const dx of [-3,0,3]){
        const p={x:Math.floor(position.x)+dx+.5,y:Math.floor(position.y)+dy+.5};
        const clear=states.every(s=>Array.from({length:16},(_,i)=>(i+1)/16).every(t=>!blocked[Math.floor(s.view.position.y+(p.y-s.view.position.y)*t)*width+Math.floor(s.view.position.x+(p.x-s.view.position.x)*t)]));
        if((dx||dy)&&clear&&p.x>=0&&p.y>=0&&p.x<width&&p.y<opening.height&&!blocked[Math.floor(p.y)*width+Math.floor(p.x)]&&objects.every(e=>Math.hypot(p.x-e.position.x,p.y-e.position.y)>3))points.push(p);
      }
      assert(points.length,'No walkable party-menu clearance away from nearby containers/portals');
      await appendFile(join(out,'party-clearance.jsonl'),JSON.stringify({positions:states.map(s=>s.view.position),objects:objects.map(e=>({id:e.id,position:e.position})),point:points[0]})+'\n');
      await approach(points[0],1);
    }
    assert.fail('Native movement did not clear the interaction panel for party-menu travel');
  }
  async function travel(anchor){
    await clearPartyPanel();const points=[];
    for(let y=-5;y<=5;y++)for(let x=-5;x<=5;x++){
      const p={x:Math.floor(anchor.x)+x+.5,y:Math.floor(anchor.y)+y+.5},d=Math.hypot(p.x-anchor.x,p.y-anchor.y);
      if(d>=1.5&&d<5&&!blocked[Math.floor(p.y)*width+Math.floor(p.x)])points.push(p);
    }
    points.sort((a,b)=>Math.abs(Math.hypot(a.x-anchor.x,a.y-anchor.y)-2.5)-Math.abs(Math.hypot(b.x-anchor.x,b.y-anchor.y)-2.5));
    assert(points.length,'No legal prepared scout position');
    await ready();await cheat(scout.actor,{kind:'teleport',position:points[0]});
    for(const client of clients){
      let now=(await client.observe()).game_ms;
      while(jumps.has(client.actor)&&now-jumps.get(client.actor)<11000){await act(game,{keys:[],duration_ms:Math.min(10000,11000-now+jumps.get(client.actor))});now=(await client.observe()).game_ms;}
    }
    await act(game,menus(400));
    for(const client of clients){
      const after=await client.observe();
      assert(after.session.world===origin&&after.lifecycle.alive&&Math.hypot(after.position.x-points[0].x,after.position.y-points[0].y)<.2,'Native menu did not complete the actual scout teleport: '+JSON.stringify({actor:client.actor,expected:points[0],position:after.position,world:after.session.world,origin}));
      jumps.set(client.actor,after.game_ms);await appendFile(join(out,'travel.jsonl'),JSON.stringify({actor:client.actor,anchor,position:after.position,game_ms:after.game_ms})+'\n');
    }
  }
  async function approach(point,range){
    const group=[],before=await Promise.all(clients.map(c=>c.observe()));
    for(let i=0;i<clients.length;i++){
      const view=before[i],dx=point.x-view.position.x,dy=point.y-view.position.y,d=Math.hypot(dx,dy),bend=(stalled.get(clients[i].actor)??0)>0;
      const horizontal=Math.abs(dx)>=Math.abs(dy)*.2||bend&&Math.abs(dx)>.001,vertical=Math.abs(dy)>=Math.abs(dx)*.2||bend&&Math.abs(dy)>.001;
      const keys=d<=range?[]:horizontal&&vertical?[dy>0?(dx>0?'s':'a'):(dx>0?'d':'w')]:horizontal?(dx>0?['d','s']:['a','w']):(dy>0?['a','s']:['d','w']);
      group.push({actor:clients[i].actor,input:{keys,duration_ms:d<=range?100:Math.max(100,Math.min(1200,Math.round((d-range+1)*800/(4+5.6*view.character.stats.speed/75))))}});
    }
    await act(game,[group]);
    const after=await Promise.all(clients.map(c=>c.observe()));
    for(let i=0;i<clients.length;i++)if(group[i].input.keys.length){const count=Math.hypot(after[i].position.x-before[i].position.x,after[i].position.y-before[i].position.y)>.02?0:(stalled.get(clients[i].actor)??0)+1;stalled.set(clients[i].actor,count);assert(count<3,'Three stationary local native approach inputs: '+JSON.stringify({point,before:before[i].position,after:after[i].position,group}));}
    await appendFile(join(out,'approach.jsonl'),JSON.stringify({point,group,before:before.map(v=>v.position),after:after.map(v=>v.position)})+'\n');
  }
  async function recharge(returnToCombat){
    const before=await api({op:'observe',actor:scout.actor}),until=Math.min(...protection.values());
    await travel(restPoint);
    let now=(await game.observe()).game_ms;
    while(now<until){await act(game,{keys:[],duration_ms:Math.min(10000,until-now)});now=(await game.observe()).game_ms;}
    await ready();
    await appendFile(join(out,'renewals.jsonl'),JSON.stringify({before_ms:before.game_ms,renewed_ms:now,restPoint,returnToCombat})+'\n');
    if(returnToCombat)await travel(before.position);
  }
  const seen=new Map(original.map(e=>[e.quest??e.id,e]));
  for(let encounter=0;encounter<100;encounter++){
    const log=await events(),credits=log.filter(e=>e.message==='qualifying Realm hero defeated'&&e.origin===origin),defeated=new Set(credits.map(e=>e.hero)),current=await map();
    for(const e of current.entities)if(roots.has(e.kind)&&e.quest===e.id)seen.set(e.id,e);
    current.blocked.length=0;current.entities.length=0;
    const position=(await game.observe()).position,next=[...seen.keys()].filter(id=>!defeated.has(id)).sort((a,b)=>Math.hypot(seen.get(a).pos.x-position.x,seen.get(a).pos.y-position.y)-Math.hypot(seen.get(b).pos.x-position.x,seen.get(b).pos.y-position.y))[0];
    if(next===undefined){assert(credits.at(-1)?.remaining===0,'Every counted root must earn actual death credit');break;}
    if(await ready()<30000)await recharge(false);
    await travel(seen.get(next).pos);
    if(seen.get(next).kind===0xd5f&&await ready()<50000)await recharge(true);
    let completed=false;
    for(let step=0;step<100;step++){
      if((await events()).some(e=>e.message==='qualifying Realm hero defeated'&&e.origin===origin&&e.hero===next)){completed=true;break;}
      if(await ready()<30000){await recharge(true);continue;}
      const state=await api({op:'inspect',actor:scout.actor}),family=state.diagnostic.entities.filter(e=>e.id===next||e.quest===next),vulnerable=family.filter(e=>e.hp>0&&!(e.condition&((1<<21)|(1<<23)|(1<<24))));
      const views=await Promise.all(clients.map(c=>c.observe())),position=views[0].position;
      vulnerable.sort((a,b)=>Math.hypot(a.pos.x-position.x,a.pos.y-position.y)-Math.hypot(b.pos.x-position.x,b.pos.y-position.y));
      const target=vulnerable.find(e=>e.kind===0x91d)??vulnerable.find(e=>leaders.has(e.kind))??vulnerable.find(e=>[0x929,0xd5e,0xd62].includes(e.kind));
      const anchor=target??family.find(e=>leaders.has(e.kind));
      if(!anchor){await act(game,{keys:[],duration_ms:1000});continue;}
      const range=target?.kind===0xd5e?6.5:5.5;
      if(views.some(v=>Math.hypot(v.position.x-anchor.pos.x,v.position.y-anchor.pos.y)>range)){await approach(anchor.pos,range);continue;}
      if(target){
        const duration_ms=target.kind===0xd5e?1000:4000,states=await Promise.all(clients.map(c=>c.inspect()));
        const group=states.map((s,i)=>{
          const known=s.view.objects.find(e=>e.id===target.id&&e.hp>0);assert(known,'A living target must be known to each actual native client');
          const dx=known.position.x-s.view.position.x,dy=known.position.y-s.view.position.y,scale=Math.min(50,200/Math.hypot(dx,dy));
          return {actor:clients[i].actor,input:{keys:[],duration_ms,mouse:{x:300+(dx-dy)*scale/Math.SQRT2,y:325+(dx+dy)*scale/Math.SQRT2}}};
        });
        await act(game,[group]);
      }else await act(game,{keys:[],duration_ms:anchor.stage===1&&[0x91f,0x91b,0x928].includes(anchor.kind)?1000:10000});
    }
    assert(completed||(await events()).some(e=>e.message==='qualifying Realm hero defeated'&&e.origin===origin&&e.hero===next),'Native combat did not finish counted encounter '+next);
    await writeFile(join(out,'root-'+next+'.json'),JSON.stringify({root:next,kind:seen.get(next).kind,main:await game.observe(),peer:await peer.observe()}));
  }
  const credits=(await events()).filter(e=>e.message==='qualifying Realm hero defeated'&&e.origin===origin);
  game.check('ordinary roster plus admitted events earns once-only closure credit',credits.length===seen.size&&new Set(credits.map(e=>e.hero)).size===credits.length&&credits.at(-1)?.remaining===0);
  await travel(restPoint);
  const closing=await save('closed');game.check('both native fighters remain in the closed Realm',closing.main.session.world===origin&&closing.peer.session.world===origin);
  await act(game,{keys:[],duration_ms:700},{name:'realm-closed',when:()=>true});
  // Reconnect retains charId rather than creating a successor: https://github.com/moistosaurus/realm-cli/blob/a13c855dc5e09ee301fd0a66fcd303cfb5dff74e/src/kabam/rotmg/messaging/impl/GameServerConnection.as#L1635-L1690
  for(let n=0;n<30&&(await game.observe()).session.world===origin;n++)await act(game,{keys:[],duration_ms:5000});
  const arrived=await save('castle');
  for(const [label,before,after,settled] of [['first',initial,arrived.main,closing.main],['second',peerInitial,arrived.peer,closing.peer]]){
    game.check(label+' follows the forced route into Castle with the same living character',after.world==="Oryx's Castle"&&after.lifecycle.alive&&after.session.account===before.session.account&&after.session.character===before.session.character&&after.session.world!==origin);
    game.check(label+' retains exact inventory and revision',JSON.stringify(after.character.items)===JSON.stringify(before.character.items)&&after.character.inventory_revision===before.character.inventory_revision);
    game.check(label+' retains earned XP',after.character.experience>=settled.character.experience&&after.verification.shots>0);
  }
  game.check('collective recipients enter the same Castle instance',arrived.main.session.world===arrived.peer.session.world);
  await act(game,{keys:[],duration_ms:3000},{name:'castle-arrival',when:v=>v.world==="Oryx's Castle"&&v.lifecycle.alive});
  const finalLog=await events(),route=finalLog.find(e=>e.message==='Realm closed; collective Castle route and fresh Realm installed'&&e.origin===origin),departure=finalLog.findIndex(e=>e.message==='all collective recipients durably departed; replacement delay started'&&e.origin===origin),published=finalLog.findIndex(e=>e.message==='safe collective departure completed; replacement published'&&e.origin===origin);
  game.check('normal replacement follows both durable departures',route?.recipients===2&&departure>=0&&published>departure&&finalLog[published].replacement===route.replacement);
  await writeFile(join(out,'lifecycle.json'),JSON.stringify({original:[...seen.values()],credits,route,events:finalLog.filter(e=>e.origin===origin)}));
}
