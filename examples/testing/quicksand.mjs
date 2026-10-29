import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
export const fixture = {
  class:0x300, level:20, seed:12351, world:'tomb-of-the-ancients', worldSeed:1, position:{x:63.1,y:16.5},
  equipment:[-1,-1,-1,-1], backpack:Array(8).fill(-1), stats:[700,100,15,25,75,15,15,10],
};
export async function run(game) {
  const dir=process.argv[process.argv.indexOf('--run-dir')+1],config=JSON.parse(await readFile(join(dir,'native.json'),'utf8'));
  const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+(await readFile(join(config.state,'control.token'),'utf8')).trim(),'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invincible',duration_ms:60000}}}),signal:AbortSignal.timeout(10000)});
  const protection=await response.json();assert(response.ok&&protection.status==='succeeded',JSON.stringify(protection));
  const ground=async()=>{
    const {view}=await game.inspect(),p=view.position;
    const tile=view.tiles.find(t=>t.x===Math.floor(p.x)&&t.y===Math.floor(p.y));
    game.check('standing tile was actually published',Boolean(tile));
    return tile.ground;
  };
  const before=await game.observe();
  game.check('prepared position is generated Tomb quicksand',before.world==='Tomb of the Ancients'&&await ground()===0xbf);
  await game.act({keys:[],duration_ms:5000});
  const sunk=await game.observe();
  game.check('sinking idle does not translate the player',Math.hypot(sunk.position.x-before.position.x,sunk.position.y-before.position.y)<0.05);
  await game.act({keys:['d','s'],duration_ms:1200},{name:'quicksand',when:v=>v.position.x>sunk.position.x+0.3});
  const slow=await game.observe();
  game.check('sunk native walk is slow and stays on quicksand',slow.position.x-sunk.position.x>0.7&&slow.position.x-sunk.position.x<1.5&&await ground()===0xbf);
  for(let i=0;i<12&&await ground()===0xbf;i++)await game.act({keys:['d','s'],duration_ms:400});
  const out=await game.observe();
  game.check('ordinary input exits quicksand onto dry ground',out.position.x>66&&await ground()!==0xbf);
  await game.act({keys:['d','s'],duration_ms:200});
  const dry=await game.observe();
  game.check('dry ground restores ordinary movement speed',dry.position.x-out.position.x>1.2);
  for(let i=0;i<12&&await ground()!==0xbf;i++)await game.act({keys:['a','w'],duration_ms:200});
  const reentered=await game.observe();
  game.check('ordinary input reenters generated quicksand',await ground()===0xbf);
  await game.act({keys:['a','w'],duration_ms:200});
  const fresh=await game.observe(),early=reentered.position.x-fresh.position.x;
  game.check('reentry resets sinking rather than retaining its cap',early>0.45&&await ground()===0xbf);
  await game.act({keys:[],duration_ms:5000});
  const capped=await game.observe();
  await game.act({keys:['d','s'],duration_ms:200});
  const late=(await game.observe()).position.x-capped.position.x;
  game.check('remaining on quicksand progressively slows movement again',late>0.1&&early>late*1.8&&await ground()===0xbf);
  await game.act({keys:['Insert'],duration_ms:500});
  const nexus=await game.observe();
  game.check('ordinary escape keeps the same living character',nexus.world==='Nexus'&&nexus.lifecycle.alive&&nexus.session.character===before.session.character);
  await game.act({keys:['d','s'],duration_ms:600});
  const resumed=await game.observe();
  game.check('movement resumes after world transfer without stale sinking',Math.hypot(resumed.position.x-nexus.position.x,resumed.position.y-nexus.position.y)>2);
}
