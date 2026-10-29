import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {setTimeout as delay} from 'node:timers/promises';
import {connect} from '../../scripts/record-demo.mjs';

export const fixture={class:0x30e,level:1,seed:927,world:-2,position:{x:80.9,y:74.5},equipment:[0xa97,0xa2e,-1,-1],backpack:[0xa22,-1,-1,-1,-1,-1,-1,-1],stats:null};
let receipt;
export async function enter({actor,account,browser,signal}) {
  const root=join(process.env.QA_RESULTS_DIR,'native');
  const {origin}=JSON.parse(await readFile(join(root,'result.json'),'utf8'));
  const token=(await readFile(join(root,'state/control.token'),'utf8')).trim();
  let serial=0;
  const api=async command=>{
    const response=await fetch(origin+'/dev/testing',{method:'POST',headers:{Authorization:`Bearer ${token}`,'Content-Type':'application/json'},body:JSON.stringify({id:'slots-'+serial++,command}),signal});
    const value=await response.json();assert(response.ok&&value.status!=='failed',JSON.stringify(value));return value;
  };
  const c=await connect({...browser,url:origin+'/',helper:join(process.env.PI_BROWSER_TOOLS,'direct-cdp.mjs')});
  const captures=[];
  try {
    await c.call('Emulation.setFocusEmulationEnabled',{enabled:true});
    await c.eval('(()=>{const m=document.querySelector("ruffle-player").shadowRoot.querySelector("#hardware-acceleration-modal");if(m&&!m.classList.contains("hidden"))m.querySelector(".close-modal").click();})()');
    const rect=await c.eval('(()=>{const r=document.querySelector("ruffle-player").getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height};})()');
    assert(rect.width===800&&rect.height===600,'Unclipped native client required');
    const mouse=(type,x,y,buttons)=>c.call('Input.dispatchMouseEvent',{type,x:rect.x+x,y:rect.y+y,button:'left',buttons,clickCount:1});
    const click=async(x,y)=>{try{await mouse('mousePressed',x,y,1);}finally{await mouse('mouseReleased',x,y,0);}};
    const capture=async name=>{await writeFile(join(root,name+'.png'),Buffer.from((await c.call('Page.captureScreenshot',{format:'png'})).data,'base64'));captures.push(name);};
    const wait=async(test,label)=>{for(let i=0;i<150;i++){signal.throwIfAborted();if(await test())return;await delay(100);}throw Error(label);};
    const ready=async()=>{const s=await api({op:'native_state',actors:[actor]});return !s.stopped&&s.actors[0]?.stage==='ready';};
    await click(400,550);await delay(300);await click(400,550);
    await wait(ready,'Initial native character LOAD did not complete');await delay(500);
    const before=await api({op:'observe',actor});
    await capture('before-switch');
    await mouse('mouseMoved',634,528,0);await mouse('mousePressed',634,528,1);await delay(150);
    await mouse('mouseMoved',678,528,1);await delay(150);await mouse('mouseReleased',678,528,0);
    await wait(async()=>{const v=(await api({op:'observe',actor}));return v.character.items[4]===-1&&v.character.items[5]===0xa22;},'Native inventory drag was not committed');
    await mouse('mouseMoved',400,300,0);await mouse('mousePressed',400,300,1);await delay(500);await mouse('mouseReleased',400,300,0);
    await wait(async()=> (await api({op:'observe',actor})).verification.shots>before.verification.shots,'Native weapon fire did not advance the live shot counter');
    const changed=await api({op:'observe',actor});
    assert(changed.character.inventory_revision>before.character.inventory_revision,'Inventory revision must advance');
    // The selected client dispatches COMPLETE, not death: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Rotmg123.5.1/src/_R_v/_zL_.as#L16-L18
    await click(700,580);await delay(700);await capture('character-selection');
    const {stdout}=await promisify(execFile)('python3',['-c','import sqlite3,json,sys; c=sqlite3.connect(sys.argv[1]); print(json.dumps(c.execute("SELECT alive,items,revision,stats FROM characters WHERE account=? AND id=?",(int(sys.argv[2]),int(sys.argv[3]))).fetchone()))',join(root,'state/game.db'),String(account),String(actor)]);
    const [alive,items,revision,stats]=JSON.parse(stdout);
    assert(alive===1&&JSON.parse(stats).shots===changed.verification.shots,'Selection must follow durable living shot progress');
    assert.deepEqual(JSON.parse(items),changed.character.items,'Selection must follow durable inventory');
    assert.equal(revision,changed.character.inventory_revision);
    await click(400,550);
    await wait(async()=>{if(!await ready())return false;const v=await api({op:'observe',actor});return v.session.player!==before.session.player;},'Change Characters did not reconnect the same owned living character');
    await delay(500);
    const after=await api({op:'observe',actor});await capture('after-switch');
    assert(after.lifecycle.alive&&after.session.account===account&&after.session.character===before.session.character,'Switch must preserve living ownership');
    assert.deepEqual(after.character.items,changed.character.items,'Native reconnect must retain the changed inventory');
    assert.equal(after.character.inventory_revision,changed.character.inventory_revision,'Native reconnect must retain committed inventory revision');
    assert.equal(after.verification.shots,changed.verification.shots,'Reconnect must retain shot progress');
    receipt={before,changed,storedAtSelection:{alive,items:JSON.parse(items),revision,shots:JSON.parse(stats).shots},after,captures,fixture:'Prepared living guest; one unearned health potion. No slot or currency grant.'};
    await writeFile(join(root,'character-switch.json'),JSON.stringify(receipt,null,2));
  }finally{c.close();}
}
export async function run(game) {
  const before=await game.observe();
  await game.act({keys:['d'],duration_ms:300});
  const after=await game.observe();
  game.check('Change Characters reconnects the same living account and character',receipt.after.lifecycle.alive&&receipt.after.session.character===receipt.before.session.character&&receipt.after.session.player!==receipt.before.session.player);
  game.check('Changed inventory and revision survive native selection and reconnect',JSON.stringify(after.character.items)===JSON.stringify(receipt.changed.character.items)&&after.character.inventory_revision===receipt.changed.character.inventory_revision);
  game.check('Living shot progress is stored before reentry and retained afterward',receipt.storedAtSelection.shots>receipt.before.verification.shots&&receipt.storedAtSelection.shots===after.verification.shots);
  game.check('The reselected native character remains playable',Math.hypot(after.position.x-before.position.x,after.position.y-before.position.y)>.3);
}
