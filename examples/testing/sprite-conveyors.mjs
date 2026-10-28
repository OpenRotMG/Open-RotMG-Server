import assert from 'node:assert/strict';
import {writeFile} from 'node:fs/promises';
import {join} from 'node:path';
export const fixture = {
  class:0x31e, level:20, seed:12351, world:'sprite-world', worldSeed:1, position:'spawn',
  equipment:[0xb0b,0xb22,0xafc,0xacd], backpack:Array(8).fill(-1),
  stats:[770,252,50,40,50,50,75,50],
};
export async function run(game) {
  const before=await game.observe();
  await game.act({keys:[],duration_ms:600});
  const idle=await game.observe();
  game.check('white entrance remains stationary',Math.hypot(idle.position.x-before.position.x,idle.position.y-before.position.y)<0.05);
  const detail=await game.inspect(), tiles=detail.view.tiles;
  const at=(x,y)=>tiles.find(t=>t.x===x&&t.y===y)?.ground;
  const samples=[];
  for(const [axis,choices] of [['x',[[0xa1,-1,0],[0xa2,1,0]]],['y',[[0xa3,0,-1],[0xa4,0,1]]]]) {
    let selected;
    for(const [kind,dx,dy] of choices) {
      const tile=tiles.find(t=>t.ground===kind&&at(t.x+dx,t.y+dy)===kind&&at(t.x+2*dx,t.y+2*dy)===kind&&!detail.view.objects.some(o=>Math.hypot(o.position.x-t.x-.5,o.position.y-t.y-.5)<2));
      if(tile){selected={tile,kind,dx,dy};break;}
    }
    assert(selected,'seeded entrance must expose a clear conveyor on '+axis);
    const {tile,kind,dx,dy}=selected;
    const peer=await game.join({...fixture,world:before.session.world,worldSeed:undefined,seed:12352+samples.length,position:{x:tile.x+.5,y:tile.y+.5}});
    // A fresh player's relative vector is null, so prime controls before idle Push: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Rotmg123.5.1/src/com/company/assembleegameclient/objects/Player.as#L183-L191
    await peer.act({keys:['a','d'],duration_ms:50});
    const start=await peer.observe();
    await peer.act({keys:[],duration_ms:400},{name:'sprite-push-'+axis,when:v=>Math.hypot(v.position.x-start.position.x,v.position.y-start.position.y)>.1});
    const end=await peer.observe();
    const along=(end.position.x-start.position.x)*dx+(end.position.y-start.position.y)*dy;
    const across=(end.position.x-start.position.x)*dy-(end.position.y-start.position.y)*dx;
    samples.push({kind,start,end,along,across});
    game.check('idle conveyor pushes on '+axis,along>.5&&along<(end.game_ms-start.game_ms)*.0025+.5&&Math.abs(across)<.05);
    game.check('conveyor preserves living character on '+axis,end.lifecycle.alive&&end.session.character===start.session.character);
  }
  await writeFile(join(process.env.QA_RESULTS_DIR,'sprite-push.json'),JSON.stringify(samples,null,2));
}
