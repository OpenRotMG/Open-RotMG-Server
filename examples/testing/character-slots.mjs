import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {setTimeout as delay} from 'node:timers/promises';
import {connect} from '../../scripts/record-demo.mjs';

export const fixture={class:0x30e,level:1,seed:927,world:-2,position:{x:80.9,y:74.5},equipment:[0xa97,0xa2e,-1,-1],backpack:[0xa22,-1,-1,-1,-1,-1,-1,-1],stats:null};
let c;
export async function beforeNavigate({browser}) {
  c=await connect({...browser,url:'about:blank',helper:join(process.env.PI_BROWSER_TOOLS,'direct-cdp.mjs')});
  await c.call('Emulation.setFocusEmulationEnabled',{enabled:true});
}
export async function run(game) {
  const root=join(process.env.QA_RESULTS_DIR,'native');
  const {account}=JSON.parse(await readFile(join(root,'result.json'),'utf8'));
  const rect=await c.eval('(()=>{const r=document.querySelector("ruffle-player").getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height};})()');
  assert(rect.width===800&&rect.height===600,'Unclipped native client required');
  const click=async(x,y)=>{
    const p={x:rect.x+x,y:rect.y+y,button:'left',clickCount:1};
    try{await c.call('Input.dispatchMouseEvent',{type:'mousePressed',buttons:1,...p});}finally{await c.call('Input.dispatchMouseEvent',{type:'mouseReleased',buttons:0,...p});}
  };
  const capture=async name=>writeFile(join(root,name+'.png'),Buffer.from((await c.call('Page.captureScreenshot',{format:'png'})).data,'base64'));
  const before=await game.observe();
  await game.act({keys:[],duration_ms:700},{name:'before-switch',when:v=>v.game_ms>=before.game_ms+300});
  await game.act({keys:[],duration_ms:350,drag:{from:{x:634,y:528},to:{x:678,y:528}}});
  await game.act({keys:[],mouse:{x:400,y:300},duration_ms:500});
  const changed=await game.observe();
  game.check('Native inventory drag and weapon fire change actual living progress',changed.character.items[4]===-1&&changed.character.items[5]===0xa22&&changed.character.inventory_revision>before.character.inventory_revision&&changed.verification.shots>before.verification.shots);
  const action=game.act({keys:[],duration_ms:6000});
  let storedAtSelection;
  try {
    for(let i=0;;i++) {
      if(!(await game.observe()).paused&&await c.eval('document.querySelector("ruffle-player").ruffle().isPlaying'))break;
      assert(i<100,'Native control action did not resume');await delay(20);
    }
    // The selected client dispatches COMPLETE, not death: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Rotmg123.5.1/src/_R_v/_zL_.as#L16-L18
    await click(700,580);await delay(700);await capture('character-selection');
    const {stdout}=await promisify(execFile)('python3',['-c','import sqlite3,json,sys; c=sqlite3.connect(sys.argv[1]); print(json.dumps(c.execute("SELECT alive,items,revision,stats FROM characters WHERE account=? AND id=?",(int(sys.argv[2]),int(sys.argv[3]))).fetchone()))',join(root,'state/game.db'),String(account),String(game.actor)]);
    const [alive,items,revision,stats]=JSON.parse(stdout);
    storedAtSelection={alive,items:JSON.parse(items),revision,shots:JSON.parse(stats).shots};
    await click(400,550);
  }finally{await action;}
  const after=await game.observe();
  game.check('Selection retains committed living inventory and shot progress',storedAtSelection.alive===1&&JSON.stringify(storedAtSelection.items)===JSON.stringify(changed.character.items)&&storedAtSelection.revision===changed.character.inventory_revision&&storedAtSelection.shots===changed.verification.shots);
  game.check('Change Characters reconnects the same living account and character',after.lifecycle.alive&&after.session.account===account&&after.session.character===before.session.character&&after.session.player!==before.session.player);
  game.check('Native reconnect retains changed inventory revision and shots',JSON.stringify(after.character.items)===JSON.stringify(changed.character.items)&&after.character.inventory_revision===changed.character.inventory_revision&&after.verification.shots===changed.verification.shots);
  await game.act({keys:[],duration_ms:700},{name:'after-switch',when:v=>v.game_ms>=after.game_ms+300});
  await game.act({keys:['d'],duration_ms:300});
  const moved=await game.observe();
  game.check('The reselected native character remains playable',Math.hypot(moved.position.x-after.position.x,moved.position.y-after.position.y)>.3);
  await writeFile(join(root,'character-switch.json'),JSON.stringify({before,changed,storedAtSelection,after,moved,fixture:'Prepared living guest and one unearned health potion; no slot/currency grant.'},null,2));
  c.close();
}
