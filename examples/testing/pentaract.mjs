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
    const e=m.entities.find(e=>[0x668,0x67d,0x678,0xd56,0xd59].includes(e.kind));assert(e,'No bounded admission target remains');
    const position=[[1,0],[-1,0],[0,1],[0,-1]].map(([x,y])=>({x:e.pos.x+x,y:e.pos.y+y})).find(p=>!blocked.has([Math.floor(p.x),Math.floor(p.y)].join(',')));assert(position);
    await cheat(admissionActor,{kind:'teleport',position});
    await direct(Math.atan2(e.pos.y-position.y,e.pos.x-position.x));m=await map();
    admission.push({target:e.id,kind:e.kind,survived:m.entities.some(t=>t.id===e.id),events:m.entities.filter(t=>[0xd56,0xd59,0xd5f].includes(t.kind))});await save('admission',admission);
  }
  assert(rootEntity,'Bounded ordinary direct hero defeats did not admit Pentaract');await save('admitted',m);
  await cheat(admissionActor,{kind:'teleport',position:safePosition});admitted=m;
}
export async function run(lobby){
  assert(admitted,'Before-navigation admission must finish before native gameplay');
  lobby.check('Realm starts without an admitted Pentaract',startedWithoutPentaract);
  let m=admitted;
  const members=(m,kind)=>m.entities.filter(e=>e.quest===rootEntity.id&&e.kind===kind),towers=members(m,0xd5e);
  lobby.check('ordinary admission retains five owned towers and an invisible controller',towers.length===5&&rootEntity.size===0);
  const game=await lobby.join({...spec,position:{x:towers[0].pos.x-3,y:towers[0].pos.y}});
  const observer=await lobby.join({...spec,position:rootEntity.pos,equipment:[-1,-1,0xb05,0xac0]});
  await cheat(observer.actor,{kind:'protection',condition:'invulnerable',duration_ms:60000});
  const observerStart=await observer.inspect();await save('observer-before',observerStart);
  const before=await game.inspect(),samples=[];let done=false;
  const action=game.act({keys:[],duration_ms:7000},{name:'pentaract-standing',when:v=>v.game_ms>=before.view.time_ms+3000}).finally(()=>done=true);
  const sampling=(async()=>{while(!done){samples.push(await game.inspect());await new Promise(ok=>setTimeout(ok,60));}})();
  for(const r of await Promise.allSettled([action,sampling]))if(r.status==='rejected')throw r.reason;
  await save('attacks',samples);await save('standing-packets',packets);m=await map(game.actor);
  game.check('tower activates and spawns five owned Eyes',members(m,0xd5d).filter(e=>e.parent===towers[0].id).length===5);
  game.check('Eyes leave their birth tower',samples.some(s=>s.diagnostic.entities.some(e=>e.kind===0xd5d&&e.parent===towers[0].id&&Math.hypot(e.pos.x-towers[0].pos.x,e.pos.y-towers[0].pos.y)>1.5)));
  game.check('Eyes publish speed-150, 500ms projectiles',samples.some(s=>s.view.projectiles.some(p=>members(m,0xd5d).some(e=>e.id===p.owner)&&p.speed===150&&p.lifetime_ms===500)));
  game.check('unprotected native character takes encounter damage',(await game.observe()).character.hp<before.view.character.hp);
  game.check('living towers publish red connections',packets.some(p=>p.effect===3&&p.color===0xffff0000&&towers.some(t=>t.id===p.owner)));
  game.check('tower throws its grenade',packets.some(p=>p.effect===4&&p.owner===towers[0].id));
  await cheat(game.actor,{kind:'protection',condition:'invulnerable',duration_ms:60000});
  await cheat(game.actor,{kind:'stat_modifier',index:2,amount:100,duration_ms:60000});
  async function approach(point){
    for(let n=0;n<150;n++){
      const v=await game.observe(),dx=point.x-v.position.x,dy=point.y-v.position.y;if(Math.hypot(dx,dy)<.35)return;
      const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,keys=[];
      if(Math.abs(x)>=Math.abs(y)*.4142)keys.push(x>0?'d':'a');if(Math.abs(y)>=Math.abs(x)*.4142)keys.push(y>0?'s':'w');
      await game.act({keys,duration_ms:Math.max(50,Math.min(500,Math.round(Math.hypot(dx,dy)/(4+5.6*v.character.stats.speed/75)*1000)))});
    }
    throw Error('Native walking did not reach the observed tower');
  }
  async function kill(tower){
    await approach(tower.pos);m=await map(game.actor);
    for(let n=0;n<12&&m.entities.some(e=>e.id===tower.id);n++){await game.act({keys:[],duration_ms:150,target:{entity:tower.id,camera_angle:-Math.PI/4,fire:true}});m=await map(game.actor);}
    assert(!m.entities.some(e=>e.id===tower.id),'Tower survived bounded native fire');
  }
  await kill(towers[0]);const downAt=(await game.observe()).game_ms;
  await game.act({keys:[],duration_ms:500},{name:'pentaract-one-down',when:v=>v.game_ms>=downAt+250});await save('one-down',m);
  const corpse=members(m,0xd60)[0];
  game.check('one fallen tower changes connections to yellow',packets.some(p=>p.effect===3&&p.color===0xffffff00));
  game.check('one corpse does not complete the encounter',members(m,0xd5e).length===4&&!!corpse&&m.entities.some(e=>e.id===rootEntity.id));
  game.check('contribution reaches corpse and controller',Object.values(corpse.damage).some(n=>n>0)&&Object.values(m.entities.find(e=>e.id===rootEntity.id).damage).some(n=>n>0));
  const beforeRevive=corpse.activated+14900;
  while((await game.observe()).game_ms<beforeRevive)await game.act({keys:[],duration_ms:Math.min(7000,beforeRevive-(await game.observe()).game_ms)});
  m=await map(game.actor);game.check('corpse remains before 15 seconds',members(m,0xd60).some(e=>e.id===corpse.id));
  await game.act({keys:[],duration_ms:500},{name:'pentaract-revived',when:()=>true});m=await map(game.actor);await save('revived',m);
  game.check('corpse revives at full HP in its original position',members(m,0xd60).length===0&&members(m,0xd5e).length===5&&members(m,0xd5e).some(e=>e.hp===5000&&e.pos.x===corpse.pos.x&&e.pos.y===corpse.pos.y));
  const packetStart=packets.length,start=await game.inspect();
  for(const tower of members(m,0xd5e))await kill(tower);
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
