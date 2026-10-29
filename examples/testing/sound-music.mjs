import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {setTimeout as delay} from 'node:timers/promises';
import {connect} from '../../scripts/record-demo.mjs';

export const fixture={class:0x30e,level:20,seed:932,world:-2,position:'spawn',equipment:[0xa97,0xa2e,-1,-1],backpack:[0xa23,0xa23,-1,-1,-1,-1,-1,-1],stats:null};
let c;
const results=[];
export async function beforeNavigate({browser,signal}) {
  c=await connect({port:browser.port,target:browser.target,url:'about:blank',helper:join(process.env.PI_BROWSER_TOOLS,'direct-cdp.mjs')});
  signal.addEventListener('abort',()=>c.close(),{once:true});
  await c.call('Page.enable');
  // Successful requests cannot prove playback through saved mute settings: https://openrotmg.org/restoration/reference-data/client-123.5.1.swf
  await c.call('Page.addScriptToEvaluateOnNewDocument',{source:`(()=>{
    const connect=AudioNode.prototype.connect,outputs=new Map(),since=new Map();
    window.audioEvidence=[];
    AudioNode.prototype.connect=function(destination,...args){
      const result=Reflect.apply(connect,this,[destination,...args]);
      if(destination===this.context.destination){
        let analyser=outputs.get(this.context);
        if(!analyser){analyser=this.context.createAnalyser();outputs.set(this.context,analyser);since.set(this.context,this.context.currentTime);}
        Reflect.apply(connect,this,[analyser]);
      }
      return result;
    };
    window.resetAudioEvidence=()=>{audioEvidence=[];for(const context of outputs.keys())since.set(context,context.currentTime);};
    setInterval(()=>{for(const [context,analyser] of outputs){const data=new Float32Array(analyser.fftSize);analyser.getFloatTimeDomainData(data);audioEvidence.push({seconds:context.currentTime-since.get(context),state:context.state,rms:Math.sqrt(data.reduce((s,x)=>s+x*x,0)/data.length)});}},20);
  })()`});
}
async function measure(name,work) {
  await c.eval('resetAudioEvidence()');await work();
  const samples=await c.eval('audioEvidence');results.push({name,samples});
  await writeFile(join(process.env.QA_RESULTS_DIR,'audio.json'),JSON.stringify(results,null,2));
  const active=samples.filter(s=>s.state==='running'&&s.seconds>.3);
  assert(active.length>=3,'No sustained decoded output samples: '+name);
  return Math.max(...active.map(s=>s.rms));
}
async function menuClick(x,y) {
  const r=await c.eval('(()=>{const r=document.querySelector("ruffle-player").getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height};})()');
  assert(r.width===800&&r.height===600&&r.x>=0&&r.y>=0,'Unclipped native menu required');
  const p={x:r.x+x,y:r.y+y,button:'left',clickCount:1};
  try{await c.call('Input.dispatchMouseEvent',{type:'mousePressed',buttons:1,...p});}finally{await c.call('Input.dispatchMouseEvent',{type:'mouseReleased',buttons:0,...p});}
  await c.eval('new Promise(ok=>requestAnimationFrame(()=>requestAnimationFrame(ok)))');
}
async function closeModal() {
  await c.eval('(()=>{const m=document.querySelector("ruffle-player").shadowRoot.querySelector("#hardware-acceleration-modal");if(m&&!m.classList.contains("hidden"))m.querySelector(".close-modal").click();})()');
  await c.call('Emulation.setFocusEmulationEnabled',{enabled:true});
}
let splashMuted,splashRestored;
export async function enter() {
  await closeModal();await menuClick(14,14);
  splashMuted=await measure('splash-muted',()=>delay(1000));
  await menuClick(14,14);splashRestored=await measure('splash-restored',()=>delay(1200));
  await menuClick(400,550);await menuClick(400,550);
}
export async function run(game) {
  const click=(x,y)=>game.act({keys:[],mouse:{x,y},duration_ms:120});
  const options=async()=>{await game.act({keys:['o'],duration_ms:120});await click(480,82);};
  const sample=(name,input)=>measure(name,()=>game.act([{actor:game.actor,input:{keys:[],duration_ms:500}},{actor:game.actor,input}]));
  const idle={keys:[],duration_ms:1600},fire={keys:[],mouse:{x:450,y:300},duration_ms:1000};
  const continueInput={keys:[],mouse:{x:400,y:540},duration_ms:1000};
  try {
    game.check('splash mute silences decoded output',splashMuted<0.00001);
    game.check('splash unmute restores decoded music',splashRestored>0.001);
    await game.act({keys:[],duration_ms:1000});
    game.check('music produces nonzero decoded output',await sample('music-on',idle)>0.001);
    await options();await click(60,138);await game.act({keys:['o'],duration_ms:120});
    game.check('music toggle silences idle output',await sample('music-off',idle)<0.00001);
    game.check('weapon fires audibly with music off',await sample('weapon-on',fire)>0.001);
    await options();await click(60,226);await game.act({keys:['o'],duration_ms:120});
    game.check('weapon toggle suppresses firing audio',await sample('weapon-off',fire)<0.00001);
    const before=await game.observe();await game.act({keys:[' '],duration_ms:500});
    const spent=await game.observe();game.check('ordinary spell spends mana',spent.character.mp<before.character.mp);
    game.check('SFX remains audible with weapon sounds disabled',await sample('potion-weapon-off',{keys:['1'],duration_ms:1000})>0.001);
    const after=await game.observe();game.check('audible potion consumes once and restores mana',after.character.items[4]===-1&&after.character.mp>spent.character.mp&&after.character.inventory_revision===spent.character.inventory_revision+1);
    await options();await click(60,182);await game.act({keys:['o'],duration_ms:120});
    await game.act({keys:[' '],duration_ms:500});
    game.check('SFX toggle suppresses potion audio',await sample('sfx-off',{keys:['2'],duration_ms:1000})<0.00001);
    game.check('muted potion still consumes normally',(await game.observe()).character.items[5]===-1);
    await options();await click(60,226);await game.act({keys:['o'],duration_ms:120});
    game.check('weapon remains audible with SFX disabled',await sample('weapon-without-sfx',fire)>0.001);
    await options();await click(60,226);await click(60,138);await game.act({keys:['o'],duration_ms:120});
    game.check('music remains audible with both sound toggles off',await sample('music-only',idle)>0.001);
    const initial=await game.observe(),out=process.env.QA_RESULTS_DIR;
    const config=JSON.parse(await readFile(join(out,'native/native.json'),'utf8'));
    const token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
    const previous=await c.eval('performance.timeOrigin'),deadline=Date.now()+30000;
    await c.call('Page.reload');
    for(;;){
      assert(Date.now()<deadline,'Reload did not start Ruffle');
      if(await c.eval('performance.timeOrigin!=='+previous+'&&document.querySelector("ruffle-player")?.ruffle().isPlaying===true'))break;
      await delay(50);
    }
    await closeModal();
    for(;;){
      const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:'sound-reload-'+Date.now(),command:{op:'native_state',actors:[game.actor]}})});
      assert(response.ok,'Native reconnect state request failed');const state=await response.json();
      if(state.actors.find(a=>a.actor===game.actor)?.stage==='ready')break;
      assert(Date.now()<deadline,'Same character did not reconnect');await menuClick(400,550);await delay(250);
    }
    await game.act({keys:[],duration_ms:600});const reloaded=await game.observe();
    game.check('normal page reload reconnects same living character',reloaded.lifecycle.alive&&reloaded.session.character===initial.session.character);
    game.check('enabled music persists across reload',await sample('music-after-reload',idle)>0.001);
    await options();await click(60,138);
    await game.act({keys:[],duration_ms:500},{name:'sound-options-reloaded',when:()=>true});
    game.check('disabled SFX persists across reload',await sample('sfx-after-reload',continueInput)<0.00001);
    game.check('disabled weapon sounds persist across reload',await sample('weapon-after-reload',fire)<0.00001);
    await options();await click(60,182);
    game.check('re-enabling SFX restores the same UI sound',await sample('sfx-restored',continueInput)>0.001);
  } finally {c.close();}
}
