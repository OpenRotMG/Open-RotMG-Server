import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {connect} from '../../scripts/record-demo.mjs';

export const fixture={class:0x30e,level:20,seed:778,world:-2,position:'spawn',
  equipment:[0xa97,0xa2e,-1,-1],backpack:Array(8).fill(-1),stats:null};
const root=()=>process.argv[process.argv.indexOf('--run-dir')+1];
let page,rect;
export async function enter({actor,account,browser,signal}){
  const prepared=JSON.parse(await readFile(join(root(),'fixture.json'),'utf8'));
  assert(prepared.prepared&&prepared.actor===actor&&prepared.account===account);
  // Seed only the unloaded disposable fixture; earning five stars is outside this HUD check.
  const seeded=execFileSync('python3',['-c',`
import json,sqlite3,sys
with sqlite3.connect(sys.argv[1]) as db:
    row=db.execute('SELECT stats,alive FROM characters WHERE id=? AND account=?',(int(sys.argv[2]),int(sys.argv[3]))).fetchone()
    assert row and row[1]==1
    ch=json.loads(row[0])
    assert (ch['class'],ch['level'],ch['exp'],ch['fame'])==(782,20,18050,18)
    assert db.execute('SELECT count(*) FROM deaths').fetchone()[0]==0
    assert db.execute('SELECT balance,earned FROM account_fame WHERE account=?',(int(sys.argv[3]),)).fetchone()==(0,0)
    ch.update(exp=2000000,fame=2000)
    assert db.execute('UPDATE characters SET stats=? WHERE id=? AND account=?',(json.dumps(ch),int(sys.argv[2]),int(sys.argv[3]))).rowcount==1
    print(json.dumps({'actor':int(sys.argv[2]),'account':int(sys.argv[3]),'exp':ch['exp'],'fame':ch['fame'],'prepared':True}))
`,join(root(),'state','game.db'),String(actor),String(account)],{encoding:'utf8',signal});
  await writeFile(join(process.env.QA_RESULTS_DIR,'terminal-fixture.json'),seeded);
  const pages=await (await fetch(`http://127.0.0.1:${browser.port}/json/list`,{signal})).json();
  const target=pages.find(p=>p.id===browser.target);assert(target);
  page=await connect({...browser,url:target.url,helper:join(process.env.PI_BROWSER_TOOLS,'direct-cdp.mjs')});
  try{
    await page.eval('(()=>{const m=document.querySelector("ruffle-player").shadowRoot.querySelector("#hardware-acceleration-modal");if(m&&!m.classList.contains("hidden"))m.querySelector(".close-modal").click();})()');
    rect=await page.eval('(()=>{const r=document.querySelector("ruffle-player").getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height};})()');
    assert.equal(rect.width,800);assert.equal(rect.height,600);
    await page.call('Emulation.setFocusEmulationEnabled',{enabled:true});
    for(let n=0;n<2;n++){
      const p={x:rect.x+400,y:rect.y+550,button:'left',clickCount:1};
      try{await page.call('Input.dispatchMouseEvent',{type:'mousePressed',buttons:1,...p});}
      finally{await page.call('Input.dispatchMouseEvent',{type:'mouseReleased',buttons:0,...p});}
      await page.eval('new Promise(ok=>requestAnimationFrame(()=>requestAnimationFrame(ok)))');
    }
  }catch(error){page.close();throw error;}
}
export async function run(game){
  try{
    await game.act({keys:[],duration_ms:1800});
    const start=(await game.observe()).time_ms;
    // Nonpositive goals intentionally omit the denominator: selected 123.5.1 StatusBar, https://github.com/OpenRotMG/Open-RotMG-Client/blob/39acbfbf3bf545c397d6ac94f0a9430d6d660ac2/client.swf
    await game.act({keys:[],duration_ms:1000,mouse:{x:700,y:240}},{name:'terminal-fame-hover',when:view=>view.time_ms>=start+500});
    const before=await game.inspect();
    game.check('prepared terminal character loaded through native client',before.view.character.level===20&&before.view.character.fame===2000);
    await page.call('Input.dispatchMouseEvent',{type:'mouseMoved',x:rect.x+300,y:rect.y+300});
    await game.act({keys:['d'],duration_ms:300});
    await game.act({keys:[],duration_ms:800},{name:'terminal-fame-unhover',when:()=>true});
    const after=await game.inspect();
    await writeFile(join(process.env.QA_RESULTS_DIR,'terminal-observations.json'),JSON.stringify({before,after},null,2));
    game.check('terminal character remains alive and native movement works',after.view.lifecycle.alive&&after.view.position.x>before.view.position.x);
    game.check('terminal Fame retained without inventing XP',after.view.character.fame===2000&&after.view.character.experience===2000000);
  }finally{page.close();}
}
