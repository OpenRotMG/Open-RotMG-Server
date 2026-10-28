import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {setTimeout as delay} from 'node:timers/promises';
import {connect} from '../../scripts/record-demo.mjs';

const exec=promisify(execFile),root=join(process.env.QA_RESULTS_DIR,'native');
let c,account;
export async function beforeNavigate({browser}) {
  c=await connect({...browser,url:'about:blank',helper:join(process.env.PI_BROWSER_TOOLS,'direct-cdp.mjs')});
  await c.call('Emulation.setFocusEmulationEnabled',{enabled:true});
}
export const fixture={class:0x30e,level:1,seed:900,world:-2,position:{x:73.9,y:74.5},equipment:[0xa97,0xa2e,-1,-1],backpack:Array(8).fill(-1),stats:null};
async function db(sql) {
  const {stdout}=await exec('python3',['-c','import sqlite3,json,sys; c=sqlite3.connect(sys.argv[1]); r=c.execute(sys.argv[2],[int(sys.argv[3])]); print(json.dumps(r.fetchall())); c.commit()',join(root,'state/game.db'),sql,String(account)]);
  return JSON.parse(stdout);
}
async function click(x,y) {
  const r=await c.eval('(()=>{const r=document.querySelector("ruffle-player").getBoundingClientRect();return{x:r.x,y:r.y};})()');
  const p={x:r.x+x,y:r.y+y,button:'left',clickCount:1};
  await c.call('Input.dispatchMouseEvent',{type:'mouseMoved',...p});
  try {await c.call('Input.dispatchMouseEvent',{type:'mousePressed',buttons:1,...p});}
  finally {await c.call('Input.dispatchMouseEvent',{type:'mouseReleased',buttons:0,...p});}
}
async function type(x,y,text) {
  await click(x,y);
  for(const letter of text)await c.call('Input.dispatchKeyEvent',{type:'char',text:letter});
}
export async function run(game) {
  account=JSON.parse(await readFile(join(root,'result.json'),'utf8')).account;
  const ui=async(name,work)=>{
    let finished=false;
    const action=game.act({keys:[],duration_ms:6000},{name,when:()=>finished});
    try {
      for(let i=0;;i++) {
        if(!(await game.observe()).paused&&await c.eval('document.querySelector("ruffle-player").ruffle().isPlaying'))break;
        if(i===100)throw Error('Native UI action did not start');
        await delay(20);
      }
      await work();await delay(300);finished=true;
    } finally {await action;}
  };
  const original=await game.observe();
  await ui('guest-name-panel',()=>delay(2000));
  await ui('guest-name-refusal',()=>click(700,580));
  game.check('guest chooser does not alter identity or wallet',JSON.stringify(await db('SELECT name,credits FROM accounts WHERE id=?'))===JSON.stringify([[`Guest${account}`,100]]));
  await ui('register-dialog',()=>click(475,330));
  await ui('registered',async()=>{await type(300,170,'name-choice@example.test');await type(300,258,'test-password');await type(300,346,'test-password');await click(288,396);await click(484,467);});
  game.check('normal client registration persists on the same account',(await db('SELECT count(*) FROM credentials WHERE account=?'))[0][0]===1);
  game.check('registration preserves the living prepared character',(await game.observe()).session.character===original.session.character);
  await db('UPDATE accounts SET credits=1000 WHERE id=?');
  await ui('first-name-dialog',()=>click(700,580));
  await ui('short-name-refusal',async()=>{await type(300,255,'Ab');await click(480,380);});
  game.check('short-name refusal preserves the unnamed account and prepared wallet',JSON.stringify(await db('SELECT name,credits FROM accounts WHERE id=?'))===JSON.stringify([[`Guest${account}`,1000]]));
  async function replace(text) {
    await click(300,255);
    await c.call('Input.dispatchKeyEvent',{type:'keyDown',key:'End',code:'End',windowsVirtualKeyCode:35});
    await c.call('Input.dispatchKeyEvent',{type:'keyUp',key:'End',code:'End',windowsVirtualKeyCode:35});
    for(let i=0;i<12;i++) {
      await c.call('Input.dispatchKeyEvent',{type:'keyDown',key:'Backspace',code:'Backspace',windowsVirtualKeyCode:8});
      await c.call('Input.dispatchKeyEvent',{type:'keyUp',key:'Backspace',code:'Backspace',windowsVirtualKeyCode:8});
    }
    for(const letter of text)await c.call('Input.dispatchKeyEvent',{type:'char',text:letter});
    await click(480,380);
  }
  await ui('free-name-chosen',()=>replace('FirstNamesEXTRA123'));
  game.check('first choice is free and the actual client caps input at ten letters',JSON.stringify(await db('SELECT name,credits FROM accounts WHERE id=?'))===JSON.stringify([['FirstNames',1000]]));
  game.check('first choice publishes the account name to the live player',(await game.inspect()).diagnostic.character.name==='FirstNames');
  await walk(72.5,74.5);await walk(73.9,74.5);
  await ui('paid-name-offer',async()=>{});
  await ui('paid-name-dialog',()=>click(700,580));
  await ui('duplicate-name-refusal',async()=>{await type(300,255,'firstnames');await click(480,380);});
  game.check('case-insensitive native duplicate refusal preserves name and Gold',JSON.stringify(await db('SELECT name,credits FROM accounts WHERE id=?'))===JSON.stringify([['FirstNames',1000]]));
  await ui('paid-name-chosen',()=>replace('NextName'));
  game.check('accepted native rename debits exactly 1000 Gold',JSON.stringify(await db('SELECT name,credits FROM accounts WHERE id=?'))===JSON.stringify([['NextName',0]]));
  game.check('paid rename publishes the new player identity',(await game.inspect()).diagnostic.character.name==='NextName');
  await walk(72.5,74.5);await walk(73.9,74.5);
  await ui('unfunded-name-dialog',()=>click(700,580));
  await ui('unfunded-name-refusal',async()=>{await type(300,255,'ThirdName');await click(480,380);});
  game.check('unfunded native rename preserves identity and zero wallet',JSON.stringify(await db('SELECT name,credits FROM accounts WHERE id=?'))===JSON.stringify([['NextName',0]]));
  await ui('cancel-name-change',()=>click(390,380));
  async function walk(x,y) {
    for(let i=0;i<40;i++) {
      const p=(await game.observe()).position,dx=x-p.x,dy=y-p.y,distance=Math.hypot(dx,dy);
      if(distance<.2)return;
      const sx=(dx-dy)/Math.SQRT2,sy=(dx+dy)/Math.SQRT2,keys=[];
      if(Math.abs(sx)>.1)keys.push(sx>0?'d':'a');
      if(Math.abs(sy)>.1)keys.push(sy>0?'s':'w');
      await game.act({keys,duration_ms:Math.min(300,Math.max(50,Math.round(distance*100)))});
    }
    throw Error('Native walk did not reach the Vault route');
  }
  await walk(73.9,75.5);await walk(79,77.5);
  await game.act({keys:[],mouse:{x:700,y:575},duration_ms:1000});
  for(let i=0;i<8&&(await game.observe()).world!=='Vault';i++)await game.act({keys:[],duration_ms:500});
  game.check('ordinary Vault entry reconnects the same living character',(await game.observe()).world==='Vault'&&(await game.observe()).session.character===original.session.character);
  await game.act({keys:['Insert'],duration_ms:1000});
  for(let i=0;i<8&&(await game.observe()).world!=='Nexus';i++)await game.act({keys:[],duration_ms:500});
  await game.act({keys:[],duration_ms:3000},{name:'renamed-after-reconnect',when:v=>v.world==='Nexus'&&v.game_ms>original.game_ms+1000});
  const final=await game.observe(),identity=(await game.inspect()).diagnostic.character.name;
  game.check('normal reconnect retains renamed account, wallet, character and inventory',final.world==='Nexus'&&final.lifecycle.alive&&final.session.account===account&&final.session.character===original.session.character&&identity==='NextName'&&JSON.stringify(final.character.items)===JSON.stringify(original.character.items)&&JSON.stringify(await db('SELECT name,credits FROM accounts WHERE id=?'))===JSON.stringify([['NextName',0]]));
  await writeFile(join(root,'name-choice.json'),JSON.stringify({preparedWallet:1000,original,final,identity},null,2));
  c.close();
}
