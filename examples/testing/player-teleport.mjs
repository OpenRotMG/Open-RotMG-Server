import {connect} from '../../scripts/record-demo.mjs';
import {join} from 'node:path';
import {setTimeout as delay} from 'node:timers/promises';

const clients=[];
export async function beforeNavigate({browser}) {
  clients.push(await connect({...browser,url:'about:blank',helper:join(process.env.PI_BROWSER_TOOLS,'direct-cdp.mjs')}));
}
export const fixture={class:0x300,level:20,seed:910,world:-2,position:{x:76.5,y:72.5},equipment:[0xa14,0xa59,-1,-1],backpack:Array(8).fill(-1),stats:[670,252,50,25,50,50,40,50]};
export async function run(game) {
  const start=await game.observe();
  const peer=await game.join({...fixture,world:start.session.world,position:{x:78.5,y:72.5}});
  const originalPeer=await peer.observe(),targetName=(await peer.inspect()).diagnostic.character.name;
  const distance=(a,b)=>Math.hypot(a.position.x-b.position.x,a.position.y-b.position.y);
  async function chat(who,index,text) {
    const action=who.act({keys:[],duration_ms:1800});
    try {
      for(let i=0;;i++) {
        if(!(await who.observe()).paused&&await clients[index].eval('document.querySelector("ruffle-player").ruffle().isPlaying'))break;
        if(i===100)throw Error('Native chat action did not start');
        await delay(20);
      }
      const key=async(type)=>clients[index].call('Input.dispatchKeyEvent',{type,key:'Enter',code:'Enter',windowsVirtualKeyCode:13});
      await key('keyDown');await key('keyUp');
      for(const letter of text)await clients[index].call('Input.dispatchKeyEvent',{type:'char',text:letter});
      await key('keyDown');await key('keyUp');
    } finally {await action;}
  }
  const menu=()=>game.act({keys:[],mouse:{x:650,y:520},duration_ms:500});
  async function teleport(name) {
    await game.act({keys:[],mouse:{x:550,y:400},duration_ms:100});
    return game.act({keys:[],duration_ms:800},{name,when:()=>true});
  }
  await menu();
  await game.act({keys:[],mouse:{x:300,y:300},duration_ms:300});
  game.check('cancelled player menu leaves both positions unchanged',distance(await game.observe(),start)<.1&&distance(await peer.observe(),originalPeer)<.1);
  await chat(peer,1,'/pause');
  game.check('ordinary pause command marks the target paused',((await peer.inspect()).view.character.conditions&(1<<20))!==0);
  await chat(game,0,`/teleport ${targetName}`);
  await game.act({keys:[],duration_ms:800},{name:'paused-target-refusal',when:()=>true});
  game.check('paused target refusal leaves both players in place',distance(await game.observe(),start)<.1&&distance(await peer.observe(),originalPeer)<.1);
  await chat(peer,1,'/pause');
  await peer.act({keys:[' '],duration_ms:200});
  game.check('ordinary Rogue cloak is active',((await peer.inspect()).view.character.conditions&(1<<12))!==0);
  await chat(game,0,`/teleport ${targetName}`);
  await game.act({keys:[],duration_ms:800},{name:'invisible-target-refusal',when:()=>true});
  game.check('invisible target refusal leaves both players in place',distance(await game.observe(),start)<.1&&distance(await peer.observe(),originalPeer)<.1);
  const refused=await game.observe();
  await peer.act({keys:[],duration_ms:6000});
  await menu();await teleport('nexus-teleport-landed');
  const landed=await game.observe();
  game.check('eligible Nexus menu teleport lands exactly on the live target',distance(landed,await peer.observe())<.01&&landed.session.character===start.session.character&&landed.game_ms-refused.game_ms<10000);
  await peer.act({keys:[],duration_ms:800},{name:'target-observes-arrival',when:()=>true});
  await game.act({keys:['a'],duration_ms:200});
  const moved=await game.observe();
  game.check('native movement resumes after GOTO acknowledgement',distance(moved,landed)>.3);
  await menu();await teleport('successful-teleport-cooldown');
  game.check('immediate repeat is refused without moving either player',distance(await game.observe(),moved)<.1&&distance(await peer.observe(),originalPeer)<.1);
  await game.act({keys:[],duration_ms:10000});
  await menu();await teleport('teleport-after-cooldown');
  game.check('teleport succeeds again after ten active seconds',distance(await game.observe(),await peer.observe())<.01);
  for(const [x,y] of [[78.5,75.5],[79,77.5]]) {
    for(let i=0;;i++) {
      const p=(await game.observe()).position,dx=x-p.x,dy=y-p.y,d=Math.hypot(dx,dy);
      if(d<.2)break;
      if(i===40)throw Error('Native walk to Vault portal did not arrive');
      const sx=(dx-dy)/Math.SQRT2,sy=(dx+dy)/Math.SQRT2,keys=[];
      if(Math.abs(sx)>.1)keys.push(sx>0?'d':'a');
      if(Math.abs(sy)>.1)keys.push(sy>0?'s':'w');
      await game.act({keys,duration_ms:Math.min(300,Math.max(50,Math.round(d*100)))});
    }
  }
  await game.act({keys:[],mouse:{x:700,y:575},duration_ms:1000});
  for(let i=0;i<8&&(await game.observe()).world!=='Vault';i++)await game.act({keys:[],duration_ms:500});
  game.check('ordinary Vault entry reconnects only the sender',(await game.observe()).world==='Vault'&&(await peer.observe()).session.world===start.session.world);
  await game.act({keys:['Insert'],duration_ms:1000});
  for(let i=0;i<8&&(await game.observe()).world!=='Nexus';i++)await game.act({keys:[],duration_ms:500});
  await game.act({keys:[],duration_ms:1000},{name:'teleport-reconnected',when:()=>true});
  const final=await game.observe(),other=await peer.observe();
  game.check('reconnect preserves both living character identities',final.world==='Nexus'&&final.session.character===start.session.character&&other.session.character===originalPeer.session.character);
  game.check('both ordinary clients remain alive with unchanged inventory',final.lifecycle.alive&&other.lifecycle.alive&&JSON.stringify(final.character.items)===JSON.stringify(start.character.items)&&JSON.stringify(other.character.items)===JSON.stringify(originalPeer.character.items));
  for(const client of clients)client.close();
}
