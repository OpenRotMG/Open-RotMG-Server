import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
import {connect} from '../../scripts/record-demo.mjs';
const out=process.env.QA_RESULTS_DIR;
const spec={class:0x30e,level:20,seed:821,world:1,equipment:[0xc03,0xa2e,0xb05,0xac0],backpack:Array(8).fill(-1),stats:[670,385,75,25,50,75,40,60]};
let admissionActor,safePosition,rootEntity,admitted,startedWithoutPentaract,api,cheat,map,clients=0;
const save=(name,value)=>writeFile(join(out,name+'.json'),JSON.stringify(value,null,2));
export async function prepare(api){
  const world=await api({op:'world_info',world:1}),[x,y]=world.spawn_points[0];safePosition={x,y};
  admissionActor=(await api({op:'spawn',spec:{...spec,class:0x31d,guid:'pentaract-admission',position:safePosition,controller:'direct',profile:null,equipment:[0xb0b,0xb29,0xa7b,-1],stats:[770,252,75,25,50,50,75,0]}})).result.actor;
  return {...spec,world:-2,position:'spawn',equipment:[-1,-1,-1,-1]};
}
const packets=[];
export async function beforeNavigate({browser,signal}){
  if(clients++===0)await admit();
  const streams=new Map();
  const connection=await connect({...browser,url:'about:blank',helper:join(process.env.PI_BROWSER_TOOLS,'direct-cdp.mjs'),onEvent:event=>{
    if(event.method!=='Network.webSocketFrameReceived'||event.params.response.opcode!==2)return;
    const {requestId,response}=event.params;
    if(!streams.has(requestId)){
      const key=Buffer.from('72c5583cafb6818995cbd74b80','hex'),s=Array.from({length:256},(_,i)=>i);let j=0;
      for(let i=0;i<256;i++){j=(j+s[i]+key[i%key.length])&255;[s[i],s[j]]=[s[j],s[i]];}
      streams.set(requestId,{s,i:0,j:0,buffer:Buffer.alloc(0)});
    }
    const state=streams.get(requestId);state.buffer=Buffer.concat([state.buffer,Buffer.from(response.payloadData,'base64')]);
    while(state.buffer.length>=5){
      const n=state.buffer.readInt32BE();assert(n>=5&&n<=1048576);if(state.buffer.length<n)break;
      const id=state.buffer[4],body=Buffer.from(state.buffer.subarray(5,n));state.buffer=state.buffer.subarray(n);
      for(let k=0;k<body.length;k++){state.i=(state.i+1)&255;state.j=(state.j+state.s[state.i])&255;[state.s[state.i],state.s[state.j]]=[state.s[state.j],state.s[state.i]];body[k]^=state.s[(state.s[state.i]+state.s[state.j])&255];}
      if(id===63)packets.push({id,player:body.readInt32BE(),text:body.subarray(6,6+body.readUInt16BE(4)).toString()});
      if(id===56)packets.push({id,effect:body[0],owner:body.readInt32BE(1),a:{x:body.readFloatBE(5),y:body.readFloatBE(9)},b:{x:body.readFloatBE(13),y:body.readFloatBE(17)},color:body.readUInt32BE(21)});
    }
  }});
  await connection.call('Network.enable');signal.addEventListener('abort',()=>connection.close(),{once:true});
}
async function admit(){
  const root=join(out,'native'),report=JSON.parse(await readFile(join(root,'result.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(root,'state/control.token'),'utf8')).trim(),'Content-Type':'application/json'};
  api=async command=>{const r=await fetch(report.origin+'/dev/testing',{method:'POST',headers,body:JSON.stringify({id:randomUUID(),command})}),v=await r.json();assert(r.ok&&v.status!=='failed',JSON.stringify(v));return v;};
  cheat=(actor,change)=>api({op:'cheat',actor,change});
  map=async(actor=admissionActor)=>{const s=await api({op:'observe',actor}),r=await fetch(report.origin+'/dev/map?player='+s.session.player,{headers});assert(r.ok);return r.json();};
  const direct=(angle,duration_ms=3000)=>api({op:'act',actor:admissionActor,action:{command:'shoot',angle},duration_ms,pauseAfter:true});
  let m=await map(),until=0;
  startedWithoutPentaract=!m.entities.some(e=>e.kind===0xd5f);assert(startedWithoutPentaract);
  const blocked=new Set(m.blocked.map(p=>p.join(','))),admission=[];
  for(let n=0;n<100;n++){
    rootEntity=m.entities.find(e=>e.kind===0xd5f);if(rootEntity)break;
    let now=(await api({op:'observe',actor:admissionActor})).game_ms;
    if(now+3500>=until){
      await cheat(admissionActor,{kind:'teleport',position:safePosition});
      if(now<until)await direct(0,until-now+50);
      now=(await api({op:'observe',actor:admissionActor})).game_ms;
      await cheat(admissionActor,{kind:'protection',condition:'invulnerable',duration_ms:60000});
      await cheat(admissionActor,{kind:'stat_modifier',index:2,amount:100,duration_ms:60000});
      until=now+60000;
    }
    const e=[0x668,0x67d,0x678,0xd56,0xd59].map(kind=>m.entities.find(e=>e.kind===kind)).find(Boolean);assert(e,'Bounded simple-hero sample did not admit a Pentaract; this take cannot exercise the scenario');
    const position=[[1,0],[-1,0],[0,1],[0,-1]].map(([x,y])=>({x:e.pos.x+x,y:e.pos.y+y})).find(p=>!blocked.has([Math.floor(p.x),Math.floor(p.y)].join(',')));assert(position);
    await cheat(admissionActor,{kind:'teleport',position});
    await direct(Math.atan2(e.pos.y-position.y,e.pos.x-position.x),1000);m=await map();
    admission.push({target:e.id,kind:e.kind,survived:m.entities.some(t=>t.id===e.id),events:m.entities.filter(t=>[0xd56,0xd59,0xd5f].includes(t.kind))});await save('admission',admission);
  }
  rootEntity=m.entities.find(e=>e.kind===0xd5f);
  assert(rootEntity,'Bounded ordinary direct hero defeats did not admit Pentaract');await save('admitted',m);
  await cheat(admissionActor,{kind:'teleport',position:safePosition});
  const expire=async()=>{while((await api({op:'observe',actor:admissionActor})).game_ms<until)await direct(0,Math.min(7000,until-(await api({op:'observe',actor:admissionActor})).game_ms+50));};
  await expire();await cheat(admissionActor,{kind:'item',slot:0,item:0xa00});
  const stats=(await api({op:'observe',actor:admissionActor})).character.stats;
  assert(stats.dexterity===50&&Math.ceil(90*(.5+1.5*stats.attack/75))<200,'Short Sword preparation must leave a safe positive-health margin');
  const assistance=[];
  for(const tower of m.entities.filter(e=>e.quest===rootEntity.id&&e.kind===0xd5e).slice(1)){
    const now=(await api({op:'observe',actor:admissionActor})).game_ms;
    if(now+40000>=until){await cheat(admissionActor,{kind:'teleport',position:safePosition});await expire();await cheat(admissionActor,{kind:'protection',condition:'invulnerable',duration_ms:60000});until=(await api({op:'observe',actor:admissionActor})).game_ms+60000;}
    await cheat(admissionActor,{kind:'teleport',position:{x:tower.pos.x-.1,y:tower.pos.y}});
    let e;
    for(let n=0;n<80;n++){e=(await api({op:'inspect',actor:admissionActor})).diagnostic.entities.find(e=>e.id===tower.id);assert(e&&e.hp>0,'Ordinary assistance must not defeat a tower');if(e.hp<=200)break;await direct(0,100);await api({op:'act',actor:admissionActor,action:{command:'stop'},duration_ms:400,pauseAfter:true});}
    assert(e.hp<=200,'Bounded ordinary assistance did not reach its declared health range');assistance.push({id:tower.id,hp:e.hp,damage:e.damage});
  }
  await save('assistance',{method:'Before native LOAD, the direct admission participant ordinarily weakens four towers to 1–200 HP with Short Sword shots. The first tower stays at 5000 HP; native input delivers all five final defeats. Revival is qualified separately by take 7e093a7f88954a20bbde45cfb4dfe4c1.',towers:assistance});
  await cheat(admissionActor,{kind:'teleport',position:safePosition});admitted=m;
}
export async function run(lobby){
  assert(admitted,'Before-navigation admission must finish before native gameplay');
  lobby.check('Realm starts without an admitted Pentaract',startedWithoutPentaract);
  let m=admitted;
  const members=(m,kind)=>m.entities.filter(e=>e.quest===rootEntity.id&&e.kind===kind),towers=members(m,0xd5e);
  lobby.check('ordinary admission retains five owned towers and an invisible controller',towers.length===5&&rootEntity.size===0);
  await save('accepted-prerequisites',{checks:[{take:'fce508e0d2f4451ea3664e8fee571f63',source:'5666cecd29f1479066494a80b9f8736e506c350c',scope:'Eleven partial checks retain activation, Eyes, projectiles, unprotected damage, red/yellow signals and contribution transfer; the take failed its pre-revival sample.'},{take:'7e093a7f88954a20bbde45cfb4dfe4c1',source:'a8e700334f44c5ac0a4c2d78ccd7e51a876a71c9',scope:'Pre-15-second corpse sample and full-HP/original-position revival passed; the take failed all-five completion.'}],scope:'Fresh prepared cooperative completion, rewards and reconnect only.'});
  const game=await lobby.join({...spec,position:{x:towers[0].pos.x-3,y:towers[0].pos.y},equipment:[0xc03,0xa2e,0xb05,0xac9]});
  await cheat(game.actor,{kind:'protection',condition:'invulnerable',duration_ms:60000});
  await cheat(game.actor,{kind:'stat_modifier',index:4,amount:100,duration_ms:60000});
  const observer=await lobby.join({...spec,position:rootEntity.pos,equipment:[-1,-1,0xb05,0xac0]});
  await cheat(observer.actor,{kind:'protection',condition:'invulnerable',duration_ms:60000});
  const observerStart=await observer.inspect();await save('observer-before',observerStart);
  async function approach(point){
    for(let n=0;n<150;n++){
      const v=await game.observe(),dx=point.x-v.position.x,dy=point.y-v.position.y;if(Math.hypot(dx,dy)<.35)return;
      const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,keys=[];
      if(Math.abs(x)>=Math.abs(y)*.4142)keys.push(x>0?'d':'a');if(Math.abs(y)>=Math.abs(x)*.4142)keys.push(y>0?'s':'w');
      await game.act({keys,duration_ms:Math.max(50,Math.min(500,Math.round(Math.hypot(dx,dy)/(4+5.6*v.character.stats.speed/75)*1000)))});
    }
    throw Error('Native walking did not reach the observed tower');
  }
  await approach(towers[0].pos);m=await map(game.actor);
  assert(members(m,0xd5e).length===5&&members(m,0xd60).length===0&&m.entities.find(e=>e.id===towers[0].id).hp===5000&&towers.slice(1).every(t=>{const e=m.entities.find(e=>e.id===t.id);return e.hp>0&&e.hp<=200;}),'Declared ordinary assistance must retain all five living towers');
  const packetStart=packets.length,start=await game.inspect(),v=await game.observe();
  assert.equal(v.character.stats.speed,158,'Prepared speed window expired before the bounded tower race');
  const solid=new Set(m.blocked.filter(([x,y])=>Math.abs(x-rootEntity.pos.x)<25&&Math.abs(y-rootEntity.pos.y)<25).map(p=>p.join(',')));
  const alive=members(m,0xd5e),tour=towers.map(t=>alive.find(e=>e.pos.x===t.pos.x&&e.pos.y===t.pos.y));assert(tour.every(Boolean));
  const sequence=[];let at=v.position;const speed=4+5.6*v.character.stats.speed/75;
  function route(from,to){
    const dx=to.x-from.x,dy=to.y-from.y,x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,d=Math.min(Math.abs(x),Math.abs(y)),sx=Math.sign(x)*d,sy=Math.sign(y)*d;
    const diagonal={keys:[x>0?'d':'a',y>0?'s':'w'],x:(sx+sy)/Math.SQRT2,y:(sy-sx)/Math.SQRT2};
    const axis={keys:[Math.abs(x)>Math.abs(y)?(x>0?'d':'a'):(y>0?'s':'w')],x:dx-diagonal.x,y:dy-diagonal.y};
    const parts=[diagonal,axis].filter(s=>Math.hypot(s.x,s.y)>.01);
    const clear=parts=>{let p=from;for(const s of parts){const steps=Math.ceil(Math.hypot(s.x,s.y)*8);for(let n=0;n<=steps;n++)for(const ox of [-.4,.4])for(const oy of [-.4,.4])if(solid.has([Math.floor(p.x+s.x*n/steps+ox),Math.floor(p.y+s.y*n/steps+oy)].join(',')))return false;p={x:p.x+s.x,y:p.y+s.y};}return true;};
    const chosen=[parts,[...parts].reverse()].find(clear);assert(chosen,'No clear two-direction native circuit leg: '+JSON.stringify({from,to}));
    return chosen.map(s=>({actor:game.actor,input:{keys:s.keys,duration_ms:Math.round(Math.hypot(s.x,s.y)/speed*1000)}}));
  }
  // Per-action synchronization consumes encounter time, so the race uses one existing bounded transaction.
  for(const tower of tour){sequence.push(...route(at,tower.pos));if(tower!==tour[0])sequence.push({actor:game.actor,input:{keys:[],duration_ms:50}});sequence.push({actor:game.actor,input:{keys:[' '],duration_ms:1000,target:{entity:tower.id,camera_angle:-Math.PI/4,fire:true}}});at=tower.pos;}
  await save('race-plan',{tour,sequence,input_ms:sequence.reduce((sum,s)=>sum+s.input.duration_ms,0)});
  assert(sequence.reduce((sum,s)=>sum+s.input.duration_ms,0)<=10000,'Native tower circuit exceeds the unchanged ten-second input budget');
  await game.act(sequence);
  await game.act({keys:[],duration_ms:500},{name:'pentaract-defeated',when:()=>true});m=await map(game.actor);await save('defeated',m);
  game.check('all-five completion removes controller, towers, corpses and Eyes',!m.entities.some(e=>e.quest===rootEntity.id));
  game.check('no Ultra central tower appears',!m.entities.some(e=>e.kind===0xe01));
  const end=await game.inspect(),observerEnd=await observer.inspect();await save('completion',{start,end,observerStart,observerEnd,packets:packets.slice(packetStart)});
  game.check('native completion awards encounter XP',end.view.character.experience>start.view.character.experience);
  game.check('completion notification occurs once for the fighter',packets.slice(packetStart).filter(p=>p.id===63&&p.text==='Quest Complete!'&&p.player===end.diagnostic.player).length===1);
  game.check('contributor receives one qualified quest',end.diagnostic.character.lifetime.qualified_quests===start.diagnostic.character.lifetime.qualified_quests+1);
  game.check('noncontributor receives no qualified quest',observerEnd.diagnostic.character.lifetime.qualified_quests===observerStart.diagnostic.character.lifetime.qualified_quests);
  const bags=m.entities.filter(e=>e.items.some(i=>i!==-1)&&towers.some(t=>Math.hypot(e.pos.x-t.pos.x,e.pos.y-t.pos.y)<.1));await save('tower-rewards',bags);
  const visible=(await map(observer.actor)).entities;
  assert(bags.length,'No tower reward rolled; this take cannot verify pickup');
  game.check('private tower rewards exclude the noncontributor',bags.every(b=>(b.bag_owners.includes(end.view.session.account)||b.extra_stats.some(([stat,account])=>stat===54&&account===end.view.session.account))&&!visible.some(e=>e.id===b.id)));
  let pickedReward;
  {
    const bag=bags[0],item=bag.items[0];assert(item!==-1);await approach(bag.pos);await game.act({keys:[],duration_ms:250});const inventory=await game.observe();
    await game.act({keys:[],duration_ms:350,drag:{from:{x:634,y:528},to:{x:634,y:428}}});const picked=await game.inspect();pickedReward=picked;await save('picked-reward',picked);
    game.check('native pickup commits one tower reward',picked.view.character.items[4]===item&&picked.view.character.inventory_revision===inventory.character.inventory_revision+1);
    await game.act({keys:[],duration_ms:350,drag:{from:{x:634,y:528},to:{x:678,y:428}}});game.check('empty-slot drag cannot duplicate loot',(await game.observe()).character.inventory_revision===picked.view.character.inventory_revision);
  }
  const xp=(await game.observe()).character.experience,ended=packets.length;
  await game.act({keys:[],duration_ms:8000});await game.act({keys:[],duration_ms:8000});m=await map(game.actor);await save('finished',await game.inspect());
  game.check('completed encounter stays removed beyond revival deadline',!m.entities.some(e=>e.quest===rootEntity.id));
  game.check('completion cannot repeat XP or qualified credit',(await game.observe()).character.experience===xp&&(await game.inspect()).diagnostic.character.lifetime.qualified_quests===end.diagnostic.character.lifetime.qualified_quests);
  game.check('no connections are sent after completion',!packets.slice(ended).some(p=>p.effect===3));
  game.check('prepared native character survives',(await game.observe()).lifecycle.alive);
  await game.act({keys:['Insert'],duration_ms:250});const retained=await game.observe();await save('reconnected',retained);
  game.check('ordinary Nexus reconnect retains the committed reward',retained.world==='Nexus'&&retained.session.character===pickedReward.view.session.character&&retained.character.items[4]===pickedReward.view.character.items[4]&&retained.character.inventory_revision===pickedReward.view.character.inventory_revision);
}
