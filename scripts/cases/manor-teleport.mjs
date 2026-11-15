import assert from 'node:assert/strict';
import {writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {setTimeout as delay} from 'node:timers/promises';
import {connect} from '../../scripts/record-demo.mjs';

const clients=[];
export async function beforeNavigate({browser}){
  clients.push(await connect({...browser,url:'about:blank',helper:join(process.env.PI_BROWSER_TOOLS,'direct-cdp.mjs')}));
}
export const fixture={class:0x30e,level:20,seed:1059,world:'manor-of-the-immortals',worldSeed:12,position:'spawn',equipment:[0xb08,-1,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,75,25,50,75,40,60]};
export async function run(game){
  try{
    const start=await game.inspect(),{worldSeed,...spec}=fixture;
    const peer=await game.join({...spec,world:start.view.session.world,position:{x:start.view.position.x+4,y:start.view.position.y}});
    const target=await peer.inspect(),distance=(a,b)=>Math.hypot(a.position.x-b.position.x,a.position.y-b.position.y);
    game.check('two living native players share the prepared Manor entrance',start.view.map.name==='Manor of the Immortals'&&target.view.session.world===start.view.session.world&&start.view.lifecycle.alive&&target.view.lifecycle.alive);
    await game.act({keys:[],duration_ms:2000},{name:'manor-before-teleport',when:v=>v.game_ms>=start.view.time_ms+1800});
    const before=await game.inspect();
    const action=game.act({keys:[],duration_ms:1900},{name:'manor-teleport-result',when:v=>v.game_ms>=before.view.time_ms+1600});
    const typing=(async()=>{
      for(let n=0;(await game.observe()).paused;n++){assert(n<250,'Native action did not unpause');await delay(20);}
      async function enter(){try{await clients[0].call('Input.dispatchKeyEvent',{type:'keyDown',key:'Enter',code:'Enter',windowsVirtualKeyCode:13});}finally{await clients[0].call('Input.dispatchKeyEvent',{type:'keyUp',key:'Enter',code:'Enter',windowsVirtualKeyCode:13});}}
      await enter();await clients[0].call('Input.insertText',{text:'/teleport '+target.diagnostic.character.name});await enter();
    })();
    const results=await Promise.allSettled([action,typing]);for(const r of results)if(r.status==='rejected')throw r.reason;
    const after=await game.inspect();await writeFile(join(process.env.QA_RESULTS_DIR,'manor-teleport.json'),JSON.stringify({start,target,before,after}));
    game.check('Manor MAPINFO advertises player teleport',after.view.map.allow_teleport);
    game.check('ordinary native chat teleport reaches the observed Manor peer',distance(after.view,target.view)<.2);
    await game.act({keys:['a','s'],duration_ms:800});
    const moved=await game.observe(),other=await peer.inspect();
    await writeFile(join(process.env.QA_RESULTS_DIR,'manor-post-teleport.json'),JSON.stringify({moved,other}));
    game.check('native movement resumes after the teleport acknowledgement',distance(moved,after.view)>.5);
    game.check('teleport preserves both living identities and inventories without Manor completion',moved.lifecycle.alive&&other.view.lifecycle.alive&&moved.session.character===start.view.session.character&&other.view.session.character===target.view.session.character&&JSON.stringify(moved.character.items)===JSON.stringify(start.view.character.items)&&JSON.stringify(other.view.character.items)===JSON.stringify(target.view.character.items)&&after.diagnostic.character.manors===0);
  }finally{for(const client of clients)client.close();}
}
