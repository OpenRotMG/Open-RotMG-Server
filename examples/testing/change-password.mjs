import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {setTimeout as delay} from 'node:timers/promises';
import {connect} from '../../scripts/record-demo.mjs';

const out=()=>join(process.env.QA_RESULTS_DIR,'native');
const email='password-native@example.test',old='old-local-password',next='new-local-password';
let checks=[];
export const fixture={class:0x30e,level:1,seed:925,world:-2,position:'spawn',equipment:[0xa97,0xa2e,-1,-1],backpack:Array(8).fill(-1),stats:null};
export async function enter({account,browser,signal}) {
  const pages=await (await fetch(`http://127.0.0.1:${browser.port}/json/list`,{signal})).json();
  const responses=[];
  const page=await connect({...browser,url:pages.find(p=>p.id===browser.target).url,helper:join(process.env.PI_BROWSER_TOOLS,'direct-cdp.mjs'),onEvent:e=>{
    if(e.method==='Network.responseReceived'&&e.params.response.url.endsWith('/account/changePassword'))responses.push(e.params.requestId);
  }});
  try {
    await page.call('Network.enable');
    await page.call('Emulation.setFocusEmulationEnabled',{enabled:true});
    await page.eval('(()=>{const m=document.querySelector("ruffle-player").shadowRoot.querySelector("#hardware-acceleration-modal");if(m&&!m.classList.contains("hidden"))m.querySelector(".close-modal").click();})()');
    const r=await page.eval('(()=>{const r=document.querySelector("ruffle-player").getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height};})()');
    assert.equal(r.width,800);assert.equal(r.height,600);
    async function key(key,code,n) {for(const type of ['keyDown','keyUp'])await page.call('Input.dispatchKeyEvent',{type,key,code,windowsVirtualKeyCode:n});}
    async function click(x,y) {
      const p={x:r.x+x,y:r.y+y,button:'left',clickCount:1};
      try {await page.call('Input.dispatchMouseEvent',{type:'mousePressed',buttons:1,...p});}
      finally {await page.call('Input.dispatchMouseEvent',{type:'mouseReleased',buttons:0,...p});}
      await delay(250,undefined,{signal});
    }
    async function field(y,text) {
      await click(310,y);await key('End','End',35);
      for(let i=0;i<40;i++)await key('Backspace','Backspace',8);
      await page.call('Input.insertText',{text});
    }
    async function capture(name) {await delay(300,undefined,{signal});const shot=await page.call('Page.captureScreenshot',{format:'png'});await writeFile(join(out(),name+'.png'),Buffer.from(shot.data,'base64'));}
    function credential() {return execFileSync('python3',['-c','import sqlite3,sys; c=sqlite3.connect("file:"+sys.argv[1]+"?mode=ro",uri=True); r=c.execute("SELECT password_hash FROM credentials WHERE account=?",(int(sys.argv[2]),)).fetchone(); assert r; print(r[0])',join(out(),'state/game.db'),String(account)],{encoding:'utf8',signal}).trim();}
    function check(name,ok) {assert(ok,name);checks.push(name);}
    await click(400,550);await click(700,12);
    await page.call('Input.insertText',{text:email});await key('Tab','Tab',9);
    await page.call('Input.insertText',{text:old});await key('Tab','Tab',9);
    await page.call('Input.insertText',{text:old});await click(285,395);await click(490,470);
    const initial=credential();
    await capture('current-account');await click(350,320);await capture('change-form');
    const origin=await page.eval('location.origin');
    async function listing(password) {
      const response=await fetch(origin+'/legacy-api/char/list',{method:'POST',body:new URLSearchParams({guid:email,password}),signal});
      assert(response.ok);return response.text();
    }
    async function submit(name,current,newPassword,retype) {
      await field(203,current);await field(291,newPassword);await field(379,retype);await click(485,440);await capture(name);
    }
    // The selected form validates before sending and changes its cache only on Success: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Rotmg123.5.1/src/_Q_A_/_ag.as
    await submit('short-current','bad',next,next);
    check('short current password leaves durable credential intact',credential()===initial);
    await submit('short-new',old,'tiny','tiny');
    check('short replacement leaves durable credential intact',credential()===initial);
    await submit('mismatched-new',old,next,'does-not-match');
    check('mismatched confirmation leaves durable credential intact',credential()===initial);
    check('native validation prevents all three invalid requests',responses.length===0);
    await submit('wrong-current','wrong-local-password',next,next);
    assert.equal(responses.length,1,'Wrong-current submission must reach the server once');
    check('server rejects wrong current password without changing credential',(await page.call('Network.getResponseBody',{requestId:responses[0]})).body.startsWith('<Error>')&&credential()===initial&&(await listing(old)).startsWith('<Chars'));
    await submit('password-changed',old,next,next);
    check('native submission commits a replacement verifier',credential()!==initial);
    check('old password no longer authenticates and new password retains the same account',(await listing(old)).startsWith('<Error>')&&(await listing(next)).includes(`<AccountId>${account}</AccountId>`));
    await click(485,385);await click(400,550);await click(400,550);
    await writeFile(join(out(),'password-checks.json'),JSON.stringify({account,checks,responses},null,2));
  } finally {page.close();}
}
export async function run(game) {
  for(const check of checks)game.check(check,true);
  const before=await game.observe();
  game.check('native cached replacement password loads the original living character',before.lifecycle.alive&&before.world==='Nexus');
  const {origin}=JSON.parse(await readFile(join(out(),'native.json'),'utf8'));
  const response=await fetch(origin+'/legacy-api/account/changePassword',{method:'POST',body:new URLSearchParams({guid:email,password:next,newPassword:'refused-while-playing'})});
  game.check('authenticated password change refuses active ownership',response.ok&&(await response.text()).includes('active session'));
  const after=await game.observe();
  game.check('active refusal preserves the living session and inventory',after.lifecycle.alive&&JSON.stringify(after.session)===JSON.stringify(before.session)&&JSON.stringify(after.character.items)===JSON.stringify(before.character.items));
  await game.act({keys:[],duration_ms:1000},{name:'password-native-gameplay',when:v=>v.game_ms>=before.game_ms+500});
}
