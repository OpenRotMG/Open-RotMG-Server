import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {execFileSync} from 'node:child_process';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
import {setTimeout as delay} from 'node:timers/promises';
import {connect} from '../../scripts/record-demo.mjs';

export const fixture={class:0x30e,level:20,seed:916,world:-2,position:{x:76.5,y:76.5},equipment:[-1,-1,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,50,25,50,50,40,50]};
const root=()=>join(process.env.QA_RESULTS_DIR,'native');
export async function enter({account,browser,signal}){
  const targets=await(await fetch(`http://127.0.0.1:${browser.port}/json/list`,{signal})).json();let registered=false,verified=false;
  const page=await connect({...browser,url:targets.find(t=>t.id===browser.target).url,helper:join(process.env.PI_BROWSER_TOOLS,'direct-cdp.mjs'),onEvent:e=>{if(e.method==='Network.responseReceived'){if(e.params.response.url.endsWith('/account/register'))registered=true;if(e.params.response.url.endsWith('/account/verify'))verified=true;}}});
  try{
    await page.call('Network.enable');
    await page.eval('(()=>{const m=document.querySelector("ruffle-player").shadowRoot.querySelector("#hardware-acceleration-modal");if(m&&!m.classList.contains("hidden"))m.querySelector(".close-modal").click();})()');
    const r=await page.eval('(()=>{const r=document.querySelector("ruffle-player").getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height};})()');assert.equal(r.width,800);assert.equal(r.height,600);
    await page.call('Emulation.setFocusEmulationEnabled',{enabled:true});
    async function click(x,y){const p={x:r.x+x,y:r.y+y,button:'left',clickCount:1};try{await page.call('Input.dispatchMouseEvent',{type:'mousePressed',buttons:1,...p});}finally{await page.call('Input.dispatchMouseEvent',{type:'mouseReleased',buttons:0,...p});}await delay(200,undefined,{signal});}
    async function tab(){for(const type of ['keyDown','keyUp'])await page.call('Input.dispatchKeyEvent',{type,key:'Tab',code:'Tab',windowsVirtualKeyCode:9});}
    await click(400,550);await click(700,12);
    await page.call('Input.insertText',{text:`hall${account}@example.test`});await tab();
    await page.call('Input.insertText',{text:'local-hall-fixture'});await tab();await page.call('Input.insertText',{text:'local-hall-fixture'});
    await click(285,395);await click(490,470);
    const deadline=Date.now()+15000;while(!registered||!verified){assert(Date.now()<deadline,'Native registration responses missing');await delay(100,undefined,{signal});}
    await delay(200,undefined,{signal});await click(485,385);await click(400,550);await click(400,550);
  }finally{page.close();}
}
export async function run(game){
  const config=JSON.parse(await readFile(join(root(),'native.json'),'utf8')),token=(await readFile(join(config.state,'control.token'),'utf8')).trim(),records=[];
  const api=async command=>{const r=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command}),signal:AbortSignal.timeout(10000)});const s=await r.json();assert(r.ok&&s.status==='succeeded',JSON.stringify(s));return s;};
  const db=(sql,args=[])=>JSON.parse(execFileSync('python3',['-c',`import json,sqlite3,sys
with sqlite3.connect(sys.argv[1]) as db:
 print(json.dumps(db.execute(sys.argv[2],json.loads(sys.argv[3])).fetchall()))`,config.state+'/game.db',sql,JSON.stringify(args)],{encoding:'utf8'}));
  async function save(label,g=game){const s=await g.inspect();records.push({label,...s});await writeFile(join(process.env.QA_RESULTS_DIR,'guild-hall.json'),JSON.stringify(records,null,2));return s;}
  async function capture(g,name){await g.act({keys:[],duration_ms:500});await g.act({keys:[],duration_ms:800},{name,when:()=>true});}
  async function walk(g,p){for(let n=0;n<100;n++){const v=await g.observe(),dx=p.x-v.position.x,dy=p.y-v.position.y,d=Math.hypot(dx,dy);if(d<.9)return;const x=dx-dy,y=dx+dy,keys=[];if(Math.abs(x)>.12)keys.push(x>0?'d':'a');if(Math.abs(y)>.12)keys.push(y>0?'s':'w');await g.act({keys,duration_ms:Math.min(350,Math.max(150,Math.round(d*100)))});}assert.fail('Native hall walk failed '+JSON.stringify({target:p,actual:(await g.observe()).position}));}
  async function portal(g,kind,world){const before=await g.observe(),e=(await g.inspect()).diagnostic.entities.find(e=>e.kind===kind);assert(e,'Portal not visible '+kind);await walk(g,e.pos);await g.act({keys:[],mouse:{x:700,y:575},duration_ms:700});for(let i=0;i<10&&(await g.observe()).session.world===before.session.world;i++)await g.act({keys:[],duration_ms:500});assert.equal((await g.observe()).world,world);}
  async function nexus(g){await g.act({keys:['Insert'],duration_ms:700});for(let i=0;i<10&&(await g.observe()).world!=='Nexus';i++)await g.act({keys:[],duration_ms:500});assert.equal((await g.observe()).world,'Nexus');}
  const peer=await game.join({...fixture,seed:917}),outsider=await game.join({...fixture,seed:918});
  const initial=await save('initial'),peerInitial=await save('peer-initial',peer);
  await game.act({keys:[],duration_ms:1000});
  await walk(outsider,{x:76.5,y:77.5});await outsider.act({keys:[],mouse:{x:700,y:575},duration_ms:700});
  await capture(outsider,'hall-nonmember-refused');
  game.check('nonmember portal refuses entry without reconnect',(await outsider.observe()).world==='Nexus');
  await api({op:'cheat',actor:game.actor,change:{kind:'guild',name:'Hall Research',members:[peer.actor]}});await game.act({keys:[],duration_ms:1000});
  await portal(game,0x72f,'Guild Hall');const first=await save('founder-arrived');
  await portal(peer,0x72f,'Guild Hall');await save('peer-arrived',peer);await game.act({keys:[],duration_ms:700});
  const shared=await save('shared'),other=await save('shared-peer',peer);
  game.check('members enter one shared hall and see each other',shared.view.session.world===other.view.session.world&&shared.view.player_markers.some(p=>p.account===other.view.session.account)&&other.view.player_markers.some(p=>p.account===shared.view.session.account));
  await capture(game,'hall-shared-founder');await capture(peer,'hall-shared-peer');
  await walk(game,{x:14.5,y:7.5});await game.act({keys:[],mouse:{x:700,y:575},duration_ms:700});await capture(game,'hall-chronicle');
  await game.act({keys:[],mouse:{x:400,y:540},duration_ms:300});
  await walk(game,{x:19.5,y:8.5});await game.act({keys:[],mouse:{x:700,y:575},duration_ms:700});await capture(game,'hall-board');
  await game.act({keys:[],mouse:{x:540,y:510},duration_ms:300});
  game.check('viewing and dismissing hall facilities does not change membership or Board',db('SELECT count(*) FROM guild_members')[0][0]===2&&db('SELECT board FROM guilds')[0][0]==='');
  await walk(game,{x:16.5,y:7.5});await walk(peer,{x:16.5,y:7.5});
  const before=db('SELECT level,fame,earned FROM guilds');assert.deepEqual(before,[[1,0,0]]);
  await peer.act({keys:[],mouse:{x:700,y:575},duration_ms:700});await capture(peer,'hall-initiate-upgrade-refused');
  game.check('Initiate upgrade click does not charge or upgrade',JSON.stringify(db('SELECT level,fame,earned FROM guilds'))===JSON.stringify(before));
  await game.act({keys:[],mouse:{x:700,y:575},duration_ms:700});await capture(game,'hall-insufficient-fame');
  game.check('Founder with insufficient Guild Fame is refused atomically',JSON.stringify(db('SELECT level,fame,earned FROM guilds'))===JSON.stringify(before));
  assert.equal(db('UPDATE guilds SET fame=360000,earned=360000 WHERE level=1 AND fame=0 AND earned=0 RETURNING id').length,1);
  await writeFile(join(process.env.QA_RESULTS_DIR,'prepared-guild-fame.json'),JSON.stringify({prepared:true,earned_gameplay:false,guild_fame:360000,personal_fame:0,reason:'Bounded native upgrade purchase, not earned progression'}));
  await game.act({keys:[],mouse:{x:700,y:575},duration_ms:700});await capture(game,'hall-upgrade-purchased');
  game.check('native Founder purchase spends exactly 10000 Guild Fame',JSON.stringify(db('SELECT level,fame,earned FROM guilds'))===JSON.stringify([[2,350000,360000]])&&db('SELECT balance FROM account_fame').every(([f])=>f===0));
  await game.act({keys:[],mouse:{x:700,y:575},duration_ms:700});await capture(game,'hall-stale-upgrade-refused');
  game.check('repeated purchase cannot charge the old merchant twice',JSON.stringify(db('SELECT level,fame FROM guilds'))===JSON.stringify([[2,350000]]));
  const upgraded=await save('occupied-after-upgrade');game.check('occupied hall is not rebuilt',upgraded.view.session.world===first.view.session.world&&upgraded.diagnostic.entities.some(e=>e.kind===0x736));
  await nexus(game);await walk(game,{x:76.5,y:77.5});await portal(game,0x72f,'Guild Hall');
  game.check('reconnect while peer occupies hall reuses the old level',(await game.observe()).session.world===first.view.session.world);
  await nexus(game);await nexus(peer);
  for(let i=0;i<31;i++)await game.act({keys:[],duration_ms:10000});
  await walk(game,{x:76.5,y:77.5});await portal(game,0x72f,'Guild Hall');const rebuilt=await save('upgraded-hall-reentered');
  game.check('empty idle hall is replaced with durable purchased level',rebuilt.view.session.world!==first.view.session.world&&rebuilt.view.map.width===53&&rebuilt.view.map.height===47);
  await capture(game,'hall-upgraded-layout');
  await walk(peer,{x:76.5,y:77.5});await portal(peer,0x72f,'Guild Hall');
  game.check('second member reconnects to the same upgraded hall',(await peer.observe()).session.world===rebuilt.view.session.world);
  await walk(game,{x:25.5,y:8.5});await game.act({keys:[],mouse:{x:700,y:575},duration_ms:700});await capture(game,'hall-upgraded-chronicle');
  await game.act({keys:[],mouse:{x:748,y:158},duration_ms:500});await capture(game,'hall-remove-confirmation');
  await game.act({keys:[],mouse:{x:325,y:338},duration_ms:500});
  game.check('cancelled removal retains membership and both hall occupants',db('SELECT count(*) FROM guild_members')[0][0]===2&&(await peer.observe()).session.world===rebuilt.view.session.world);
  await game.act({keys:[],mouse:{x:748,y:158},duration_ms:500});await game.act({keys:[],mouse:{x:475,y:338},duration_ms:700});
  for(let i=0;i<10&&(await peer.observe()).world!=='Nexus';i++)await peer.act({keys:[],duration_ms:500});
  const removed=await save('removed-member',peer);
  game.check('confirmed removal durably revokes occupied hall access',db('SELECT count(*) FROM guild_members')[0][0]===1&&removed.view.map.name==='Nexus'&&(await game.observe()).session.world===rebuilt.view.session.world);
  game.check('forced return preserves the removed member living identity and inventory',removed.view.lifecycle.alive&&removed.view.session.character===peerInitial.view.session.character&&JSON.stringify(removed.view.character.items)===JSON.stringify(peerInitial.view.character.items));
  await capture(peer,'hall-removed-to-nexus');await game.act({keys:[],mouse:{x:400,y:540},duration_ms:300});
  await walk(peer,{x:76.5,y:77.5});await peer.act({keys:[],mouse:{x:700,y:575},duration_ms:700});
  game.check('removed member cannot reenter the hall',(await peer.observe()).world==='Nexus');
  const final=await save('final');game.check('hall journey preserves living identity and inventory',final.view.lifecycle.alive&&final.view.session.character===initial.view.session.character&&JSON.stringify(final.view.character.items)===JSON.stringify(initial.view.character.items));
}
