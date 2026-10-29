import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {setTimeout as delay} from 'node:timers/promises';
import {randomUUID} from 'node:crypto';
import {connect} from '../../scripts/record-demo.mjs';

const out=()=>join(process.env.QA_RESULTS_DIR,'native');
let c,api;
export const fixture={class:0x30e,level:20,seed:931,world:-2,position:{x:109.5,y:42.5},equipment:[0xa97,0xa2e,-1,-1],backpack:Array(8).fill(-1),stats:null};
export async function enter({browser,signal,actor}){
  const {origin}=JSON.parse(await readFile(join(out(),'result.json'),'utf8'));
  const token=(await readFile(join(out(),'state/control.token'),'utf8')).trim();
  api=async command=>{const r=await fetch(origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command}),signal:AbortSignal.timeout(10000)});const s=await r.json();assert(r.ok&&s.status==='succeeded',JSON.stringify(s));return s.result;};
  c=await connect({...browser,url:origin+'/',helper:join(process.env.PI_BROWSER_TOOLS,'direct-cdp.mjs')});
  signal.addEventListener('abort',()=>c.close(),{once:true});
  await c.call('Emulation.setFocusEmulationEnabled',{enabled:true});
  await c.eval('(()=>{const m=document.querySelector("ruffle-player").shadowRoot.querySelector("#hardware-acceleration-modal");if(m&&!m.classList.contains("hidden"))m.querySelector(".close-modal").click();})()');
  const deadline=Date.now()+30000;
  while((await api({op:'native_state',actors:[actor]})).actors[0].stage!=='ready'){assert(Date.now()<deadline,'Normal menu did not load the owned character');await click(400,550);await delay(250);}
}
async function click(x,y){
  const r=await c.eval('(()=>{const r=document.querySelector("ruffle-player").getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height};})()');
  assert(r.width===800&&r.height===600);
  const p={x:r.x+x,y:r.y+y,button:'left',clickCount:1};
  await c.call('Input.dispatchMouseEvent',{type:'mouseMoved',...p});
  try{await c.call('Input.dispatchMouseEvent',{type:'mousePressed',buttons:1,...p});}finally{await c.call('Input.dispatchMouseEvent',{type:'mouseReleased',buttons:0,...p});}
}
export async function run(game){
  const config=JSON.parse(await readFile(join(out(),'native.json'),'utf8')),records=[];
  async function save(label){records.push({label,state:await game.inspect(),storage:await c.eval('Object.fromEntries(Object.entries(localStorage))')});await writeFile(join(out(),'preferences.json'),JSON.stringify(records,null,2));}
  async function ui(label,work){
    const log=join(out(),'controller.stderr'),offset=(await readFile(log,'utf8')).length;
    const action=game.act({keys:[],duration_ms:1800});
    const input=(async()=>{const end=Date.now()+5000;while(true){const lines=(await readFile(log,'utf8')).slice(offset).split('\n').slice(0,-1);if(lines.some(l=>l.startsWith('{"event":"native_sync_verified_ack"')&&JSON.parse(l).context.actor===game.actor))break;assert(Date.now()<end,'UI requires current native ACK');await delay(20);}await work();})();
    const results=await Promise.allSettled([action,input]);for(const r of results)if(r.status==='rejected')throw r.reason;
    await save(label);
  }
  const shot=name=>game.act({keys:[],duration_ms:700},{name,when:()=>true});
  const button=(label,x,y)=>ui(label,()=>click(x,y));
  const key=async key=>game.act({keys:[key],duration_ms:250});
  const press=async(key,code,windowsVirtualKeyCode)=>{const p={key,code,windowsVirtualKeyCode};try{await c.call('Input.dispatchKeyEvent',{type:'keyDown',...p});}finally{await c.call('Input.dispatchKeyEvent',{type:'keyUp',...p});}};
  const before=await game.inspect();
  await shot('default-display');await save('default');
  await key('o');await shot('controls-default');
  await button('allow-rotation',35,225);
  await key('o');await key('q');await shot('rotated-display');
  await key('r');await shot('reset-camera');
  await key('o');await button('graphics-tab',365,83);await shot('graphics-default');
  // Popup preferences select panel versus chat, not refusal: selected 123.5.1 Options.as, https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Rotmg123.5.1/src/_0C_P_/Options.as
  for(const [label,x,y] of [['camera-zero',35,137],['offset-player',430,137],['quest-portraits-off',35,181],['tips-off',430,181],['shadows-off',35,225],['bubbles-off',430,225],['trade-popup-off',35,269],['guild-popup-off',430,269]])await button(label,x,y);
  await shot('graphics-changed');
  await button('chat-tab',255,83);await button('filter-off',35,225);await shot('chat-changed');
  await key('o');await shot('offset-zero-display');
  await ui('zoom-out',async()=>{await press('-','Minus',189);await press('-','Minus',189);});await shot('minimap-zoom-out');
  await ui('zoom-in',()=>press('=','Equal',187));await shot('minimap-zoom-in');
  const start=await game.inspect();
  await game.act({keys:['d'],duration_ms:600});
  const moved=await game.inspect();
  game.check('camera-zero movement is observed in the ordinary native client',moved.view.position.x>start.view.position.x+.3&&Math.abs(moved.view.position.y-start.view.position.y)<.2);
  await ui('tutorial-command',async()=>{const enter={key:'Enter',code:'Enter',windowsVirtualKeyCode:13};for(const first of [true,false]){try{await c.call('Input.dispatchKeyEvent',{type:'keyDown',...enter});}finally{await c.call('Input.dispatchKeyEvent',{type:'keyUp',...enter});}if(first)await c.call('Input.insertText',{text:'/tutorial'});}});
  game.check('ordinary tutorial reconnect succeeds',(await game.observe()).world==='Tutorial');
  await shot('tutorial-preferences');
  await game.act({keys:['Insert'],duration_ms:1200});await game.act({keys:[],duration_ms:700});
  const after=await game.inspect();
  game.check('ordinary Nexus reconnect preserves living identity and inventory',after.view.map.name==='Nexus'&&after.view.lifecycle.alive&&after.view.session.account===before.view.session.account&&after.view.session.character===before.view.session.character&&JSON.stringify(after.view.character.items)===JSON.stringify(before.view.character.items)&&after.view.character.inventory_revision===before.view.character.inventory_revision);
  await key('o');await button('graphics-after-reconnect',365,83);await shot('graphics-after-reconnect');
  await key('o');await save('before-reload');
  const previous=await c.eval('performance.timeOrigin'),deadline=Date.now()+30000;
  await c.call('Page.reload');
  while(!await c.eval('performance.timeOrigin!=='+previous+'&&document.querySelector("ruffle-player")?.ruffle().isPlaying===true')){assert(Date.now()<deadline,'Fresh Ruffle did not start');await delay(50);}
  await c.eval('(()=>{const m=document.querySelector("ruffle-player").shadowRoot.querySelector("#hardware-acceleration-modal");if(m&&!m.classList.contains("hidden"))m.querySelector(".close-modal").click();})()');
  while((await api({op:'native_state',actors:[game.actor]})).actors[0].stage!=='ready'){assert(Date.now()<deadline,'Same-profile reload did not load the owned character');await click(400,550);await delay(250);}
  await game.act({keys:[],duration_ms:700});
  const reloaded=await game.inspect();
  game.check('same-profile reload retains the same living account character and inventory',reloaded.view.lifecycle.alive&&reloaded.view.session.account===before.view.session.account&&reloaded.view.session.character===before.view.session.character&&JSON.stringify(reloaded.view.character.items)===JSON.stringify(before.view.character.items));
  await key('o');await button('graphics-after-reload',365,83);await shot('graphics-after-reload');
  await key('o');await save('final');
  game.check('same owned native actor remains bound',config.native[0].actor===game.actor);
}
