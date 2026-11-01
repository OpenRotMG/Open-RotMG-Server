import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {connect} from '../../scripts/record-demo.mjs';

export const fixture={class:0x30e,level:20,seed:778,world:-1,position:{x:12.5,y:68.5},
  equipment:[0xb08,0xa2e,0xb05,-1],backpack:Array(8).fill(-1),stats:[670,385,75,25,50,75,40,60]};
const root=()=>process.argv[process.argv.indexOf('--run-dir')+1];
export async function enter({actor,account,browser,signal}){
  // Only the unloaded disposable starting record is prepared; the native kill must earn the crossing.
  const seeded=execFileSync('python3',['-c',`
import json,sqlite3,sys
with sqlite3.connect(sys.argv[1]) as db:
    row=db.execute('SELECT stats,alive FROM characters WHERE id=? AND account=?',(int(sys.argv[2]),int(sys.argv[3]))).fetchone()
    assert row and row[1]==1
    ch=json.loads(row[0])
    assert (ch['class'],ch['level'],ch['exp'],ch['fame'])==(782,20,18050,18)
    assert db.execute('SELECT count(*) FROM deaths').fetchone()[0]==0
    ch.update(exp=19999,fame=19)
    assert db.execute('UPDATE characters SET stats=? WHERE id=? AND account=?',(json.dumps(ch),int(sys.argv[2]),int(sys.argv[3]))).rowcount==1
    print(json.dumps({'actor':int(sys.argv[2]),'account':int(sys.argv[3]),'exp':19999,'fame':19,'prepared':True}))
`,join(root(),'state','game.db'),String(actor),String(account)],{encoding:'utf8',signal});
  await writeFile(join(process.env.QA_RESULTS_DIR,'class-quest-fixture.json'),seeded);
  const pages=await(await fetch(`http://127.0.0.1:${browser.port}/json/list`,{signal})).json();
  const target=pages.find(p=>p.id===browser.target);assert(target);
  const page=await connect({...browser,url:target.url,helper:join(process.env.PI_BROWSER_TOOLS,'direct-cdp.mjs')});
  try{
    await page.eval('(()=>{const m=document.querySelector("ruffle-player").shadowRoot.querySelector("#hardware-acceleration-modal");if(m&&!m.classList.contains("hidden"))m.querySelector(".close-modal").click();})()');
    const rect=await page.eval('(()=>{const r=document.querySelector("ruffle-player").getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height};})()');
    assert.equal(rect.width,800);assert.equal(rect.height,600);
    await page.call('Emulation.setFocusEmulationEnabled',{enabled:true});
    for(let n=0;n<2;n++){
      const p={x:rect.x+400,y:rect.y+550,button:'left',clickCount:1};
      try{await page.call('Input.dispatchMouseEvent',{type:'mousePressed',buttons:1,...p});}
      finally{await page.call('Input.dispatchMouseEvent',{type:'mouseReleased',buttons:0,...p});}
      await page.eval('new Promise(ok=>requestAnimationFrame(()=>requestAnimationFrame(ok)))');
    }
  }finally{page.close();}
}
export async function run(game){
  await game.act({keys:[],duration_ms:1800});
  const before=await game.inspect();
  game.check('prepared first class record starts at 19 Fame',before.view.character.fame===19&&before.view.lifecycle.alive);
  const boss=before.diagnostic.entities.find(e=>e.kind===0x6b1);assert(boss,'authored Evil Chicken God missing');
  await game.act({keys:[],duration_ms:800},{name:'class-quest-before-crossing',when:()=>true});
  const input={keys:[],duration_ms:800,target:{entity:boss.id,camera_angle:7*Math.PI/4,fire:true}};
  await game.act(input,{name:'class-quest-crossing',when:view=>view.character.fame>=20});
  const after=await game.inspect();
  await writeFile(join(process.env.QA_RESULTS_DIR,'class-quest-observations.json'),JSON.stringify({before,after},null,2));
  game.check('native Tutorial enemy defeat earns and commits the first Fame crossing',!after.diagnostic.entities.some(e=>e.id===boss.id)&&after.view.character.fame>=20&&after.view.character.experience>19999);
  game.check('same living native character retained',after.view.lifecycle.alive&&after.view.session.character===before.view.session.character);
}
