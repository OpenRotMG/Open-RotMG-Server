import assert from 'node:assert/strict';
import {writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
import {connect} from '../../scripts/record-demo.mjs';

export const fixture={class:0x30e,level:5,seed:786,world:1,position:'spawn',equipment:[0xb08,0xa2e,0xb05,-1],backpack:Array(8).fill(-1),stats:[670,385,75,25,50,75,40,60]};
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
  game.check('level-five character has a published level-five quest',!!target&&target.level===5);
  game.check('quest object is retained outside local view',Math.hypot(target.position.x-start.view.position.x,target.position.y-start.view.position.y)>20);
  await game.act({keys:[],duration_ms:1500},{name:'quest-initial-delay',when:()=>true});
  await game.act({keys:[],duration_ms:4500});
  await game.act({keys:[],duration_ms:500},{name:'quest-distant',when:()=>true});
  await prepare({kind:'teleport',position:{x:target.position.x+4,y:target.position.y}});
  await game.act({keys:[],duration_ms:500},{name:'quest-near',when:()=>true});
  const near=await save('quest-near'),boss=near.diagnostic.entities.find(o=>o.id===target.id);
  game.check('approaching quest activates the ordinary enemy',!!boss&&boss.activated!==null);
  const packetStart=clients[0].packets.length;
  for(let n=0;n<16;n++){
    if(!(await game.inspect()).diagnostic.entities.some(o=>o.id===target.id))break;
    await game.act({keys:[],duration_ms:500,target:{entity:target.id,camera_angle:-Math.PI/4,fire:true}});
  }
  const defeated=await save('quest-defeated'),events=clients[0].packets.slice(packetStart);
  game.check('native shooting defeats the assigned quest and awards XP',!defeated.diagnostic.entities.some(o=>o.id===target.id)&&defeated.view.character.experience>near.view.character.experience);
  game.check('completion notification occurs exactly once',events.filter(p=>p.id===63&&p.text==='Quest Complete!').length===1);
  const completion=events.findIndex(p=>p.id===63&&p.text==='Quest Complete!'),reselection=events.findIndex(p=>p.id===34&&p.target!==target.id);
  game.check('server publishes completion before reselection',completion>=0&&reselection>completion&&defeated.view.quest!==target.id);
  await game.act({keys:[],duration_ms:1500},{name:'quest-completed',when:()=>true});
  await game.act({keys:[],duration_ms:8000});
  await game.act({keys:[],duration_ms:8000});
  await game.act({keys:[],duration_ms:500},{name:'quest-next',when:()=>true});
  game.check('same living character retains its inventory',defeated.view.lifecycle.alive&&defeated.view.session.character===start.view.session.character&&JSON.stringify(defeated.view.character.items)===JSON.stringify(start.view.character.items));
  await prepare({kind:'teleport',position:start.view.position});
  for(const level of [1,20]){
    game=await game.join({...fixture,level,equipment:[-1,-1,-1,-1]});
    await prepare({kind:'protection',condition:'invulnerable',duration_ms:60000});
    const state=await save('quest-level-'+level),quest=state.view.objects.find(o=>o.id===state.view.quest);
    game.check('level '+level+' receives an eligible published quest',!!quest&&(level===1?quest.level===1:quest.level>=14));
    await game.act({keys:[],duration_ms:6000});
    await game.act({keys:[],duration_ms:500},{name:'quest-level-'+level,when:()=>true});
  }
  await writeFile(join(process.env.QA_RESULTS_DIR,'quest-packets.json'),JSON.stringify(clients.map(c=>c.packets),null,2));
  for(const client of clients)client.connection.close();
}
