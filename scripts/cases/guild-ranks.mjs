import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {randomUUID} from 'node:crypto';
import {setTimeout as delay} from 'node:timers/promises';
import {connect} from '../../scripts/record-demo.mjs';

export const fixture={class:0x30e,level:5,seed:913,world:-2,position:{x:76.5,y:76.5},equipment:[-1,-1,-1,-1],backpack:Array(8).fill(-1),stats:null};
const clients=[],rosters=[],exec=promisify(execFile),out=process.env.QA_RESULTS_DIR,root=join(out,'native');
export async function beforeNavigate({browser,signal}){
  const index=clients.length,requests=new Set();rosters.push(0);
  const c=await connect({...browser,url:'about:blank',helper:join(process.env.PI_BROWSER_TOOLS,'direct-cdp.mjs'),onEvent:e=>{
    if(e.method==='Network.responseReceived'&&e.params.response.url.endsWith('/guild/listMembers')&&e.params.response.status===200)requests.add(e.params.requestId);
    if(e.method==='Network.loadingFinished'&&requests.delete(e.params.requestId))rosters[index]++;
  }});
  await c.call('Network.enable');await c.call('Emulation.setFocusEmulationEnabled',{enabled:true});signal.addEventListener('abort',()=>c.close(),{once:true});clients.push(c);
}
async function rows(){
  const {stdout}=await exec('python3',['-c',"import sqlite3,json,sys; c=sqlite3.connect('file:'+sys.argv[1]+'?mode=ro',uri=True); print(json.dumps(c.execute('SELECT a.id,a.name,m.rank FROM guild_members m JOIN accounts a ON a.id=m.account ORDER BY m.rank DESC,m.fame DESC,a.name COLLATE NOCASE').fetchall()))",join(root,'state/game.db')]);
  return JSON.parse(stdout);
}
async function click(index,x,y){
  const c=clients[index],r=await c.eval('(()=>{const r=document.querySelector("ruffle-player").getBoundingClientRect();return{x:r.x,y:r.y};})()'),p={x:r.x+x,y:r.y+y,button:'left',clickCount:1};
  await c.call('Input.dispatchMouseEvent',{type:'mouseMoved',...p});
  try{await c.call('Input.dispatchMouseEvent',{type:'mousePressed',buttons:1,...p});}finally{await c.call('Input.dispatchMouseEvent',{type:'mouseReleased',buttons:0,...p});}
}
export async function run(game){
  const peer=await game.join({...fixture,seed:914}),target=await game.join({...fixture,seed:915}),players=[game,peer,target];
  const config=JSON.parse(await readFile(join(root,'native.json'),'utf8')),token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:game.actor,change:{kind:'guild',name:'Rank Research',members:[peer.actor,target.actor]}}})});
  const prepared=await response.json();assert(response.ok&&prepared.status==='succeeded',JSON.stringify(prepared));
  const names=await Promise.all(players.map(async p=>(await p.inspect()).diagnostic.character.name)),records=[];
  async function record(label){records.push({label,rosters:[...rosters],members:await rows(),states:await Promise.all(players.map(p=>p.observe()))});await writeFile(join(out,'guild-ranks-observations.json'),JSON.stringify({preparedGuild:true,records},null,2));}
  async function ui(index,name,work,capture=true){
    const log=join(root,'controller.stderr'),offset=(await readFile(log,'utf8')).length;let done=false;
    const action=players[index].act({keys:[],duration_ms:2500},capture?{name,when:()=>done}:undefined);
    const input=(async()=>{
      const deadline=Date.now()+10000;
      while(true){
        const lines=(await readFile(log,'utf8')).slice(offset).split('\n').slice(0,-1),acks=new Set(lines.filter(l=>l.startsWith('{"event":"native_sync_verified_ack"')).map(l=>JSON.parse(l).context.actor));
        if(config.native.every(c=>acks.has(c.actor)))break;
        assert(Date.now()<deadline,'UI requires current cohort resume ACKs');await delay(20);
      }
      await work();await delay(350);done=true;
    })();
    const results=await Promise.allSettled([action,input]);for(const r of results)if(r.status==='rejected')throw r.reason;
    await record(name);
  }
  async function walk(index,point){
    for(let i=0;i<100;i++){
      const v=await players[index].observe(),dx=point.x-v.position.x,dy=point.y-v.position.y,d=Math.hypot(dx,dy);if(d<.2)return;
      const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,keys=[];if(Math.abs(x)>.1)keys.push(x>0?'d':'a');if(Math.abs(y)>.1)keys.push(y>0?'s':'w');
      await players[index].act({keys,duration_ms:Math.min(350,Math.max(50,Math.round(d*100)))});
    }
    assert.fail('Native walk failed: '+JSON.stringify({index,point,view:await players[index].observe()}));
  }
  async function enter(index){
    await walk(index,{x:76.5,y:77.5});
    await ui(index,'hall-entry-'+index,()=>click(index,700,575),false);
    game.check('ordinary Guild Hall admission '+index,(await players[index].observe()).world==='Guild Hall');
    await walk(index,{x:14.5,y:8.2});
  }
  const open=async(index,name)=>{const before=rosters[index];await ui(index,name,()=>click(index,700,575));game.check('Chronicle roster completed: '+name,rosters[index]>before);};
  const close=index=>ui(index,'close-roster-'+index,()=>click(index,400,545));
  async function change(index,targetIndex,direction,rank,cancel=false){
    const before=await rows(),row=before.findIndex(r=>r[1]===names[targetIndex]);assert(row>=0);
    await ui(index,'confirm-'+index+'-'+targetIndex+'-'+direction+'-'+rank,()=>click(index,direction==='promote'?686:716,126+row*32));
    await ui(index,(cancel?'cancel':'accept')+'-'+index+'-'+targetIndex+'-'+direction+'-'+rank,()=>click(index,cancel?325:475,338));
    const after=await rows();
    game.check(cancel?'cancelled promotion preserves the roster':names[index]+' '+direction+' '+names[targetIndex]+' to '+rank,cancel?JSON.stringify(before)===JSON.stringify(after):after.find(r=>r[1]===names[targetIndex])?.[2]===rank);
  }
  await game.act({keys:[],duration_ms:1000},{name:'prepared-guild-before',when:()=>true});
  for(let i=0;i<3;i++)await enter(i);
  await open(0,'founder-roster');
  await change(0,1,'promote',10,true);
  await change(0,1,'promote',10);await change(0,1,'promote',20);await change(0,2,'promote',10);
  await close(0);await open(2,'member-no-controls');const member=await rows();await ui(2,'member-cannot-manage',()=>click(2,686,126));
  game.check('Member has no Founder promotion control',JSON.stringify(member)===JSON.stringify(await rows()));await close(2);
  await open(1,'officer-permissions');const officer=await rows(),memberRow=officer.findIndex(r=>r[1]===names[2]);await ui(1,'officer-cannot-appoint-officer',()=>click(1,686,126+memberRow*32));
  game.check('Officer cannot promote Member to Officer',JSON.stringify(officer)===JSON.stringify(await rows()));
  await change(1,2,'demote',0);await change(1,2,'promote',10);
  await close(1);await open(0,'founder-appoints-leader');await change(0,1,'promote',30);await close(0);
  await open(1,'leader-permissions');await change(1,2,'promote',20);await change(1,2,'promote',30);
  const leaders=await rows(),coLeaderRow=leaders.findIndex(r=>r[1]===names[2]);await ui(1,'co-leader-no-controls',()=>click(1,716,126+coLeaderRow*32));
  game.check('clicking absent co-Leader demotion does not alter ranks',JSON.stringify(leaders)===JSON.stringify(await rows()));
  await close(1);await open(0,'founder-manages-leaders');await change(0,2,'demote',20);await change(0,2,'demote',10);await change(0,2,'demote',0);
  await close(0);await open(2,'initiate-no-controls');const lowest=await rows();await ui(2,'initiate-cannot-manage',()=>click(2,686,126));
  game.check('Initiate has no Founder promotion control',JSON.stringify(lowest)===JSON.stringify(await rows()));await close(2);
  await open(0,'founder-restores-leader');for(const rank of [10,20,30])await change(0,2,'promote',rank);await close(0);
  const original=await game.observe();await game.act({keys:['Insert'],duration_ms:700});await game.act({keys:[],duration_ms:500});
  game.check('ordinary Nexus reconnect keeps Founder and living character',(await game.observe()).world==='Nexus'&&(await game.observe()).session.character===original.session.character&&(await rows()).find(r=>r[1]===names[0])[2]===40);
  await walk(0,{x:78.5,y:41.5});await walk(0,{x:109.5,y:42.2});await ui(0,'renounce-dialog',()=>click(0,700,575));await ui(0,'renounce-cancel',()=>click(0,325,338));
  game.check('cancelled renunciation preserves Founder',(await rows()).find(r=>r[1]===names[0])[2]===40);
  await ui(0,'renounce-confirm',()=>click(0,700,575));await ui(0,'founder-renounced',()=>click(0,475,338));
  const remaining=await rows();game.check('Founder departure leaves both Leaders without automatic succession',remaining.length===2&&remaining.every(r=>r[2]===30));
  await open(1,'founderless-leader-roster');await close(1);
  const alive=await peer.observe();await peer.act({keys:['Insert'],duration_ms:700});await peer.act({keys:[],duration_ms:500});await walk(1,{x:76.5,y:76.5});await enter(1);await open(1,'founderless-after-reconnect');
  game.check('founderless guild and Leader rank survive normal reconnect',(await peer.observe()).session.character===alive.session.character&&JSON.stringify(remaining)===JSON.stringify(await rows()));
  await close(1);await record('final');game.check('all three ordinary clients remain alive',(await Promise.all(players.map(p=>p.observe()))).every(v=>v.lifecycle.alive));
}
