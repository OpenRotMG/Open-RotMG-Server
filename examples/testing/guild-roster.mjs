import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
import {connect} from '../../scripts/record-demo.mjs';

export const fixture={class:0x30e,level:5,seed:915,world:-2,position:{x:76.5,y:78.5},equipment:[-1,-1,-1,-1],backpack:Array(8).fill(-1),stats:null};
const clients=[];
export async function beforeNavigate({browser,signal}){
  const client={rosters:[],pending:new Set()};
  const connection=await connect({...browser,url:'about:blank',helper:join(process.env.PI_BROWSER_TOOLS,'direct-cdp.mjs'),onEvent:event=>{
    if(event.method==='Network.requestWillBeSent'&&event.params.request.url.endsWith('/guild/listMembers'))client.form=new URLSearchParams(event.params.request.postData);
    if(event.method==='Network.responseReceived'&&event.params.response.url.endsWith('/guild/listMembers'))client.pending.add(event.params.requestId);
    if(event.method==='Network.loadingFinished'&&client.pending.delete(event.params.requestId)){
      connection.call('Network.getResponseBody',{requestId:event.params.requestId}).then(r=>client.rosters.push(r.base64Encoded?Buffer.from(r.body,'base64').toString():r.body)).catch(e=>{client.error=e;});
    }
  }});
  client.connection=connection;
  await connection.call('Network.enable');signal.addEventListener('abort',()=>connection.close(),{once:true});clients.push(client);
}
export async function run(game){
  const out=process.env.QA_RESULTS_DIR,root=join(out,'native');
  const peer=await game.join({...fixture,seed:916});
  const config=JSON.parse(await readFile(join(root,'native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const api=async command=>{const r=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command}),signal:AbortSignal.timeout(10000)});const s=await r.json();assert(r.ok&&s.status==='succeeded',JSON.stringify(s));return s;};
  const records=[];
  async function record(label){
    for(const c of clients)if(c.error)throw c.error;
    records.push({label,states:await Promise.all([game.inspect(),peer.inspect()]),rosters:clients.map(c=>[...c.rosters])});
    await writeFile(join(out,'guild-roster-observations.json'),JSON.stringify(records,null,2));
  }
  async function click(player,x,y){await player.act({keys:[],mouse:{x,y},duration_ms:400});await player.act({keys:[],duration_ms:400});}
  async function capture(player,name){await player.act({keys:[],duration_ms:900},{name,when:()=>true});await record(name);}
  async function walk(player,pos){
    for(let i=0;i<80;i++){
      const v=await player.observe(),dx=pos.x-v.position.x,dy=pos.y-v.position.y,distance=Math.hypot(dx,dy);
      if(distance<.6)return;
      const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,keys=[];
      if(Math.abs(x)>.12)keys.push(x>0?'d':'a');if(Math.abs(y)>.12)keys.push(y>0?'s':'w');
      await player.act({keys,duration_ms:Math.min(300,Math.max(50,Math.round(distance*80)))});
    }
    throw Error('Native walk did not reach Chronicle');
  }
  await capture(game,'roster-before');
  await api({op:'cheat',actor:game.actor,change:{kind:'guild',name:'Roster Research',members:[peer.actor]}});
  await game.act({keys:[],duration_ms:700});
  for(const player of [game,peer]){
    await click(player,700,575);
    game.check('ordinary Guild Hall portal transfers member '+player.actor,(await player.observe()).world==='Guild Hall');
    await walk(player,{x:14.5,y:8.5});
    await click(player,700,575);
    await capture(player,player===game?'roster-founder':'roster-initiate');
  }
  const roster=index=>clients[index].rosters.at(-1);
  const rows=index=>[...roster(index).matchAll(/<Member><Name>([^<]+)<\/Name><Rank>(\d+)<\/Rank><Fame>(\d+)<\/Fame><\/Member>/g)].map(m=>({name:m[1],rank:Number(m[2]),fame:Number(m[3])}));
  game.check('both clients request all fifty slots at offset zero',clients.every(c=>c.form.get('num')==='50'&&c.form.get('offset')==='0'));
  game.check('both clients load the same two-member roster',clients.every(c=>c.rosters.length===1)&&roster(0)===roster(1)&&rows(0).length===2&&rows(0)[0].rank===40&&rows(0)[1].rank===0&&rows(0).every(r=>r.fame===0));
  const original=roster(0),peerName=rows(0)[1].name;
  await click(peer,686,126);
  game.check('initiate cannot invoke a Founder management control',clients[1].rosters.length===1&&roster(1)===original);
  await capture(peer,'roster-no-controls');
  await click(game,686,158);await capture(game,'roster-promote-confirmation');
  await click(game,325,335);
  game.check('cancelled promotion leaves roster unchanged',clients[0].rosters.length===1&&roster(0)===original);
  await click(game,686,158);await click(game,475,335);
  // The selected screen reloads on acceptance, so require a new response: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Rotmg123.5.1/src/_sP_/_07x.as
  game.check('accepted promotion reloads the actual Member rank',clients[0].rosters.length===2&&rows(0).find(r=>r.name===peerName)?.rank===10);
  await capture(game,'roster-promoted');
  await click(peer,400,540);await click(peer,700,575);
  game.check('peer sees persisted promotion after reopening',rows(1).find(r=>r.name===peerName)?.rank===10);
  await capture(peer,'roster-peer-promoted');
  await click(game,716,158);await click(game,475,335);
  game.check('accepted demotion reloads Initiate',clients[0].rosters.length===3&&rows(0).find(r=>r.name===peerName)?.rank===0);
  await click(game,747,158);await capture(game,'roster-remove-confirmation');await click(game,325,335);
  game.check('cancelled removal retains both members',clients[0].rosters.length===3&&rows(0).length===2);
  await click(game,747,158);await click(game,475,335);
  game.check('accepted removal reloads the one-member roster',clients[0].rosters.length===4&&rows(0).length===1&&rows(0)[0].rank===40);
  await capture(game,'roster-removed');
  await peer.act({keys:[],duration_ms:700});
  game.check('removed peer is durably returned to Nexus',(await peer.observe()).world==='Nexus');
  await capture(peer,'roster-peer-removed');
  const refused=await clients[1].connection.call('Runtime.evaluate',{expression:`fetch(${JSON.stringify(config.origin+'/legacy-api/guild/listMembers')},{method:'POST',body:new URLSearchParams(${JSON.stringify(clients[1].form.toString())})}).then(r=>r.text())`,awaitPromise:true,returnByValue:true});
  game.check('removed account cannot read the authenticated roster',refused.result.value==='<Error>You are not in a guild.</Error>');
  const final=roster(0);
  await click(game,400,540);await game.act({keys:['Insert'],duration_ms:700});await game.act({keys:[],duration_ms:700});
  game.check('Founder ordinary reconnect returns to Nexus',(await game.observe()).world==='Nexus');
  await walk(game,{x:76.5,y:78.5});await click(game,700,575);
  game.check('Founder still enters the same guild hall',(await game.observe()).world==='Guild Hall');
  await walk(game,{x:14.5,y:8.5});await click(game,700,575);
  game.check('ordinary reconnect preserves the final roster',clients[0].rosters.length===5&&roster(0)===final);
  await capture(game,'roster-reconnected');
  game.check('both ordinary clients remain alive',(await game.observe()).lifecycle.alive&&(await peer.observe()).lifecycle.alive);
}
