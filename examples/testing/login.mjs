import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {setTimeout as delay} from 'node:timers/promises';
import {connect} from '../../scripts/record-demo.mjs';

const root=join(process.env.QA_RESULTS_DIR,'native'),exec=promisify(execFile),traffic=[],requests=new Map();
let c,networkError,account,initial;
export const fixture={class:0x30e,level:1,seed:921,world:-2,position:{x:74.5,y:78.5},equipment:[0xa97,0xa2e,-1,-1],backpack:Array(8).fill(-1),stats:null};
export async function beforeNavigate({browser}) {
  c=await connect({...browser,url:'about:blank',helper:join(process.env.PI_BROWSER_TOOLS,'direct-cdp.mjs'),onEvent:e=>{
    if(e.method==='Network.requestWillBeSent'&&e.params.request.url.includes('/legacy-api/'))requests.set(e.params.requestId,new URL(e.params.request.url).pathname);
    if(e.method==='Network.loadingFinished'&&requests.has(e.params.requestId)) {
      const path=requests.get(e.params.requestId);requests.delete(e.params.requestId);
      c.call('Network.getResponseBody',{requestId:e.params.requestId}).then(r=>traffic.push({path,body:r.base64Encoded?Buffer.from(r.body,'base64').toString():r.body})).catch(error=>networkError=error);
    }
  }});
  await c.call('Network.enable');
  await c.call('Emulation.setFocusEmulationEnabled',{enabled:true});
}
async function until(predicate,label) {
  for(let i=0;i<300;i++) {if(networkError)throw networkError;if(await predicate())return;await delay(100);}
  throw Error('Native login did not reach '+label);
}
async function shot(name) {
  const image=await c.call('Page.captureScreenshot',{format:'png'});
  await writeFile(join(root,name+'.png'),Buffer.from(image.data,'base64'));
}
async function click(x,y) {
  const r=await c.eval('(()=>{const r=document.querySelector("ruffle-player").getBoundingClientRect();return{x:r.x,y:r.y};})()');
  const p={x:r.x+x,y:r.y+y,button:'left',clickCount:1};
  try {await c.call('Input.dispatchMouseEvent',{type:'mousePressed',buttons:1,...p});}
  finally {await c.call('Input.dispatchMouseEvent',{type:'mouseReleased',buttons:0,...p});}
}
async function type(x,y,text,replace=false) {
  await click(x,y);
  if(replace) {
    for(const type of ['keyDown','keyUp'])await c.call('Input.dispatchKeyEvent',{type,key:'End',code:'End',windowsVirtualKeyCode:35});
    for(let i=0;i<64;i++)for(const type of ['keyDown','keyUp'])await c.call('Input.dispatchKeyEvent',{type,key:'Backspace',code:'Backspace',windowsVirtualKeyCode:8});
  }
  for(const letter of text)await c.call('Input.dispatchKeyEvent',{type:'char',text:letter});
}
async function db(sql) {
  return JSON.parse((await exec('python3',['-c','import sqlite3,json,sys; c=sqlite3.connect(sys.argv[1]); print(json.dumps(c.execute(sys.argv[2],[int(sys.argv[3])]).fetchall()))',join(root,'state/game.db'),sql,String(account)])).stdout);
}
const snapshot=()=>db('SELECT a.id,a.name,a.credits,c.id,c.stats,c.items,c.revision,c.alive FROM accounts a JOIN characters c ON c.account=a.id WHERE a.id=?');
const replies=op=>traffic.filter(t=>t.path==='/legacy-api/'+op);
export async function enter(context) {
  account=context.account;initial=await snapshot();
  await c.eval('(()=>{const m=document.querySelector("ruffle-player").shadowRoot.querySelector("#hardware-acceleration-modal");if(m&&!m.classList.contains("hidden"))m.querySelector(".close-modal").click();})()');
  await delay(1500);await shot('guest-menu');
  await click(690,20);await delay(500);await shot('registration-dialog');
  await type(300,170,'login-921@example.test');await type(300,258,'fixture-password');await type(300,346,'fixture-password');await click(288,396);await click(484,467);
  await until(()=>replies('account/register').some(r=>r.body==='<Success/>'),'normal registration');
  await until(()=>replies('account/verify').some(r=>r.body==='<Account/>'),'credential authentication without email verification');
  await delay(500);await shot('registered-account');
  assert.deepEqual(await snapshot(),initial,'Registration replaced prepared progress');
  await click(480,435);await delay(500);await shot('registered-menu');
  // Logout must discard credentials, not delete saved progress: selected 123.5.1 WebAccount.clear, https://openrotmg.org/restoration/reference-data/client-123.5.1.swf
  await click(765,20);
  await until(()=>replies('char/list').some(r=>!r.body.includes(`<AccountId>${account}</AccountId>`)&&r.body.includes('<AccountId>')),'fresh logout guest');
  await delay(500);await shot('logged-out-guest');
  const guest=replies('char/list').at(-1).body;
  assert(!guest.includes('<Char id='),'Logout guest inherited the registered character');
  assert.deepEqual(await snapshot(),initial,'Logout changed registered progress');
  await click(775,20);await delay(500);await shot('signin-dialog');
  const count=replies('account/verify').length;
  await click(480,425);await delay(500);
  assert.equal(replies('account/verify').length,count,'Empty native fields reached authentication');
  await type(300,235,'missing@example.test');await type(300,323,'wrong-password');await click(480,425);
  await until(()=>replies('account/verify').length===count+1,'unknown-email refusal');
  assert.equal(replies('account/verify').at(-1).body,'<Error>invalid email or password</Error>');
  await shot('unknown-email-refused');
  await type(300,235,'login-921@example.test',true);await click(480,425);
  await until(()=>replies('account/verify').length===count+2,'wrong-password refusal');
  assert.equal(replies('account/verify').at(-1).body,'<Error>invalid email or password</Error>');
  await shot('wrong-password-refused');
  assert.deepEqual(await snapshot(),initial,'Refused credentials changed registered progress');
  await click(350,425);await delay(500);
  await click(775,20);await delay(500);
  await type(300,235,'login-921@example.test');await type(300,323,'fixture-password');await click(480,425);
  await until(()=>replies('account/verify').length>=count+4,'successful native sign-in and account view');
  assert(replies('account/verify').slice(-2).every(r=>r.body==='<Account/>'),'Correct credentials did not authenticate');
  await delay(500);await shot('signed-in-account');
  await click(480,435);await delay(500);
  await click(400,550);await delay(800);await click(400,550);
}
export async function run(game) {
  await game.act({keys:[],duration_ms:1500},{name:'signed-in-existing-character',when:v=>v.lifecycle.alive});
  const original=await game.observe();
  game.check('native registration logout and rejected sign-ins preserve the original saved character',original.session.account===account&&original.lifecycle.alive&&original.session.character===initial[0][3]&&JSON.stringify(original.character.items)===initial[0][5]);
  game.check('logout creates a separate charless guest without removing registered credentials',(await db('SELECT count(*) FROM credentials WHERE account=?'))[0][0]===1&&replies('char/list').some(r=>!r.body.includes(`<AccountId>${account}</AccountId>`)&&!r.body.includes('<Char id=')));
  async function walk(x,y) {
    for(let i=0;i<40;i++) {
      const p=(await game.observe()).position,dx=x-p.x,dy=y-p.y,distance=Math.hypot(dx,dy);
      if(distance<.2)return;
      const sx=(dx-dy)/Math.SQRT2,sy=(dx+dy)/Math.SQRT2,keys=[];
      if(Math.abs(sx)>.1)keys.push(sx>0?'d':'a');
      if(Math.abs(sy)>.1)keys.push(sy>0?'s':'w');
      await game.act({keys,duration_ms:Math.min(300,Math.max(50,Math.round(distance*100)))});
    }
    throw Error('Native walk did not reach Vault portal');
  }
  await walk(79,77.5);
  await game.act({keys:[],mouse:{x:700,y:575},duration_ms:1000});
  for(let i=0;i<8&&(await game.observe()).world!=='Vault';i++)await game.act({keys:[],duration_ms:500});
  game.check('registered login enters Vault through the ordinary portal',(await game.observe()).world==='Vault'&&(await game.observe()).session.character===original.session.character);
  await game.act({keys:['Insert'],duration_ms:1000});
  for(let i=0;i<8&&(await game.observe()).world!=='Nexus';i++)await game.act({keys:[],duration_ms:500});
  await game.act({keys:[],duration_ms:4000},{name:'registered-reconnected-nexus',when:v=>v.world==='Nexus'&&v.lifecycle.alive});
  const final=await game.observe();
  game.check('normal reconnect retains account character and equipment',final.world==='Nexus'&&final.session.account===account&&final.session.character===original.session.character&&JSON.stringify(final.character.items)===JSON.stringify(original.character.items));
  await writeFile(join(root,'login.json'),JSON.stringify({account,original,final,requests:traffic.map(r=>({path:r.path,error:r.body.startsWith('<Error>')}))},null,2));
  c.close();
}
