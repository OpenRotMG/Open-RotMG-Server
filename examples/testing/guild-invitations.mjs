import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {randomUUID} from 'node:crypto';
import {setTimeout as delay} from 'node:timers/promises';
import {connect} from '../../scripts/record-demo.mjs';

const root=join(process.env.QA_RESULTS_DIR,'native'),exec=promisify(execFile),clients=[];
export const fixture={class:0x30e,level:20,seed:912,world:-2,position:{x:109.5,y:42.2},equipment:[-1,-1,-1,-1],backpack:Array(8).fill(-1),stats:null};
export async function beforeNavigate({browser,signal}){
  const packets=[],streams=new Map();
  const c=await connect({...browser,url:'about:blank',helper:join(process.env.PI_BROWSER_TOOLS,'direct-cdp.mjs'),onEvent:e=>{
    if(e.method!=='Network.webSocketFrameReceived'||e.params.response.opcode!==2)return;
    const {requestId,response}=e.params;
    if(!streams.has(requestId)){
      const key=Buffer.from('72c5583cafb6818995cbd74b80','hex'),s=Array.from({length:256},(_,i)=>i);let j=0;
      for(let i=0;i<256;i++){j=(j+s[i]+key[i%key.length])&255;[s[i],s[j]]=[s[j],s[i]];}
      streams.set(requestId,{s,i:0,j:0,buffer:Buffer.alloc(0)});
    }
    const t=streams.get(requestId);t.buffer=Buffer.concat([t.buffer,Buffer.from(response.payloadData,'base64')]);
    while(t.buffer.length>=5){
      const n=t.buffer.readInt32BE();assert(n>=5&&n<=1048576);if(t.buffer.length<n)break;
      const id=t.buffer[4],b=Buffer.from(t.buffer.subarray(5,n));t.buffer=t.buffer.subarray(n);
      for(let k=0;k<b.length;k++){t.i=(t.i+1)&255;t.j=(t.j+t.s[t.i])&255;[t.s[t.i],t.s[t.j]]=[t.s[t.j],t.s[t.i]];b[k]^=t.s[(t.s[t.i]+t.s[t.j])&255];}
      if(id===25){let o=0;const utf=()=>{const n=b.readUInt16BE(o);o+=2;const v=b.subarray(o,o+n).toString();o+=n;return v;};const name=utf();o+=9;packets.push({name,recipient:utf(),text:utf()});}
    }
  }});
  await c.call('Network.enable');await c.call('Emulation.setFocusEmulationEnabled',{enabled:true});
  clients.push({c,packets});signal.addEventListener('abort',()=>c.close(),{once:true});
}
async function db(sql,params=[]){
  const {stdout}=await exec('python3',['-c','import sqlite3,json,sys; c=sqlite3.connect(sys.argv[1]); r=c.execute(sys.argv[2],json.loads(sys.argv[3])); rows=r.fetchall(); c.commit(); print(json.dumps(rows))',join(root,'state/game.db'),sql,JSON.stringify(params)]);
  return JSON.parse(stdout);
}
export async function enter({account}){
  // These unearned starting records exercise eligibility without manufacturing paid death rewards.
  await db('UPDATE accounts SET name=? WHERE id=?',['Guild'+['Alpha','Beta','Gamma'][clients.length-1],account]);
  await db("UPDATE characters SET stats=json_set(stats,'$.fame',20) WHERE account=?",[account]);
  const c=clients.at(-1).c;
  await c.eval('(()=>{const p=document.querySelector("ruffle-player"),m=p.shadowRoot.querySelector("#hardware-acceleration-modal");if(m&&!m.classList.contains("hidden"))m.querySelector(".close-modal").click();})()');
  for(let i=0;i<2;i++){await click(c,400,550);await c.eval('new Promise(ok=>requestAnimationFrame(()=>requestAnimationFrame(ok)))');}
}
async function click(c,x,y){
  const r=await c.eval('(()=>{const r=document.querySelector("ruffle-player").getBoundingClientRect();return{x:r.x,y:r.y};})()'),p={x:r.x+x,y:r.y+y,button:'left',clickCount:1};
  await c.call('Input.dispatchMouseEvent',{type:'mouseMoved',...p});
  try{await c.call('Input.dispatchMouseEvent',{type:'mousePressed',buttons:1,...p});}finally{await c.call('Input.dispatchMouseEvent',{type:'mouseReleased',buttons:0,...p});}
}
export async function run(game){
  const peer=await game.join({...fixture,seed:913}),other=await game.join({...fixture,seed:914});
  const config=JSON.parse(await readFile(join(root,'native.json'),'utf8')),token=(await readFile(join(config.state,'control.token'),'utf8')).trim(),records=[];
  const api=async command=>{const r=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command}),signal:AbortSignal.timeout(10000)});const s=await r.json();assert(r.ok&&s.status==='succeeded',JSON.stringify(s));};
  async function save(label){records.push({label,states:await Promise.all([game.inspect(),peer.inspect(),other.inspect()]),messages:clients.map(c=>[...c.packets]),members:await db('SELECT a.name,g.name,m.rank FROM guild_members m JOIN accounts a ON a.id=m.account JOIN guilds g ON g.id=m.guild ORDER BY a.id')});await writeFile(join(process.env.QA_RESULTS_DIR,'guild-invitations.json'),JSON.stringify({preparedNamesAndLivingFame:true,records},null,2));}
  async function ui(player,index,label,work){
    const log=join(root,'controller.stderr'),offset=(await readFile(log,'utf8')).length;
    const action=player.act({keys:[],duration_ms:2500});
    const input=(async()=>{
      const end=Date.now()+6000;
      while(true){const lines=(await readFile(log,'utf8')).slice(offset).split('\n').slice(0,-1),acks=new Set(lines.filter(l=>l.startsWith('{"event":"native_sync_verified_ack"')).map(l=>JSON.parse(l).context.actor));if(config.native.every(c=>acks.has(c.actor)))break;assert(Date.now()<end,'UI requires current cohort ACKs');await delay(20);}
      await work(clients[index].c);
    })();
    const results=await Promise.allSettled([action,input]);for(const r of results)if(r.status==='rejected')throw r.reason;
    await save(label);
  }
  const chat=(player,index,text)=>ui(player,index,text,async c=>{async function enter(){try{await c.call('Input.dispatchKeyEvent',{type:'keyDown',key:'Enter',code:'Enter',windowsVirtualKeyCode:13});}finally{await c.call('Input.dispatchKeyEvent',{type:'keyUp',key:'Enter',code:'Enter',windowsVirtualKeyCode:13});}}await enter();await c.call('Input.insertText',{text});await enter();});
  const button=(player,index,label,x,y)=>ui(player,index,label,c=>click(c,x,y));
  const capture=(player,name)=>player.act({keys:[],duration_ms:700},{name,when:()=>true});
  const members=()=>db('SELECT a.name,g.name,m.rank FROM guild_members m JOIN accounts a ON a.id=m.account JOIN guilds g ON g.id=m.guild ORDER BY a.id');
  async function refreshRegister(player){await walk(player,109.5,44.5);await walk(player,109.5,41.5);}
  async function walk(player,x,y){
    for(let i=0;i<80;i++){
      const p=(await player.observe()).position,dx=x-p.x,dy=y-p.y,d=Math.hypot(dx,dy);if(d<.8)return;
      const sx=(dx-dy)/Math.SQRT2,sy=(dx+dy)/Math.SQRT2,keys=[];
      if(Math.abs(sx)>.08)keys.push(sx>0?'d':'a');if(Math.abs(sy)>.08)keys.push(sy>0?'s':'w');
      await player.act({keys,duration_ms:Math.min(650,Math.max(20,Math.round(d*80)))});
    }
    assert.fail('Native walk did not reach '+JSON.stringify({x,y,position:(await player.observe()).position}));
  }
  const has=(i,text)=>clients[i].packets.some(p=>p.text===text);
  await capture(game,'before-guild');
  await chat(peer,1,'/invite GuildAlpha');
  game.check('nonmember cannot invite',has(1,'You are not in a guild.'));
  await api({op:'cheat',actor:game.actor,change:{kind:'guild',name:'Alpha Guild',members:[]}});
  await api({op:'cheat',actor:other.actor,change:{kind:'guild',name:'Gamma Guild',members:[]}});
  await game.act({keys:[],duration_ms:700});
  await chat(game,0,'/invite GuildBeta');await capture(peer,'invitation-reject');
  await button(peer,1,'reject',650,580);
  game.check('Reject leaves recipient outside both guilds',(await members()).length===2);
  await chat(game,0,'/invite GuildBeta');await capture(peer,'invitation-alpha');
  // The selected 123.5.1 GuildInvitePanel sends only joinGuild on Accept; Reject is local: https://openrotmg.org/restoration/reference-data/client-123.5.1.swf
  await button(peer,1,'accept-alpha',750,580);
  game.check('Accept joins exactly the named guild as Initiate',JSON.stringify((await members()).find(r=>r[0]==='GuildBeta'))===JSON.stringify(['GuildBeta','Alpha Guild',0]));
  game.check('founder observes the ordinary join notice',has(0,'GuildBeta joined the guild.'));
  await capture(peer,'joined-alpha');await capture(game,'founder-observes-join');
  await chat(peer,1,'/invite GuildGamma');
  game.check('Initiate cannot invite',has(1,'Only Officers, Leaders and the Founder may invite players.'));
  await refreshRegister(peer);
  await button(peer,1,'open-renounce-cancel',700,580);await capture(peer,'renounce-confirmation');
  await button(peer,1,'cancel-renounce',325,340);
  game.check('cancelled renunciation preserves membership',(await members()).length===3);
  await button(peer,1,'open-renounce',700,580);await button(peer,1,'confirm-renounce',475,340);
  game.check('voluntary renunciation removes only self',(await members()).length===2&&!((await members()).some(r=>r[0]==='GuildBeta')));
  await chat(game,0,'/invite GuildBeta');await chat(other,2,'/invite GuildBeta');await capture(peer,'replacement-invitation');
  await button(peer,1,'accept-replacement',750,580);
  game.check('replacement accepts the currently displayed guild',(await members()).some(r=>r[0]==='GuildBeta'&&r[1]==='Gamma Guild'));
  await refreshRegister(peer);
  await button(peer,1,'open-gamma-renounce',700,580);await button(peer,1,'confirm-gamma-renounce',475,340);
  await chat(game,0,'/invite GuildBeta');
  for(const duration_ms of [10000,10000,1500])await peer.act({keys:[],duration_ms});
  await capture(peer,'invitation-panel-expired');
  game.check('panel timeout does not automatically join',!(await members()).some(r=>r[0]==='GuildBeta'));
  await chat(peer,1,'/join Alpha Guild');
  game.check('ordinary join still accepts after the local20-second panel timeout',(await members()).some(r=>r[0]==='GuildBeta'&&r[1]==='Alpha Guild'));
  await refreshRegister(peer);
  await button(peer,1,'open-final-renounce',700,580);await button(peer,1,'confirm-final-renounce',475,340);
  await chat(game,0,'/invite GuildBeta');
  await refreshRegister(game);
  await button(game,0,'open-inviter-renounce',700,580);await button(game,0,'confirm-inviter-renounce',475,340);
  const start=clients[1].packets.length;
  await button(peer,1,'accept-departed-inviter',750,580);
  game.check('departed inviter cannot authorize acceptance or resurrect the deleted guild',(await members()).length===1&&clients[1].packets.slice(start).some(p=>p.text==='You are not in a guild.'));
  await capture(peer,'departed-inviter-refusal');
  await chat(other,2,'/invite GuildBeta');await button(peer,1,'accept-reinvitation',750,580);
  game.check('ordinary reinvitation restores only the remaining guild relation',(await members()).some(r=>r[0]==='GuildBeta'&&r[1]==='Gamma Guild'));
  for(const [player,index] of [[other,2],[peer,1]]){
    await walk(player,78.5,42.5);await walk(player,78.5,77.5);await walk(player,76.5,77.5);
    await button(player,index,'enter-guild-hall',700,580);
    game.check('ordinary portal admits current guild member '+index,(await player.observe()).world==='Guild Hall');
    await walk(player,14.5,22.5);await walk(player,14.5,7.5);
  }
  await button(other,2,'open-chronicle',700,580);await capture(other,'lower-rank-removal');
  await button(other,2,'open-removal-cancel',746,158);await button(other,2,'cancel-removal',325,340);
  game.check('cancelled removal preserves recipient membership',(await members()).some(r=>r[0]==='GuildBeta'&&r[1]==='Gamma Guild'));
  await button(other,2,'open-removal',746,158);await button(other,2,'confirm-removal',475,340);
  game.check('confirmed lower-rank removal commits and revokes occupied Hall access',!(await members()).some(r=>r[0]==='GuildBeta')&&(await peer.observe()).world==='Nexus');
  await capture(peer,'removed-to-nexus');
  await chat(peer,1,'/tutorial');
  game.check('ordinary reconnect separates recipient from inviters',(await peer.observe()).world==='Tutorial');
  await button(other,2,'close-chronicle',400,550);
  await chat(other,2,'/invite GuildBeta');
  game.check('cross-world invitation is refused',has(2,'Player is not nearby.'));
  await peer.act({keys:['Insert'],duration_ms:1000});await peer.act({keys:[],duration_ms:1500});
  game.check('reconnect retains removed membership and living identity',(await peer.observe()).world==='Nexus'&&(await peer.observe()).lifecycle.alive&&!(await members()).some(r=>r[0]==='GuildBeta'));
  await save('final');
}
