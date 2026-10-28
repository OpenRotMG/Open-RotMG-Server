import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {setTimeout as wait} from 'node:timers/promises';
import {connect} from '../../scripts/record-demo.mjs';
const out=()=>process.argv[process.argv.indexOf('--run-dir')+1];
export const fixture={class:0x300,level:20,seed:902,world:-2,position:{x:88.5,y:58.5},equipment:[-1,-1,-1,-1],backpack:Array(8).fill(0x1071),stats:null};
export async function enter({account,browser,signal}){
  const targets=await (await fetch(`http://127.0.0.1:${browser.port}/json/list`,{signal})).json();
  let registered=false,verified=false;
  const page=await connect({...browser,url:targets.find(p=>p.id===browser.target).url,helper:join(process.env.PI_BROWSER_TOOLS,'direct-cdp.mjs'),onEvent:e=>{if(e.method==='Network.responseReceived'){const url=e.params.response.url;if(url.endsWith('/account/register'))registered=true;if(url.endsWith('/account/verify'))verified=true;}}});
  try{
    await page.call('Network.enable');
    await page.eval('(()=>{const m=document.querySelector("ruffle-player").shadowRoot.querySelector("#hardware-acceleration-modal");if(m&&!m.classList.contains("hidden"))m.querySelector(".close-modal").click();})()');
    const r=await page.eval('(()=>{const r=document.querySelector("ruffle-player").getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height};})()');
    assert.equal(r.width,800);assert.equal(r.height,600);
    await page.call('Emulation.setFocusEmulationEnabled',{enabled:true});
    async function click(x,y){const p={x:r.x+x,y:r.y+y,button:'left',clickCount:1};try{await page.call('Input.dispatchMouseEvent',{type:'mousePressed',buttons:1,...p});}finally{await page.call('Input.dispatchMouseEvent',{type:'mouseReleased',buttons:0,...p});}await wait(200,undefined,{signal});}
    async function tab(){try{await page.call('Input.dispatchKeyEvent',{type:'keyDown',key:'Tab',code:'Tab',windowsVirtualKeyCode:9});}finally{await page.call('Input.dispatchKeyEvent',{type:'keyUp',key:'Tab',code:'Tab',windowsVirtualKeyCode:9});}}
    await click(400,550);
    // Registration must precede BUY in the selected client: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Rotmg123.5.1/src/com/company/assembleegameclient/objects/SellableObject.as
    await click(700,12);
    await page.call('Input.insertText',{text:`merchant${account}@example.test`});await tab();
    await page.call('Input.insertText',{text:'local-merchant-fixture'});await tab();
    await page.call('Input.insertText',{text:'local-merchant-fixture'});
    await click(285,395);await click(490,470);
    const deadline=Date.now()+15000;
    while(!registered||!verified){assert(Date.now()<deadline,'Native registration/verification response missing');await wait(100,undefined,{signal});}
    execFileSync('python3',['-c',`import sqlite3,sys
with sqlite3.connect(sys.argv[1]) as db:
 assert db.execute('SELECT count(*) FROM credentials WHERE account=?',(int(sys.argv[2]),)).fetchone()[0]==1,'Native registration did not finish'
 assert db.execute('UPDATE account_fame SET balance=200,earned=200 WHERE account=? AND balance=0 AND earned=0',(int(sys.argv[2]),)).rowcount==1
`,join(out(),'state/game.db'),String(account)],{signal});
    await writeFile(join(out(),'prepared-wallet.json'),JSON.stringify({account,fame:200,earned_fame:200,prepared:true,earned_gameplay:false}));
    await wait(200,undefined,{signal});await click(485,385);
    for(let n=0;n<2;n++)await click(400,550);
  }finally{page.close();}
}
export async function run(game){
  const snapshots=[];
  async function snapshot(name){const s=await game.inspect();snapshots.push({name,state:s});await writeFile(join(out(),'purchases.json'),JSON.stringify(snapshots,null,2));return s;}
  const stat=(e,id)=>{const pair=e.extra_stats.find(([key])=>key===id);assert(pair,`Merchant lacks stat ${id}`);return pair[1];};
  const offer=s=>s.diagnostic.entities.find(e=>e.pos.x===88.5&&e.pos.y===58.5);
  const messages=async()=> (await readFile(join(out(),'server.log'),'utf8')).split('\n').filter(s=>s.startsWith('{')).map(s=>JSON.parse(s).fields);
  const before=await snapshot('before');
  await game.act({keys:[],duration_ms:2000});
  await game.act({keys:[],duration_ms:1000},{name:'merchant-before',when:()=>true});
  await game.act({keys:[],mouse:{x:700,y:575},duration_ms:1000});
  await game.act({keys:[],duration_ms:800},{name:'inventory-full',when:()=>true});
  const full=await snapshot('full-refused');
  game.check('full inventory purchase retains items revision and stock',(await messages()).some(f=>f.message==='purchase rejected'&&f.error==='Not enough inventory space.')&&full.view.character.inventory_revision===before.view.character.inventory_revision&&JSON.stringify(full.view.character.items)===JSON.stringify(before.view.character.items)&&stat(offer(full),42)===stat(offer(before),42));
  await game.act({keys:['1'],duration_ms:500});
  const available=await snapshot('slot-freed');
  game.check('native dye use frees exactly one backpack slot',available.view.character.items[4]===-1&&available.view.character.inventory_revision===before.view.character.inventory_revision+1);
  await game.act({keys:[],mouse:{x:700,y:575},duration_ms:1000});
  await game.act({keys:[],duration_ms:800},{name:'purchase-delivered',when:v=>v.character.items[4]===4894});
  const bought=await snapshot('bought');
  game.check('purchase delivers selected item once and decrements stock once',bought.view.character.items[4]===4894&&bought.view.character.inventory_revision===available.view.character.inventory_revision+1&&stat(offer(bought),42)===stat(offer(before),42)-1);
  await game.act({keys:['2'],duration_ms:500});
  const spent=await snapshot('second-slot-freed');
  await game.act({keys:[],mouse:{x:700,y:575},duration_ms:1000});
  await game.act({keys:[],duration_ms:800},{name:'insufficient-gold',when:()=>true});
  const refused=await snapshot('funds-refused');
  game.check('insufficient funds retain empty slot revision and stock',(await messages()).some(f=>f.message==='purchase declined: insufficient funds'&&f.price===90&&f.currency===0)&&refused.view.character.items[5]===-1&&refused.view.character.inventory_revision===spent.view.character.inventory_revision&&stat(offer(refused),42)===stat(offer(bought),42));
  async function walk(p){for(let n=0;n<60;n++){const v=await game.observe(),dx=p.x-v.position.x,dy=p.y-v.position.y,d=Math.hypot(dx,dy);if(d<.4)return;const x=dx-dy,y=dx+dy,keys=[];if(Math.abs(x)>.1)keys.push(x>0?'d':'a');if(Math.abs(y)>.1)keys.push(y>0?'s':'w');await game.act({keys,duration_ms:Math.min(250,Math.max(50,Math.round(d*80)))});}assert.fail('Merchant walk failed: '+JSON.stringify({target:p,actual:(await game.observe()).position}));}
  await walk({x:88.5,y:63.5});
  await walk({x:84.5,y:63.5});
  const fameBefore=await snapshot('fame-before'),egg=fameBefore.diagnostic.entities.find(e=>e.pos.x===84.5&&e.pos.y===63.5);assert(egg);
  await game.act({keys:[],mouse:{x:700,y:575},duration_ms:1000});
  const fameBought=await snapshot('fame-bought');
  game.check('prepared Fame purchases an egg once with one stock decrement',fameBought.view.character.items[5]===2801&&fameBought.view.character.inventory_revision===fameBefore.view.character.inventory_revision+1&&stat(fameBought.diagnostic.entities.find(e=>e.id===egg.id),42)===stat(egg,42)-1);
  await game.act({keys:[],duration_ms:800},{name:'fame-purchase-delivered',when:()=>true});
  await game.act({keys:['3'],duration_ms:500});
  for(const [position,name,error] of [[{x:84.5,y:63.5},'insufficient-fame','purchase declined: insufficient funds'],[{x:88.5,y:67.5},'insufficient-rank','purchase rejected']]){
    await walk(position);const start=await snapshot(name+'-before'),seller=start.diagnostic.entities.find(e=>e.pos.x===position.x&&e.pos.y===position.y);assert(seller);
    const offset=(await messages()).length;
    await game.act({keys:[],mouse:{x:700,y:575},duration_ms:1000});
    await game.act({keys:[],duration_ms:800},{name,when:()=>true});
    const after=await snapshot(name),response=(await messages()).slice(offset).find(f=>f.message===error&&f.merchant===seller.id);
    game.check(name+' is refused without changing inventory revision or stock',(name==='insufficient-fame'?Boolean(response)&&response.currency===1:!response&&stat(seller,45)===7)&&JSON.stringify(after.view.character.items)===JSON.stringify(start.view.character.items)&&after.view.character.inventory_revision===start.view.character.inventory_revision&&stat(after.diagnostic.entities.find(e=>e.id===seller.id),42)===stat(seller,42));
  }
  for(const position of [{x:78.5,y:67.5},{x:78.5,y:71.5},{x:79.5,y:77.5}])await walk(position);
  await game.act({keys:[],mouse:{x:700,y:575},duration_ms:700});
  for(let n=0;n<8&&(await game.observe()).world!=='Vault';n++)await game.act({keys:[],duration_ms:500});
  const vault=await snapshot('vault-reconnect');
  game.check('ordinary Vault entry reloads the same purchased inventory',vault.view.map.name==='Vault'&&vault.view.session.character===before.view.session.character&&vault.view.character.items[4]===4894&&vault.view.character.items[5]===2801);
  await game.act({keys:['Insert'],duration_ms:1000});
  const retained=await snapshot('reconnected');
  game.check('normal Nexus reconnect preserves purchased item and living identity',retained.view.map.name==='Nexus'&&retained.view.session.character===before.view.session.character&&retained.view.lifecycle.alive&&retained.view.character.items[4]===4894&&retained.view.character.items[5]===2801&&retained.view.character.items[6]===-1);
  const saved=JSON.parse(execFileSync('python3',['-c',`import json,sqlite3,sys
with sqlite3.connect(sys.argv[1]) as db:
 a=db.execute('SELECT credits FROM accounts WHERE id=?',(int(sys.argv[2]),)).fetchone()
 f=db.execute('SELECT balance,earned FROM account_fame WHERE account=?',(int(sys.argv[2]),)).fetchone()
 c=db.execute('SELECT items,revision FROM characters WHERE id=? AND account=?',(int(sys.argv[3]),int(sys.argv[2]))).fetchone()
 print(json.dumps({'credits':a[0],'fame':f[0],'earned_fame':f[1],'items':json.loads(c[0]),'revision':c[1]}))`,join(out(),'state/game.db'),String(retained.view.session.account),String(retained.view.session.character)],{encoding:'utf8'}));
  await writeFile(join(out(),'durable-purchase.json'),JSON.stringify(saved,null,2));
  game.check('durable Gold and Fame debits and saved item revision agree',saved.credits===10&&saved.fame===20&&saved.earned_fame===200&&JSON.stringify(saved.items)===JSON.stringify(retained.view.character.items)&&saved.revision===retained.view.character.inventory_revision);
}
