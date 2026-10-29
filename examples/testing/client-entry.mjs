import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {connect} from '../../scripts/record-demo.mjs';

export const fixture={class:0x30e,level:1,seed:918,world:-1,position:{x:12.5,y:164.5},equipment:[0xa97,0xa2e,-1,-1],backpack:Array(8).fill(-1),stats:null};
export async function enter({browser}){
  const {origin}=JSON.parse(await readFile(join(process.env.QA_RESULTS_DIR,'native/result.json'),'utf8'));
  const cdp=await connect({...browser,url:origin+'/',helper:join(process.env.PI_BROWSER_TOOLS,'direct-cdp.mjs')});
  try{
    await cdp.call('Emulation.setFocusEmulationEnabled',{enabled:true});
    await cdp.eval('(()=>{const modal=document.querySelector("ruffle-player").shadowRoot.querySelector("#hardware-acceleration-modal");if(modal&&!modal.classList.contains("hidden"))modal.querySelector(".close-modal").click();})()');
    const rect=await cdp.eval('(()=>{const r=document.querySelector("ruffle-player").getBoundingClientRect();return {x:r.x,y:r.y,width:r.width,height:r.height};})()');
    assert(rect.width===800&&rect.height===600);
    const capture=async name=>writeFile(join(process.env.QA_RESULTS_DIR,name+'.png'),Buffer.from((await cdp.call('Page.captureScreenshot',{format:'png'})).data,'base64'));
    await capture('title');
    for(let i=0;i<2;i++){
      const p={x:rect.x+400,y:rect.y+550,button:'left',clickCount:1};
      try{await cdp.call('Input.dispatchMouseEvent',{type:'mousePressed',buttons:1,...p});}finally{await cdp.call('Input.dispatchMouseEvent',{type:'mouseReleased',buttons:0,...p});}
      await cdp.eval('new Promise(ok=>requestAnimationFrame(()=>requestAnimationFrame(ok)))');
      if(i===0)await capture('selection');
    }
  }finally{cdp.close();}
}
export async function run(game){
  const out=join(process.env.QA_RESULTS_DIR,'native'),before=await game.inspect();
  await game.act({keys:[],duration_ms:1000},{name:'client-tutorial',when:v=>v.world==='Tutorial'&&v.game_ms>=300});
  game.check('normal selection and LOAD enter the prepared living Wizard in Tutorial',before.view.map.name==='Tutorial'&&before.view.lifecycle.alive&&before.view.character.level===1);
  await game.act({keys:['d'],duration_ms:500});
  const moved=await game.observe();
  game.check('the entered native client can actually move',Math.hypot(moved.position.x-before.view.position.x,moved.position.y-before.view.position.y)>.3);
  await game.act({keys:['Insert'],duration_ms:2000},{name:'client-nexus',when:v=>v.world==='Nexus'});
  const after=await game.inspect();
  game.check('ordinary reconnect retains account living character inventory and revision',after.view.map.name==='Nexus'&&after.view.lifecycle.alive&&after.view.session.player!==before.view.session.player&&after.view.session.account===before.view.session.account&&after.view.session.character===before.view.session.character&&JSON.stringify(after.view.character.items)===JSON.stringify(before.view.character.items)&&after.view.character.inventory_revision===before.view.character.inventory_revision);
  const config=JSON.parse(await readFile(join(out,'native.json'),'utf8'));
  // The recovered menu gates map-editor access on Admin, not the public pixel editor: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Rotmg123.5.1/src/_F_1/_C_Q_.as#L56-L59
  const xml=await (await fetch(config.origin+'/legacy-api/char/list',{method:'POST',body:new URLSearchParams({guid:'client-entry-unprivileged',password:''})})).text();
  game.check('ordinary account listings do not grant the Admin editor flag',xml.includes('<Chars')&&!xml.includes('<Admin'));
  await writeFile(join(out,'client-entry.json'),JSON.stringify({before,moved,after,ordinary_listing:xml},null,2));
}
