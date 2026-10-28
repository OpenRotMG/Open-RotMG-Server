import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {setTimeout as delay} from 'node:timers/promises';
import {connect} from '../../scripts/record-demo.mjs';

export const fixture={class:0x30e,level:20,seed:860,world:'tomb-of-the-ancients',worldSeed:12,position:'spawn',equipment:[0xb08,-1,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,75,25,50,75,40,60]};
export async function run(game){
  const out=process.env.QA_RESULTS_DIR,start=await game.inspect(),{worldSeed,...spec}=fixture;
  const peer=await game.join({...spec,world:start.view.session.world,position:{x:88.5,y:81.5}});
  await game.act({keys:[],duration_ms:3000});
  const target=await peer.inspect(),before=await game.inspect();
  const config=JSON.parse(await readFile(join(out,'native/native.json'),'utf8')),browser=config.native.find(n=>n.actor===game.actor);
  const connection=await connect({...browser,url:config.origin+'/',helper:config.browserTools});
  try{
    const action=game.act({keys:[],duration_ms:2500},{name:'tomb-player-teleport',when:v=>v.game_ms>=before.view.time_ms+2000});
    const typing=(async()=>{
      for(let n=0;(await game.observe()).paused;n++){assert(n<250,'Native action did not unpause');await delay(20);}
      async function enter(){try{await connection.call('Input.dispatchKeyEvent',{type:'keyDown',key:'Enter',code:'Enter',windowsVirtualKeyCode:13});}finally{await connection.call('Input.dispatchKeyEvent',{type:'keyUp',key:'Enter',code:'Enter',windowsVirtualKeyCode:13});}}
      await enter();await connection.call('Input.insertText',{text:'/teleport '+target.diagnostic.character.name});await enter();
    })();
    const results=await Promise.allSettled([action,typing]);for(const r of results)if(r.status==='rejected')throw r.reason;
    const after=await game.inspect();await writeFile(join(out,'teleport.json'),JSON.stringify({before,target,after}));
    game.check('ordinary native chat teleports to the observed Tomb peer',Math.hypot(after.view.position.x-target.view.position.x,after.view.position.y-target.view.position.y)<.2);
    game.check('Tomb MAPINFO advertises player teleporting',before.view.map.allow_teleport);
    game.check('teleport preserves the same living character and does not clear Tomb',after.view.session.character===before.view.session.character&&after.view.session.world===before.view.session.world&&after.view.lifecycle.alive&&after.diagnostic.character.tombs===0);
  }finally{connection.close();}
}
