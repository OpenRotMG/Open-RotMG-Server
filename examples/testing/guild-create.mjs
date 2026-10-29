import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {setTimeout as delay} from 'node:timers/promises';
import {connect} from '../../scripts/record-demo.mjs';

const exec=promisify(execFile),clients=[],root=join(process.env.QA_RESULTS_DIR,'native');
export const fixture={class:0x30e,level:1,seed:911,world:-2,position:{x:73.9,y:74.5},equipment:[0xa97,0xa2e,-1,-1],backpack:Array(8).fill(-1),stats:null};
async function db(sql,params=[]) {
  const {stdout}=await exec('python3',['-c','import sqlite3,json,sys; c=sqlite3.connect(sys.argv[1]); r=c.execute(sys.argv[2],json.loads(sys.argv[3])); print(json.dumps(r.fetchall())); c.commit()',join(root,'state/game.db'),sql,JSON.stringify(params)]);
  return JSON.parse(stdout);
}
export async function beforeNavigate({browser,signal}) {
  const c=await connect({...browser,url:'about:blank',helper:join(process.env.PI_BROWSER_TOOLS,'direct-cdp.mjs')});
  await c.call('Emulation.setFocusEmulationEnabled',{enabled:true});
  signal.addEventListener('abort',()=>c.close(),{once:true});clients.push(c);
}
async function rejected(message) {
  assert((await readFile(join(root,'state/server.log'),'utf8')).includes(message),'Expected server refusal: '+message);
}
async function click(c,x,y) {
  const r=await c.eval('(()=>{const r=document.querySelector("ruffle-player").getBoundingClientRect();return{x:r.x,y:r.y};})()');
  const p={x:r.x+x,y:r.y+y,button:'left',clickCount:1};
  await c.call('Input.dispatchMouseEvent',{type:'mouseMoved',...p});
  try {await c.call('Input.dispatchMouseEvent',{type:'mousePressed',buttons:1,...p});}
  finally {await c.call('Input.dispatchMouseEvent',{type:'mouseReleased',buttons:0,...p});}
}
async function key(c,key,code,n) {
  try {await c.call('Input.dispatchKeyEvent',{type:'keyDown',key,code,windowsVirtualKeyCode:n});}
  finally {await c.call('Input.dispatchKeyEvent',{type:'keyUp',key,code,windowsVirtualKeyCode:n});}
}
async function type(c,x,y,text) {
  await click(c,x,y);for(const letter of text)await c.call('Input.dispatchKeyEvent',{type:'char',text:letter});
}
export async function enter({account}) {
  const c=clients.at(-1),index=clients.length-1;
  await db('UPDATE account_fame SET balance=?,earned=? WHERE account=?',[index===0?3000:0,index===0?3000:0,account]);
  await db("UPDATE characters SET stats=json_set(stats,'$.fame',20) WHERE account=?",[account]);
  await c.eval('(()=>{const p=document.querySelector("ruffle-player");const m=p.shadowRoot.querySelector("#hardware-acceleration-modal");if(m&&!m.classList.contains("hidden"))m.querySelector(".close-modal").click();})()');
  await click(c,400,550);await delay(100);await click(c,400,550);
}
export async function run(game) {
  const peer=await game.join({...fixture,seed:912});
  const config=JSON.parse(await readFile(join(root,'native.json'),'utf8')),accounts=JSON.parse(await readFile(join(root,'result.json'),'utf8')).participants.map(p=>p.account);
  const headers={Authorization:'Bearer '+(await readFile(join(root,'state/control.token'),'utf8')).trim()};
  const wallet=account=>db('SELECT balance,earned FROM account_fame WHERE account=?',[account]);
  const members=()=>db('SELECT a.name,g.name,m.rank FROM guild_members m JOIN accounts a ON a.id=m.account JOIN guilds g ON g.id=m.guild ORDER BY a.id');
  const records=[];
  async function record(label) {
    records.push({label,members:await members(),wallets:await Promise.all(accounts.map(wallet)),states:await Promise.all([game.inspect(),peer.inspect()])});
    await writeFile(join(root,'guild-create.json'),JSON.stringify({fixtureLimits:'Prepared personal Fame wallets (3000/0) and living class Fame20; no earned-progression, capacity50 or historical-normalization acceptance.',records},null,2));
  }
  async function ui(player,index,name,work,capture=true) {
    const c=clients[index],log=join(root,'controller.stderr'),offset=(await readFile(log,'utf8')).length;let finished=false;
    const action=player.act({keys:[],duration_ms:5000},capture?{name,when:()=>finished}:undefined);
    const input=(async()=>{
      const deadline=Date.now()+10000;
      while(true) {
        const lines=(await readFile(log,'utf8')).slice(offset).split('\n').slice(0,-1),acks=new Set(lines.filter(l=>l.startsWith('{"event":"native_sync_verified_ack"')).map(l=>JSON.parse(l).context.actor));
        if(config.native.every(c=>acks.has(c.actor)))break;
        assert(Date.now()<deadline,'UI input requires this action cohort ACKs');await delay(20);
      }
      await work(c);await delay(500);finished=true;
    })();
    const results=await Promise.allSettled([action,input]);for(const r of results)if(r.status==='rejected')throw r.reason;
    await record(name);
  }
  async function chat(player,index,text) {
    await ui(player,index,'command-'+records.length,async c=>{await key(c,'Enter','Enter',13);await c.call('Input.insertText',{text});await key(c,'Enter','Enter',13);},false);
  }
  async function walk(player,x,y) {
    const start=await player.observe(),response=await fetch(config.origin+'/dev/map?player='+start.session.player,{headers});assert(response.ok,'Owned test map read: '+response.status);
    const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(','))),pass=(x,y)=>x>=0&&y>=0&&x<map.width&&y<map.height&&!blocked.has(x+','+y);
    const clear=(x,y)=>[-1,0,1].every(dx=>[-1,0,1].every(dy=>pass(x+dx,y+dy)));
    const goal=[Math.floor(x),Math.floor(y)],queue=[goal],distance=new Map([[goal.join(','),0]]);
    assert(clear(...goal),'Walking goal needs client corner clearance');
    await writeFile(join(root,'walk-map-'+player.actor+'.json'),JSON.stringify(map));
    for(let i=0;i<queue.length;i++) {
      const [a,b]=queue[i];
      for(const [dx,dy] of [[1,0],[-1,0],[0,1],[0,-1],[1,1],[-1,1],[1,-1],[-1,-1]]) {
        const c=a+dx,d=b+dy,k=c+','+d;if(!clear(c,d)||!pass(c,b)||!pass(a,d)||distance.has(k))continue;
        distance.set(k,distance.get(a+','+b)+1);queue.push([c,d]);
      }
    }
    for(let i=0;i<300;i++) {
      const p=(await player.observe()).position;if(Math.hypot(x-p.x,y-p.y)<.3)return;
      const a=Math.floor(p.x),b=Math.floor(p.y);let aim={x,y};
      if(a!==goal[0]||b!==goal[1]) {
        const choices=queue.filter(([c,d])=>Math.abs(c-a)<=1&&Math.abs(d-b)<=1&&pass(c,b)&&pass(a,d)).sort((c,d)=>distance.get(c.join(','))-distance.get(d.join(',')));
        assert(choices.length,'No native walking route');
        let [c,d]=choices[0];const dx=c-a,dy=d-b;
        for(let n=0;n<7;n++) {
          const e=c+dx,f=d+dy;
          if(!clear(e,f)||!pass(e,d)||!pass(c,f)||!distance.has(e+','+f)||distance.get(e+','+f)>=distance.get(c+','+d))break;
          c=e;d=f;
        }
        aim={x:c+.5,y:d+.5};
      }
      const dx=aim.x-p.x,dy=aim.y-p.y,sx=(dx-dy)/Math.SQRT2,sy=(dx+dy)/Math.SQRT2,keys=[];
      if(Math.abs(sx)>.08)keys.push(sx>0?'d':'a');if(Math.abs(sy)>.08)keys.push(sy>0?'s':'w');
      await player.act({keys,duration_ms:Math.min(1000,Math.max(50,Math.round(Math.hypot(dx,dy)*160)))});
      const moved=(await player.observe()).position;
      assert(Math.hypot(moved.x-p.x,moved.y-p.y)>.01,'Native walk stalled: '+JSON.stringify({position:p,aim,keys}));
    }
    throw Error('Native walk failed: '+JSON.stringify({x,y,position:(await player.observe()).position}));
  }
  async function approachRegister(player) {
    await player.act({keys:[],mouse:{x:300,y:300},duration_ms:150});
    await walk(player,109.5,43.2);
    await player.act({keys:['d','w'],duration_ms:250});
  }
  const initial=await Promise.all([game.observe(),peer.observe()]);
  await game.act({keys:[],duration_ms:5000});
  for(const [index,player] of [game,peer].entries()) {
    await ui(player,index,'guest-name-refused-'+index,c=>click(c,700,580));
    game.check('guest cannot choose a name before registration '+index,(await db('SELECT name FROM accounts WHERE id=?',[accounts[index]]))[0][0]===`Guest${accounts[index]}`);
    await ui(player,index,'register-dialog-'+index,c=>click(c,475,330));
    await ui(player,index,'registered-'+index,async c=>{await type(c,300,170,`guild-${index}@example.test`);await type(c,300,258,'test-password');await type(c,300,346,'test-password');await click(c,288,396);await click(c,484,467);});
    game.check('ordinary client registers the owned account '+index,(await db('SELECT count(*) FROM credentials WHERE account=?',[accounts[index]]))[0][0]===1);
    await ui(player,index,'choose-name-'+index,c=>click(c,700,580));
    await ui(player,index,'named-'+index,async c=>{await type(c,300,255,index===0?'GuildOwner':'GuildGuest');await click(c,480,380);});
    game.check('ordinary client chooses its account name '+index,(await player.inspect()).diagnostic.character.name===(index===0?'GuildOwner':'GuildGuest'));
    await player.act({keys:[],mouse:{x:300,y:300},duration_ms:200});
    await walk(player,73.9,75.5);await approachRegister(player);
  }
  await ui(game,0,'guild-register',async()=>{});
  await ui(game,0,'create-cancel-dialog',c=>click(c,700,580));
  await ui(game,0,'create-cancelled',c=>click(c,390,386));
  game.check('cancelling creation preserves membership and wallet',(await members()).length===0&&JSON.stringify(await wallet(accounts[0]))==='[[3000,3000]]');
  await approachRegister(game);
  await ui(game,0,'create-dialog',c=>click(c,700,580));
  await ui(game,0,'empty-name-refused',c=>click(c,490,386));
  await rejected('Guild names must contain 1–20 letters and spaces.');
  game.check('empty guild name refuses without spending',(await members()).length===0&&JSON.stringify(await wallet(accounts[0]))==='[[3000,3000]]');
  // The selected 123.5.1 form is authoritative for letters/spaces and maxChars20: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Rotmg123.5.1/src/_0D_d/_lx.as
  await ui(game,0,'guild-founded',async c=>{await type(c,300,260,'  Guild  Research  ');await click(c,490,386);});
  game.check('native Register creation normalizes spaces and assigns Founder',JSON.stringify(await members())==='[["GuildOwner","Guild Research",40]]');
  game.check('creation spends exactly1000 personal Fame without changing lifetime earnings',JSON.stringify(await wallet(accounts[0]))==='[[2000,3000]]');
  await approachRegister(game);
  await ui(game,0,'founder-register-rank',async()=>{});
  await ui(peer,1,'duplicate-dialog',c=>click(c,700,580));
  await ui(peer,1,'duplicate-refused',async c=>{await type(c,300,260,'guild research');await click(c,490,386);});
  await rejected('Guild name is already taken.');
  game.check('case-insensitive duplicate creation leaves the original guild and both wallets intact',(await members()).length===1&&JSON.stringify(await wallet(accounts[1]))==='[[0,0]]');
  await ui(peer,1,'duplicate-cancelled',c=>click(c,390,386));
  await approachRegister(peer);
  await ui(peer,1,'unfunded-dialog',c=>click(c,700,580));
  await ui(peer,1,'unfunded-refused',async c=>{await type(c,300,260,'Different Guild');await click(c,490,386);});
  await rejected('Creating a guild costs 1000 Fame.');
  game.check('unfunded creation cannot create membership or a second guild',(await db('SELECT count(*) FROM guilds'))[0][0]===1&&(await members()).length===1);
  await ui(peer,1,'unfunded-cancelled',c=>click(c,390,386));
  await approachRegister(peer);
  await chat(peer,1,'/join Guild Research');
  await rejected('You have not been invited to that guild.');
  game.check('joining without an invitation refuses',(await members()).length===1);
  await chat(game,0,'/invite GuildGuest');
  await ui(peer,1,'guild-invitation',async()=>{});
  await ui(peer,1,'invitation-rejected',c=>click(c,650,580));
  game.check('rejecting the native invite does not join or spend Fame',(await members()).length===1&&JSON.stringify(await wallet(accounts[1]))==='[[0,0]]');
  await chat(game,0,'/invite GuildGuest');
  await ui(peer,1,'invitation-accepted',c=>click(c,750,580));
  game.check('native Accept joins the named guild as account Initiate',JSON.stringify(await members())==='[["GuildOwner","Guild Research",40],["GuildGuest","Guild Research",0]]');
  await ui(game,0,'founder-sees-joined-member',async()=>{});
  await ui(peer,1,'initiate-register-rank',async()=>{});
  await chat(peer,1,'/invite GuildOwner');
  await rejected('Only Officers, Leaders and the Founder may invite players.');
  game.check('Initiate cannot invite or alter guild membership',(await members()).length===2);
  for(const [index,player] of [game,peer].entries()) {
    await chat(player,index,'/tutorial');await player.act({keys:['Insert'],duration_ms:1000});
    await player.act({keys:[],duration_ms:1000});
    const s=await player.observe();
    game.check('ordinary reconnect preserves living account and character '+index,s.world==='Nexus'&&s.lifecycle.alive&&s.session.account===initial[index].session.account&&s.session.character===initial[index].session.character);
  }
  game.check('reconnect preserves both memberships and exact wallets',(await members()).length===2&&JSON.stringify(await Promise.all(accounts.map(wallet)))==='[[[2000,3000]],[[0,0]]]');
  await game.act({keys:[],duration_ms:5000});
  await ui(game,0,'guild-reconnected-founder',async()=>{});await ui(peer,1,'guild-reconnected-initiate',async()=>{});
  await record('final');
}
