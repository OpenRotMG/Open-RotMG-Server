import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {connect} from '../../scripts/record-demo.mjs';

const out=()=>join(process.env.QA_RESULTS_DIR,'native');
export const fixture={class:0x30e,level:1,seed:1098,world:-2,position:'spawn',
  equipment:[-1,-1,-1,-1],backpack:Array(8).fill(-1),stats:null};
export async function enter({actor,account,browser}){
  const prepared=execFileSync('python3',['-c',`
import sqlite3,json,sys
c=sqlite3.connect(sys.argv[1]+'/state/game.db');actor,account=map(int,sys.argv[2:])
c.execute('PRAGMA foreign_keys=ON');c.execute('BEGIN IMMEDIATE')
raw,owner,alive=c.execute('SELECT stats,account,alive FROM characters WHERE id=?',(actor,)).fetchone()
assert owner==account and alive==1
ch=json.loads(raw);ch.update(id=actor+1,alive=False,hp=0)
assert ch['fame']==0
receipt={'policy':'legacy_paid','version':1,'base':0,'rows':[],'total':0}
c.execute('INSERT INTO characters(id,account,stats,items,revision,alive) VALUES(?,?,?,?,0,0)',(actor+1,account,json.dumps(ch),json.dumps(ch['items'])))
c.execute('INSERT INTO deaths(character,snapshot,killed_by,equipment_fame,receipt) VALUES(?,?,?,0,?)',(actor+1,json.dumps(ch),'prepared historical fixture',json.dumps(receipt)))
assert c.execute('SELECT stats FROM characters WHERE id=?',(actor,)).fetchone()[0]==raw
assert not c.execute('PRAGMA foreign_key_check').fetchall()
c.commit();print(json.dumps({'prepared':True,'earned_progression':False,'account':account,'live_character':actor,'archived_character':actor+1,'total_fame':0}))
`,out(),String(actor),String(account)],{encoding:'utf8'});
  await writeFile(join(out(),'glow-fixture.json'),prepared);
  const config=JSON.parse(await readFile(join(out(),'result.json'),'utf8'));
  const c=await connect({...browser,url:config.origin+'/',helper:join(process.env.PI_BROWSER_TOOLS,'direct-cdp.mjs')});
  try{
    await c.call('Emulation.setFocusEmulationEnabled',{enabled:true});
    await c.eval('(()=>{const m=document.querySelector("ruffle-player").shadowRoot.querySelector("#hardware-acceleration-modal");if(m&&!m.classList.contains("hidden"))m.querySelector(".close-modal").click();})()');
    const r=await c.eval('(()=>{const r=document.querySelector("ruffle-player").getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height};})()');
    assert(r.width===800&&r.height===600);
    await writeFile(join(out(),'canvas-rect.json'),JSON.stringify(r));
    for(let i=0;i<3;i++){
      const p={x:r.x+400,y:r.y+550,button:'left',clickCount:1};
      try{await c.call('Input.dispatchMouseEvent',{type:'mousePressed',buttons:1,...p});}finally{await c.call('Input.dispatchMouseEvent',{type:'mouseReleased',buttons:0,...p});}
      await c.eval('new Promise(ok=>requestAnimationFrame(()=>requestAnimationFrame(ok)))');
    }
  }finally{c.close();}
}
export async function run(game){
  const before=await game.inspect();
  const config=JSON.parse(await readFile(join(out(),'native.json'),'utf8'));
  for(const span of ['week','month','all']){
    const response=await fetch(config.origin+'/legacy-api/fame/list',{method:'POST',body:new URLSearchParams({timespan:span,accountId:'-1',charId:'-1'})});
    assert(response.ok);const xml=await response.text();
    game.check(span+' board ranks the prepared archived life on the living account',xml.includes(`<FameListElem accountId="${before.view.session.account}"`));
  }
  await game.act({keys:[],duration_ms:1600},{name:'legend-glow',when:()=>true});
  const after=await game.inspect();
  game.check('read-only glow fixture retains living character and inventory',after.view.lifecycle.alive&&after.view.session.character===before.view.session.character&&JSON.stringify(after.view.character.items)===JSON.stringify(before.view.character.items));
  const pixels=JSON.parse(execFileSync('python3',['-c',`
import sys,json
from PIL import Image
r=json.load(open(sys.argv[2]));im=Image.open(sys.argv[1]).convert('RGB')
box=(int(r['x'])+260,int(r['y'])+250,int(r['x'])+340,int(r['y'])+325)
red=sum(r>100 and g<r*.3 and b<r*.3 for r,g,b in im.crop(box).getdata())
print(json.dumps({'player_crop':box,'red_pixels':red}))
`,join(out(),'legend-glow.png'),join(out(),'canvas-rect.json')],{encoding:'utf8'}));
  await writeFile(join(out(),'glow-observations.json'),JSON.stringify({before,after,pixels},null,2));
  game.check('selected native client renders a red outline on the ranked account',pixels.red_pixels>12);
}
