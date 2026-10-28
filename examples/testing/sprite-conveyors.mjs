import assert from 'node:assert/strict';
import {writeFile} from 'node:fs/promises';
import {join} from 'node:path';
export const fixture={class:0x31e,level:20,seed:12351,world:'sprite-world',worldSeed:1,position:'spawn',equipment:[0xb0b,0xb22,0xafc,0xacd],backpack:Array(8).fill(-1),stats:[770,252,50,40,50,50,75,50]};
export async function run(game){await verify(game,[[0xa1,-1,0],[0xa2,1,0]]);}
export async function verify(game,directions){
  const before=await game.observe();await game.act({keys:[],duration_ms:600});const idle=await game.observe();
  game.check('white entrance remains stationary',Math.hypot(idle.position.x-before.position.x,idle.position.y-before.position.y)<.05);
  const detail=await game.inspect(),tiles=detail.view.tiles,at=(x,y)=>tiles.find(t=>t.x===x&&t.y===y)?.ground,samples=[];
  for(const [kind,dx,dy] of directions){
    const tile=tiles.find(t=>t.ground===kind&&at(t.x+dx,t.y+dy)===kind&&!detail.view.objects.some(o=>Math.hypot(o.position.x-t.x-.5,o.position.y-t.y-.5)<1.5));
    assert(tile,'Seeded entrance must expose a clear conveyor '+kind.toString(16));
    const {worldSeed,...prepared}=fixture;
    const peer=await game.join({...prepared,world:before.session.world,seed:12352+samples.length,position:{x:tile.x+.5,y:tile.y+.5}});
    // A fresh relative vector is null until controls are primed: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Rotmg123.5.1/src/com/company/assembleegameclient/objects/Player.as#L183-L191
    await peer.act({keys:['a','d'],duration_ms:25});const start=await peer.observe();await peer.act({keys:[],duration_ms:100});const end=await peer.observe();
    const along=(end.position.x-start.position.x)*dx+(end.position.y-start.position.y)*dy,across=(end.position.x-start.position.x)*dy-(end.position.y-start.position.y)*dx;
    samples.push({kind,tile,start,end,along,across});await writeFile(join(process.env.QA_RESULTS_DIR,'sprite-push.json'),JSON.stringify(samples,null,2));
    game.check('idle conveyor pushes in selected direction '+kind.toString(16),along>.1&&along<(end.game_ms-start.game_ms)*.0025+.5&&Math.abs(across)<.05);
    game.check('conveyor preserves living character '+kind.toString(16),end.lifecycle.alive&&end.session.character===start.session.character);
    await peer.act({keys:[],duration_ms:100},{name:'sprite-push-'+kind.toString(16),when:()=>true});
  }
}
