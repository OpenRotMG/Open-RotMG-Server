import assert from 'node:assert/strict';
import {nativeMap} from './native-map.mjs';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
import {connect} from '../../scripts/record-demo.mjs';

export const fixture={class:0x30e,level:5,seed:786,world:1,position:'spawn',equipment:[0xb08,0xa2e,0xb05,-1],backpack:Array(8).fill(-1),stats:[670,385,75,25,50,75,40,60]};
export const realmSeed=106;
export async function prepare(api){
  const {spawn_points:[[x,y]]}=await api({op:'world_info',world:fixture.world});
  const scout=await api({op:'spawn',spec:{...fixture,guid:'quest-scout',position:{x,y},controller:'direct',profile:null}});
  const state=await api({op:'inspect',actor:scout.result.actor}),root=join(process.env.QA_RESULTS_DIR,'native');
  const {origin}=JSON.parse(await readFile(join(root,'result.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(root,'state/control.token'),'utf8')).trim()};
  const map=await nativeMap(origin,headers,state.diagnostic.player),blocked=new Set(map.blocked.map(p=>p.join(',')));
  // Dwarf King, Swarm and Desert Werewolf have selected quest level five; position the player outside both the winner's local view and its 25-tile XP range.
  const peers=map.entities.filter(e=>[0x623,0x955,0x61f].includes(e.kind));
  for(const swarm of peers.filter(e=>e.kind===0x955))for(const [dx,dy] of [[0,32],[32,0],[-32,0],[0,-32]]){
    const position={x:swarm.pos.x+dx,y:swarm.pos.y+dy};
    if(position.x<1||position.y<1||blocked.has([Math.floor(position.x),Math.floor(position.y)].join(',')))continue;
    if(peers.some(e=>e.id!==swarm.id&&Math.hypot(e.pos.x-position.x,e.pos.y-position.y)<=32))continue;
    await writeFile(join(process.env.QA_RESULTS_DIR,'quest-prepared.json'),JSON.stringify({realmSeed,position,swarm}));
    return {...fixture,position};
  }
  throw Error('No distant, naturally allocated Swarm with a clear prepared entrance');
}
const clients=[];
export async function beforeNavigate({browser,signal}){
  const packets=[],streams=new Map();
  const connection=await connect({...browser,url:'about:blank',helper:join(process.env.PI_BROWSER_TOOLS,'direct-cdp.mjs'),onEvent:event=>{
    if(event.method!=='Network.webSocketFrameReceived'||event.params.response.opcode!==2)return;
    const {requestId,response,timestamp}=event.params;
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
      if(id===34)packets.push({id,target:body.readInt32BE(),timestamp});
      if(id===63)packets.push({id,player:body.readInt32BE(),text:body.subarray(6,6+body.readUInt16BE(4)).toString(),timestamp});
    }
  }});
  await connection.call('Network.enable');signal.addEventListener('abort',()=>connection.close(),{once:true});clients.push({connection,packets});
}
export async function run(game){
  let serial=0;
  const save=async name=>{const state=await game.inspect();await writeFile(join(process.env.QA_RESULTS_DIR,name+'.json'),JSON.stringify(state,null,2));return state;};
  async function prepare(change){
    const path=join(process.env.QA_RESULTS_DIR,'preparation-'+serial+'.json');
    await writeFile(path,JSON.stringify({id:'quest-preparation-'+serial++,command:{op:'cheat',actor:game.actor,change}}));
    execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(process.env.QA_RESULTS_DIR,'native/native.json'),path]);
  }
  await prepare({kind:'protection',condition:'invulnerable',duration_ms:60000});
  const start=await save('quest-initial'),target=start.view.objects.find(o=>o.id===start.view.quest);
  game.check('level-five character receives the selected Swarm quest',target?.kind===0x955);
  game.check('quest object is retained outside local view',Math.hypot(target.position.x-start.view.position.x,target.position.y-start.view.position.y)>20);
  await game.act({keys:[],duration_ms:3500},{name:'quest-initial-delay',when:view=>view.game_ms>=2500});
  await game.act({keys:[],duration_ms:2500});
  await game.act({keys:[],duration_ms:500},{name:'quest-distant',when:()=>true});
  const distant=game,distantPackets=clients[0].packets.length;
  game=await game.join({...fixture,position:target.position});
  await prepare({kind:'protection',condition:'invulnerable',duration_ms:60000});
  await game.act({keys:['s'],duration_ms:1000});
  await game.act({keys:[],duration_ms:500},{name:'quest-near',when:()=>true});
  const near=await save('quest-near'),boss=near.diagnostic.entities.find(o=>o.id===target.id);
  game.check('approaching quest activates the ordinary enemy',!!boss&&boss.activated!==null);
  const packetStart=clients[1].packets.length;
  for(let n=0;n<16;n++){
    const {view:{position,objects}}=await game.inspect(),visible=objects.find(o=>o.id===target.id&&o.hp>0);
    if(!visible)break;
    // A projectile can retire the Swarm before the controller resolves a second target snapshot.
    const dx=visible.position.x-position.x,dy=visible.position.y-position.y;
    const scale=50*Math.SQRT1_2/Math.max(1,50*Math.hypot(dx,dy)/110);
    await game.act({keys:[],duration_ms:500,mouse:{x:300+(dx-dy)*scale,y:325+(dx+dy)*scale}});
  }
  await game.act({keys:[],duration_ms:500});
  const defeated=await save('quest-defeated'),events=clients[1].packets.slice(packetStart);
  await writeFile(join(process.env.QA_RESULTS_DIR,'quest-packets.json'),JSON.stringify(clients.map(c=>c.packets),null,2));
  game.check('native shooting defeats the assigned quest and awards XP',!defeated.diagnostic.entities.some(o=>o.id===target.id)&&defeated.view.character.experience>near.view.character.experience);
  game.check('completion notification occurs exactly once',events.filter(p=>p.id===63&&p.text==='Quest Complete!').length===1);
  const completion=events.findIndex(p=>p.id===63&&p.text==='Quest Complete!');
  // Nearby moving quests can trade places before the defeated Swarm's final assignment.
  const lastAssigned=events.findLastIndex(p=>p.id===34&&p.target===target.id);
  const reselection=events.findIndex((p,i)=>i>lastAssigned&&p.id===34&&p.target!==target.id);
  game.check('server publishes completion before reselection',completion>lastAssigned&&reselection>completion&&defeated.view.quest!==target.id);
  await game.act({keys:[],duration_ms:1500},{name:'quest-completed',when:()=>true});
  await game.act({keys:[],duration_ms:8000});
  await game.act({keys:[],duration_ms:8000});
  await game.act({keys:[],duration_ms:500},{name:'quest-next',when:()=>true});
  game.check('same living combat character retains its inventory',defeated.view.lifecycle.alive&&defeated.view.session.account===near.view.session.account&&defeated.view.session.character===near.view.session.character&&JSON.stringify(defeated.view.character.items)===JSON.stringify(near.view.character.items));
  const remote=await distant.inspect();
  await writeFile(join(process.env.QA_RESULTS_DIR,'quest-remote-after.json'),JSON.stringify(remote,null,2));
  game.check('distant observer loses defeated target without XP or completion feedback',remote.view.quest!==target.id&&remote.view.character.experience===start.view.character.experience&&!clients[0].packets.slice(distantPackets).some(p=>p.id===63&&p.text==='Quest Complete!'));
  for(const level of [20]){
    game=await game.join({...fixture,level,equipment:[-1,-1,-1,-1]});
    await prepare({kind:'protection',condition:'invulnerable',duration_ms:60000});
    const state=await save('quest-level-'+level),quest=state.view.objects.find(o=>o.id===state.view.quest);
    game.check('level twenty receives the published Red Demon, not an outgrown Swarm',quest?.kind===0x668);
    await game.act({keys:[],duration_ms:6000});
    await game.act({keys:[],duration_ms:500},{name:'quest-level-'+level,when:()=>true});
  }
  await writeFile(join(process.env.QA_RESULTS_DIR,'quest-packets.json'),JSON.stringify(clients.map(c=>c.packets),null,2));
  for(const client of clients)client.connection.close();
}
