import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {execFileSync} from 'node:child_process';
import {join} from 'node:path';
import {connect} from '../../scripts/record-demo.mjs';

const out=()=>join(process.env.QA_RESULTS_DIR,'native');
export const fixture={class:0x30e,level:20,seed:903,world:-2,position:{x:72,y:65.5},
  equipment:[0xa97,0xa2e,-1,-1],backpack:Array(8).fill(-1),stats:null};
export async function enter({actor,account,browser}){
  const prepared=execFileSync('python3',['-c',`
import sqlite3,json,sys,xml.etree.ElementTree as E
c=sqlite3.connect(sys.argv[1]+'/state/game.db');actor,account=map(int,sys.argv[2:])
c.execute('PRAGMA foreign_keys=ON');c.execute('BEGIN IMMEDIATE')
assert c.execute('SELECT count(*) FROM accounts').fetchone()[0]==1
raw,owner,alive=c.execute('SELECT stats,account,alive FROM characters WHERE id=?',(actor,)).fetchone()
assert owner==account and alive==1
assert c.execute('SELECT credits FROM accounts WHERE id=?',(account,)).fetchone()[0]==100
classes=[int(o.get('type'),16) for o in E.parse('data/objects.xml').getroot().findall('Object') if o.findtext('Class')=='Player']
assert len(classes)==13
for i,kind in enumerate(classes,actor+1):
 ch=json.loads(raw);ch.update(id=i,**{'class':kind},exp=20000,fame=20,alive=False,hp=0,items=[-1]*12)
 c.execute('INSERT INTO characters(id,account,stats,items,revision,alive) VALUES(?,?,?,?,0,0)',(i,account,json.dumps(ch),json.dumps([-1]*12)))
assert c.execute('SELECT stats FROM characters WHERE id=?',(actor,)).fetchone()[0]==raw
assert not c.execute('PRAGMA foreign_key_check').fetchall()
c.commit();c.close();print(json.dumps({'prepared_deleted_history':True,'account':account,'actor':actor,'classes':classes,'gold_before_load':100,'earned_progression':False}))
`,out(),String(actor),String(account)],{encoding:'utf8'});
  await writeFile(join(out(),'rank-fixture.json'),prepared);
  const config=JSON.parse(await readFile(join(out(),'result.json'),'utf8'));
  const c=await connect({...browser,url:config.origin+'/',helper:join(process.env.PI_BROWSER_TOOLS,'direct-cdp.mjs')});
  try{
    await c.call('Emulation.setFocusEmulationEnabled',{enabled:true});
    await c.eval('(()=>{const m=document.querySelector("ruffle-player").shadowRoot.querySelector("#hardware-acceleration-modal");if(m&&!m.classList.contains("hidden"))m.querySelector(".close-modal").click();})()');
    const r=await c.eval('(()=>{const r=document.querySelector("ruffle-player").getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height};})()');
    assert(r.width===800&&r.height===600);
    for(let i=0;i<3;i++){
      const p={x:r.x+400,y:r.y+550,button:'left',clickCount:1};
      try{await c.call('Input.dispatchMouseEvent',{type:'mousePressed',buttons:1,...p});}finally{await c.call('Input.dispatchMouseEvent',{type:'mouseReleased',buttons:0,...p});}
      await c.eval('new Promise(ok=>requestAnimationFrame(()=>requestAnimationFrame(ok)))');
    }
  }finally{c.close();}
}
export async function run(game){
  const config=JSON.parse(await readFile(join(out(),'native.json'),'utf8'));
  const result=JSON.parse(await readFile(join(out(),'result.json'),'utf8'));
  let reply;const dialogs=[];
  const c=await connect({...result.browser,url:config.origin+'/',helper:config.browserTools,onEvent:event=>{
    if(event.method==='Page.javascriptDialogOpening'){
      dialogs.push(event.params);reply=c.call('Page.handleJavaScriptDialog',{accept:false});
    }
  }});
  const wallet=()=>JSON.parse(execFileSync('python3',['-c',`
import sqlite3,json,sys
c=sqlite3.connect('file:'+sys.argv[1]+'/state/game.db?mode=ro',uri=True)
print(json.dumps({'wallet':c.execute('SELECT credits,balance,earned FROM accounts JOIN account_fame ON account=id WHERE id=?',(int(sys.argv[2]),)).fetchone(),'rewards':c.execute('SELECT reward,notified_at FROM gold_rewards WHERE account=?',(int(sys.argv[2]),)).fetchall()}))
`,out(),String(result.account)],{encoding:'utf8'}));
  try{
    const before=await game.inspect();
    // Selected 123.5.1 keeps living Fame separate from the account wallet: https://github.com/OpenRotMG/Open-RotMG-Client/blob/39acbfbf3bf545c397d6ac94f0a9430d6d660ac2/client.swf
    game.check('prepared living Fame is not spendable account Fame',before.view.character.fame===18&&JSON.stringify(wallet().wallet)==='[175,0,0]');
    await game.act({keys:[],duration_ms:1500},{name:'wallet-rewarded',when:()=>true});
    const initial=wallet();
    game.check('blue rank pays and notifies once on native load',initial.rewards.length===1&&initial.rewards[0][0]==='class_star_blue'&&initial.rewards[0][1]>0);
    await c.call('Page.enable');
    await game.act({keys:[],duration_ms:800,mouse:{x:700,y:576}});
    await reply;
    game.check('Get Gold invokes the local guide confirmation',dialogs.length===1&&dialogs[0].type==='confirm'&&dialogs[0].message.includes(config.origin+'/gold/'));
    const after=await game.inspect();
    game.check('cancelling Gold navigation retains inventory and living identity',after.view.lifecycle.alive&&after.view.session.character===before.view.session.character&&JSON.stringify(after.view.character.items)===JSON.stringify(before.view.character.items)&&after.view.character.inventory_revision===before.view.character.inventory_revision);
    game.check('cancelling Gold navigation does not debit or regrant either wallet',JSON.stringify(wallet())===JSON.stringify(initial));
    await game.act({keys:[],duration_ms:1200},{name:'wallet-cancelled',when:()=>true});
    await writeFile(join(out(),'wallets.json'),JSON.stringify({before,after,initial,final:wallet(),dialogs},null,2));
  }finally{c.close();}
}
