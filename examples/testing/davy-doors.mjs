import {writeFile} from 'node:fs/promises';
import {join} from 'node:path';

export const realmSeed = 863;
export const fixture = {
  class:0x30e, level:1, seed:863, world:'davy-jones-locker', worldSeed:863,
  position:'spawn', equipment:[-1,-1,-1,-1], backpack:Array(8).fill(-1), stats:null,
};

export async function run(game) {
  const before = await game.inspect(), view = before.view;
  const doors = view.objects.filter(e=>e.kind===0x186a||e.kind===0x186b);
  game.check('fresh Locker arrival sees both closed purple door halves',doors.length===2);
  const [a,b] = doors.map(e=>e.position), middle = {x:(a.x+b.x)/2,y:(a.y+b.y)/2};
  const target = a.x===b.x
    ? {x:Math.floor(a.x)+Math.sign(middle.x-view.position.x),y:Math.floor(a.y)}
    : {x:Math.floor(a.x),y:Math.floor(a.y)+Math.sign(middle.y-view.position.y)};
  const discovered = v=>v.tiles.some(t=>t.x===target.x&&t.y===target.y);
  await game.act({keys:[],duration_ms:800},{name:'davy-closed-purple-door',when:()=>true});
  const after = await game.inspect();
  await writeFile(join(process.env.QA_RESULTS_DIR,'davy-door-observations.json'),JSON.stringify({target,before,after},null,2));
  game.check('same living character remains in original Locker',after.view.lifecycle.alive&&after.view.session.character===view.session.character&&after.view.session.world===view.session.world);
  game.check('idle observation did not collect keys or open doors',doors.every(door=>after.view.objects.some(e=>e.id===door.id)));
  game.check('closed purple door withholds the first hallway tile',!discovered(view)&&!discovered(after.view));
}
