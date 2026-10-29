import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {setTimeout as delay} from 'node:timers/promises';
import {connect} from '../../scripts/record-demo.mjs';
export {fixture,run} from './client-entry.mjs';

export async function enter({browser,signal}){
  const out=process.env.QA_RESULTS_DIR;
  const {origin}=JSON.parse(await readFile(join(out,'native/result.json'),'utf8'));
  let listing,listed=false;
  const cdp=await connect({...browser,url:origin+'/',helper:join(process.env.PI_BROWSER_TOOLS,'direct-cdp.mjs'),onEvent:e=>{
    if(e.method==='Network.responseReceived'&&e.params.response.url===origin+'/legacy-api/char/list')listing=e.params.requestId;
    if(e.method==='Network.loadingFinished'&&e.params.requestId===listing)listed=true;
  }});
  const wait=async probe=>{
    const end=Date.now()+15000;
    while(!await probe()){
      signal.throwIfAborted();assert(Date.now()<end,'server selection state did not settle');
      await delay(50,undefined,{signal});
    }
  };
  try{
    await cdp.call('Network.enable');
    await cdp.call('Emulation.setFocusEmulationEnabled',{enabled:true});
    const closeModal=()=>cdp.eval('(()=>{const m=document.querySelector("ruffle-player").shadowRoot.querySelector("#hardware-acceleration-modal");if(m&&!m.classList.contains("hidden"))m.querySelector(".close-modal").click();})()');
    const capture=async name=>writeFile(join(out,name+'.png'),Buffer.from((await cdp.call('Page.captureScreenshot',{format:'png'})).data,'base64'));
    const click=async(x,y)=>{
      const r=await cdp.eval('(()=>{const r=document.querySelector("ruffle-player").getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height};})()');
      assert(r.width===800&&r.height===600);
      const p={x:r.x+x,y:r.y+y,button:'left',clickCount:1};
      try{await cdp.call('Input.dispatchMouseEvent',{type:'mousePressed',buttons:1,...p});}finally{await cdp.call('Input.dispatchMouseEvent',{type:'mouseReleased',buttons:0,...p});}
      await cdp.eval('new Promise(ok=>requestAnimationFrame(()=>requestAnimationFrame(ok)))');
    };
    // Reload rewrites AMF ordering and unrelated options; compare only the saved preferredServer field: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Rotmg123.5.1/src/_F_1/_0M_L_.as
    const preferred=()=>cdp.eval('(()=>{const v=localStorage.getItem(location.hostname+"//AssembleeGameClientOptions");if(v===null)throw Error("missing saved client options");return atob(v).includes("preferredServer\\x06\\x15Open RotMG");})()');
    await closeModal();await capture('selection-title');
    await click(306,550);await capture('selection-default');
    const before=await preferred();assert.equal(before,false,'fresh guest starts without the named preference');
    await click(200,190);await capture('selection-preferred');
    await wait(preferred);
    listed=false;
    await cdp.eval('window.__serverSelectionReload=true');
    await cdp.call('Page.reload');
    await wait(async()=>listed&&await cdp.eval('window.__serverSelectionReload===undefined&&document.querySelector("ruffle-player")?.ruffle().isPlaying===true'));
    await cdp.eval('new Promise(ok=>requestAnimationFrame(()=>requestAnimationFrame(ok)))');
    await closeModal();await click(306,550);await capture('selection-reloaded');
    const reloaded=await preferred();assert.equal(reloaded,true,'named preference survives ordinary page reload');
    await writeFile(join(out,'selection-preferences.json'),JSON.stringify({before,selected:true,reloaded},null,2));
    await click(400,550);await click(400,550);await click(400,550);
  }finally{cdp.close();}
}
