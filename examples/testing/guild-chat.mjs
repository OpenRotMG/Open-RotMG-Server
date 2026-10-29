import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {setTimeout as delay} from 'node:timers/promises';
import {randomUUID} from 'node:crypto';
import {connect} from '../../scripts/record-demo.mjs';

export const fixture={class:0x30e,level:5,seed:906,world:-2,position:'spawn',equipment:[-1,-1,-1,-1],backpack:Array(8).fill(-1),stats:null};
const clients=[];
export async function beforeNavigate({browser,signal}){
  const packets=[],streams=new Map();
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
      if(id===25){
        let offset=0;const utf=()=>{const n=body.readUInt16BE(offset);offset+=2;const text=body.subarray(offset,offset+n).toString();offset+=n;return text;};
        const name=utf();offset+=9;const recipient=utf(),text=utf();packets.push({name,recipient,text});
      }
    }
  }});
  await connection.call('Network.enable');signal.addEventListener('abort',()=>connection.close(),{once:true});clients.push({connection,packets});
}
export async function run(game){
  const out=process.env.QA_RESULTS_DIR,root=join(out,'native');
  const peer=await game.join({...fixture,seed:907}),outsider=await game.join({...fixture,seed:908});
  const config=JSON.parse(await readFile(join(root,'native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const api=async command=>{const r=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command}),signal:AbortSignal.timeout(10000)});const s=await r.json();assert(r.ok&&s.status==='succeeded',JSON.stringify(s));return s;};
  const records=[];
  async function record(label){records.push({label,states:await Promise.all([game.inspect(),peer.inspect(),outsider.inspect()]),messages:clients.map(c=>[...c.packets])});await writeFile(join(out,'guild-chat-observations.json'),JSON.stringify(records,null,2));}
  async function chat(player,index,text){
    const log=join(root,'controller.stderr'),offset=(await readFile(log,'utf8')).length;
    const action=player.act({keys:[],duration_ms:2000});
    const input=(async()=>{
      const deadline=Date.now()+5000;
      while(true){
        const lines=(await readFile(log,'utf8')).slice(offset).split('\n').slice(0,-1);
        const acknowledged=new Set(lines.filter(l=>l.startsWith('{"event":"native_sync_verified_ack"')).map(l=>JSON.parse(l).context.actor));
        if(config.native.every(c=>acknowledged.has(c.actor)))break;
        assert(Date.now()<deadline,'Chat requires the current action cohort ACKs before a command can reconnect');await delay(20);
      }
      const c=clients[index].connection;
      async function enter(){try{await c.call('Input.dispatchKeyEvent',{type:'keyDown',key:'Enter',code:'Enter',windowsVirtualKeyCode:13});}finally{await c.call('Input.dispatchKeyEvent',{type:'keyUp',key:'Enter',code:'Enter',windowsVirtualKeyCode:13});}}
      await enter();await c.call('Input.insertText',{text});await enter();
    })();
    const results=await Promise.allSettled([action,input]);for(const r of results)if(r.status==='rejected')throw r.reason;
    await record(text);
  }
  const has=(index,text)=>clients[index].packets.some(p=>p.text===text);
  const guild=(index,text)=>clients[index].packets.filter(p=>p.recipient==='*Guild*'&&p.text===text);
  await game.act({keys:[],duration_ms:1000},{name:'guild-before',when:()=>true});
  await chat(outsider,2,'/g outsiders cannot send');
  game.check('nonmember gets an explicit refusal without guild delivery',has(2,'You are not in a guild.')&&!has(0,'outsiders cannot send')&&!has(1,'outsiders cannot send'));
  await outsider.act({keys:[],duration_ms:500},{name:'guild-refused',when:()=>true});
  await api({op:'cheat',actor:game.actor,change:{kind:'guild',name:'Chat Research',members:[peer.actor]}});
  await game.act({keys:[],duration_ms:1000});
  const initial=await game.inspect(),name=initial.diagnostic.character.name;
  await chat(game,0,'/g Hello guild');
  game.check('g delivers once to founder and initiate but not outsider',guild(0,'Hello guild').length===1&&guild(1,'Hello guild').length===1&&!has(2,'Hello guild'));
  await game.act({keys:[],duration_ms:500},{name:'guild-founder',when:()=>true});
  await peer.act({keys:[],duration_ms:500},{name:'guild-initiate',when:()=>true});
  // The period guide grants both aliases to every rank: https://web.archive.org/web/20120217160934id_/http://forums.wildshadow.com/wiki/realm-mad-god/guilds
  await chat(peer,1,'/guild Hello founder');
  game.check('guild alias permits lowest rank to reply',guild(0,'Hello founder').length===1&&guild(1,'Hello founder').length===1&&!has(2,'Hello founder'));
  await chat(game,0,'/g');
  game.check('empty guild message produces usage and no broadcast',has(0,'Usage: /g <message>')&&!clients[1].packets.some(p=>p.recipient==='*Guild*'&&p.text===''));
  await chat(peer,1,'/ignore '+name);
  game.check('ordinary ignore command succeeds',has(1,'Player ignored.'));
  await chat(game,0,'/g Ignored message');
  game.check('ignore suppresses only that recipient and retains sender echo',guild(0,'Ignored message').length===1&&!has(1,'Ignored message')&&!has(2,'Ignored message'));
  await peer.act({keys:[],duration_ms:500},{name:'guild-ignored',when:()=>true});
  await chat(peer,1,'/tutorial');
  const separate=await peer.observe();
  game.check('ordinary tutorial reconnect separates the two members',separate.session.world!==(await game.observe()).session.world);
  await chat(game,0,'/g Still ignored after reconnect');
  game.check('account ignore persists through ordinary world reconnect',guild(0,'Still ignored after reconnect').length===1&&!has(1,'Still ignored after reconnect'));
  await chat(peer,1,'/unignore '+name);
  game.check('ordinary unignore command succeeds',has(1,'Player unignored.'));
  await chat(game,0,'/guild Across worlds');
  game.check('guild routing crosses worlds after reconnect',guild(0,'Across worlds').length===1&&guild(1,'Across worlds').length===1&&!has(2,'Across worlds'));
  await peer.act({keys:[],duration_ms:500},{name:'guild-cross-world',when:()=>true});
  await peer.act({keys:['Insert'],duration_ms:700});
  await peer.act({keys:[],duration_ms:1000});
  game.check('Nexus reconnect preserves the same living character', (await peer.observe()).session.character===separate.session.character&&(await peer.observe()).session.world===(await game.observe()).session.world);
  await chat(peer,1,'/g Back in Nexus');
  game.check('chat still routes after return and never leaks to outsider',guild(0,'Back in Nexus').length===1&&guild(1,'Back in Nexus').length===1&&!has(2,'Back in Nexus'));
  await chat(game,0,'/pause');
  game.check('ordinary pause applies the native Paused condition',!!((await game.observe()).character.conditions&(1<<20)));
  await chat(game,0,'/g Paused but listening');
  game.check('Paused alone does not block guild chat',guild(0,'Paused but listening').length===1&&guild(1,'Paused but listening').length===1&&!has(2,'Paused but listening'));
  await chat(game,0,'/pause');
  game.check('ordinary pause command resumes the same character',!((await game.observe()).character.conditions&(1<<20)));
  game.check('world reconnects do not create extra sign-on notices',clients.every(c=>!c.packets.some(p=>p.recipient==='*Guild*'&&p.text.endsWith(' has come online.'))));
  await game.act({keys:[],duration_ms:500},{name:'guild-reconnected',when:()=>true});
  await outsider.act({keys:[],duration_ms:500},{name:'guild-outsider',when:()=>true});
  await record('final');
  game.check('all three ordinary native participants remain alive',(await Promise.all([game.observe(),peer.observe(),outsider.observe()])).every(s=>s.lifecycle.alive));
}
