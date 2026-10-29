import assert from 'node:assert/strict';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {randomBytes,createHash} from 'node:crypto';
import {setTimeout as delay} from 'node:timers/promises';
import {connect} from '../../scripts/record-demo.mjs';
export const fixture={class:0x30e,level:20,seed:923,world:-2,position:'spawn',equipment:[-1,-1,-1,-1],backpack:Array(8).fill(-1),stats:null};
const exec=promisify(execFile),email='recovery@example.test',old=randomBytes(16).toString('hex'),password=randomBytes(16).toString('hex');
let cdp,browserReceipt,origin,listRequest,lists=0,registration,registered=false,verified=false;
const checks=[];
function check(name,value){assert(value,name);checks.push(name);}
async function until(test,label){const end=Date.now()+30000;while(!await test()){assert(Date.now()<end,label);await delay(30);}}
async function click(x,y){
  const r=await cdp.eval('(()=>{const r=document.querySelector("ruffle-player").getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height}})()');assert(r.width===800&&r.height===600);
  const p={x:r.x+x,y:r.y+y,button:'left',clickCount:1};
  try{await cdp.call('Input.dispatchMouseEvent',{type:'mousePressed',buttons:1,...p});}finally{await cdp.call('Input.dispatchMouseEvent',{type:'mouseReleased',buttons:0,...p});}
  await cdp.eval('new Promise(ok=>requestAnimationFrame(()=>requestAnimationFrame(ok)))');
}
async function modal(){await cdp.eval('(()=>{const m=document.querySelector("ruffle-player").shadowRoot.querySelector("#hardware-acceleration-modal");if(m&&!m.classList.contains("hidden"))m.querySelector(".close-modal").click();})()');}
async function shot(name){const image=await cdp.call('Page.captureScreenshot',{format:'png'});await writeFile(join(process.env.QA_RESULTS_DIR,name+'.png'),Buffer.from(image.data,'base64'));}
async function post(path,body,json=false){return cdp.eval(`fetch(${JSON.stringify(path)},{method:'POST',headers:${JSON.stringify(json?{'content-type':'application/json'}:{})},body:${json?JSON.stringify(JSON.stringify(body)):'new URLSearchParams('+JSON.stringify(body)+')'}}).then(async r=>({status:r.status,cache:r.headers.get('cache-control'),text:await r.text()}))`);}
export async function beforeNavigate({browser,signal}){
  browserReceipt=browser;
  cdp=await connect({port:browser.port,target:browser.target,url:'about:blank',helper:process.env.PI_BROWSER_TOOLS+'/direct-cdp.mjs',onEvent:e=>{
    if(e.method==='Network.requestWillBeSent'){
      const u=new URL(e.params.request.url);if(u.pathname==='/legacy-api/char/list'){origin=u.origin;listRequest=e.params.requestId;lists++;}
      if(u.pathname==='/legacy-api/account/verify')verified=true;
    }
    if(e.method==='Network.responseReceived'&&e.params.response.url.endsWith('/legacy-api/account/register'))registration=e.params.requestId;
    if(e.method==='Network.loadingFinished'&&e.params.requestId===registration)registered=true;
  }});signal.addEventListener('abort',()=>cdp.close(),{once:true});await cdp.call('Network.enable',{maxPostDataSize:65536});
}
export async function enter({account}){
  await cdp.call('Emulation.setFocusEmulationEnabled',{enabled:true});await modal();
  await click(690,15);
  for(const [y,text] of [[171,email],[259,old],[347,old]]){await click(350,y);await cdp.call('Input.insertText',{text});}
  await click(288,396);await click(490,477);await until(()=>registered&&verified,'Native registration');
  check('native registration succeeds',(await cdp.call('Network.getResponseBody',{requestId:registration})).body.includes('<Success'));
  check('name is assigned without changing credentials',(await post('/legacy-api/account/setName',{guid:email,password:old,name:'Recovera'})).text.includes('<Success'));
  const disabled=await post('/legacy-api/account/forgotPassword',{guid:email});
  check('unconfigured email explicitly refuses rather than pretending delivery',disabled.text.includes('not configured')&&disabled.cache==='no-store');
  check('invalid email is refused',(await post('/legacy-api/account/forgotPassword',{guid:'not-email'})).text.startsWith('<Error>'));
  const beforeXml=(await post('/legacy-api/char/list',{guid:email,password:old})).text;
  const token=randomBytes(32).toString('hex'),hash=createHash('sha256').update(token).digest('hex');
  // The selected client promises an emailed password, not this modern grant; mail issuance is tested separately: https://openrotmg.org/restoration/reference-data/client-123.5.1.swf (§_-QA§.§_-FV§).
  await exec('python3',['-c',`import sqlite3,sys,time
c=sqlite3.connect(sys.argv[1]);a=int(sys.argv[2]);e=sys.argv[3]
assert c.execute('SELECT email FROM credentials WHERE account=?',(a,)).fetchone()==(e,)
t=int(time.time());c.execute('INSERT INTO email_password_resets(account,email,hash,issued_at,expires_at) VALUES(?,?,?,?,?)',(a,e,sys.argv[4],t,t+900));c.commit()`,join(process.env.QA_RESULTS_DIR,'native/state/game.db'),String(account),email,hash]);
  assert(process.env.RECOVERY_WEBSITE,'RECOVERY_WEBSITE must name the tested website checkout');
  const website=resolve(process.env.RECOVERY_WEBSITE),assets=new Map();
  for(const path of ['/recover/','/recover/app.js','/recover/style.css','/style.css','/backend.js']){
    assets.set(origin+path,await readFile(join(website,'public',path==='/recover/'?'recover/index.html':path.slice(1)),'utf8'));
  }
  await writeFile(join(process.env.QA_RESULTS_DIR,'recovery-assets.json'),JSON.stringify({revision:(await exec('git',['-C',website,'rev-parse','HEAD'])).stdout.trim(),files:Object.fromEntries([...assets].map(([url,bytes])=>[new URL(url).pathname,createHash('sha256').update(bytes).digest('hex')]))},null,2));
  check('reset refuses missing Origin',(await fetch(origin+'/api/account/recover',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({token,password})})).status===403);
  let staticFailure;
  const browser=browserReceipt;
  const page=await connect({port:browser.port,target:browser.target,url:origin,helper:process.env.PI_BROWSER_TOOLS+'/direct-cdp.mjs',onEvent:e=>{
    if(e.method!=='Fetch.requestPaused')return;
    (async()=>{const request=e.params;assert.equal(request.request.method,'GET');const url=request.request.url;assert(assets.has(url),'Unexpected static request '+url);let body=assets.get(url).replaceAll('{{API_ORIGIN}}',origin).replaceAll('{{BASE}}','/');await page.call('Fetch.fulfillRequest',{requestId:request.requestId,responseCode:200,responseHeaders:[{name:'Content-Type',value:url.endsWith('.js')?'text/javascript':url.endsWith('.css')?'text/css':'text/html'}],body:Buffer.from(body).toString('base64')});})().catch(e=>staticFailure=e);
  }});
  try{
    await page.call('Fetch.enable',{patterns:[...assets.keys()].map(urlPattern=>({urlPattern,requestStage:'Request'}))});
    await cdp.call('Page.navigate',{url:origin+'/recover/#'+token});
    await until(()=>cdp.eval('document.querySelector("#reset")?.hidden===false'),'Real reset page');assert(!staticFailure,String(staticFailure));
    check('reset page removes fragment from history',await cdp.eval('!location.hash&&!location.search'));
    const fill=async(a,b)=>cdp.eval(`(()=>{document.querySelector('#password').value=${JSON.stringify(a)};document.querySelector('#confirm').value=${JSON.stringify(b)};document.querySelector('#reset').requestSubmit()})()`);
    await fill('abcd','abcd');check('short password rejected in rendered form',(await cdp.eval('document.querySelector("#status").textContent')).includes('5–128'));
    await fill(password,password+'x');check('mismatched passwords rejected in rendered form',(await cdp.eval('document.querySelector("#status").textContent')).includes('do not match'));
    await shot('reset-validation');await fill(password,password);
    await until(async()=>await cdp.eval('document.querySelector("#status").textContent.startsWith("Password reset.")'),'Reset completion');await shot('reset-completed');
    check('one-use link replay is refused',(await post('/api/account/recover',{token,password:old},true)).status===400);
    check('old password is rejected',(await post('/legacy-api/account/verify',{guid:email,password:old})).text.startsWith('<Error>'));
    check('new password authenticates and verifies same email',(await post('/legacy-api/account/verify',{guid:email,password})).text.includes('<VerifiedEmail/>'));
    const afterXml=(await post('/legacy-api/char/list',{guid:email,password})).text;
    check('reset preserves account and saved character listing',afterXml.replace('<VerifiedEmail/>','')===beforeXml.replace('<VerifiedEmail/>',''));
  }finally{await page.call('Fetch.disable');page.close();}
  await cdp.call('Page.navigate',{url:origin});await until(()=>cdp.eval('document.querySelector("ruffle-player")?.ruffle().isPlaying===true'),'Native return');await modal();
  await shot('native-return');
  await click(760,15);await shot('native-login');
  for(const [y,text] of [[235,email],[321,password]]){await click(350,y);await cdp.call('Input.insertText',{text});}
  const before=lists;await click(490,412);await until(()=>lists>before,'Recovered native char list');
  check('native login uses recovered credentials',new URLSearchParams((await cdp.call('Network.getRequestPostData',{requestId:listRequest})).postData).get('guid')===email);
  const namedLists=lists;await cdp.call('Page.reload');await until(()=>lists>namedLists,'Recovered saved login reconnect');
  check('reload retains same registered email',new URLSearchParams((await cdp.call('Network.getRequestPostData',{requestId:listRequest})).postData).get('guid')===email);
  await until(()=>cdp.eval('document.querySelector("ruffle-player")?.ruffle().isPlaying===true'),'Recovered menu');await modal();await click(400,550);await click(400,550);
}
export async function run(game){
  try{
    const before=await game.observe();await game.act({keys:['d'],duration_ms:1000},{name:'recovered-gameplay',when:v=>v.game_ms>=before.game_ms+500});const after=await game.observe();
    game.check('recovered character is alive and moves in Nexus',after.lifecycle.alive&&after.world==='Nexus'&&after.position.x>before.position.x);
    for(const name of checks)game.check(name,true);
    await writeFile(join(process.env.QA_RESULTS_DIR,'recovery-checks.json'),JSON.stringify({checks,accountFixture:'email reset grant inserted in isolated state; real form and reset API, no mail sent',before,after},null,2));
  }finally{cdp.close();}
}
