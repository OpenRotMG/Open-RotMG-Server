import assert from 'node:assert/strict';
import {readFile,appendFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
const out=process.argv[process.argv.indexOf('--run-dir')+1];
const base=[670,385,75,25,50,75,40,60];
export const fixture={class:0x30e,level:20,seed:877,world:'snake-pit',worldSeed:12351,position:'spawn',equipment:[-1,-1,-1,-1],backpack:[0xc31,0xc20,-1,-1,-1,-1,-1,-1],stats:base};
const bonus=(...pairs)=>{const b=Array(8).fill(0);for(const [i,n] of pairs)b[i]=n;return b;};
const rings=[
  [0xc31,bonus([0,60],[1,60],[3,4],[6,4])],[0xc20,bonus([6,5],[7,5])],
  [0xa41,bonus([2,3],[4,3])],[0x708,bonus([6,3],[7,3])],[0xa4f,bonus([3,1])],
  [0xba0,bonus([1,100],[5,4],[7,4])],[0xba1,bonus([0,60],[1,60],[4,4],[5,4])],
  [0xba2,bonus([0,100],[2,4],[3,4])],[0xc13,bonus([1,80],[4,3],[7,3])],
  [0xc17,bonus([5,3],[7,3])],[0xc27,bonus([0,40],[2,3],[5,3])],
];
for(const [tier,ids] of [[1,[0xa27,0xa28,0xa24,0xa25,0xa26,0xa4d,0xa36,0xa37]],[2,[0xa2c,0xa2d,0xa29,0xa2a,0xa2b,0xa4e,0xa38,0xa39]],[3,[0xabd,0xabe,0xab7,0xab8,0xab9,0xabc,0xaba,0xabb]],[4,[0xac5,0xac6,0xabf,0xac0,0xac1,0xac4,0xac2,0xac3]],[5,[0xacd,0xace,0xac7,0xac8,0xac9,0xacc,0xaca,0xacb]]]){
  ids.forEach((id,i)=>rings.push([id,bonus([i,i<2?[40,80,100,120,140][tier-1]:[3,5,6,7,8][tier-1]])]));
}
export async function run(game){
  const config=JSON.parse(await readFile(join(out,'native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const initial=await game.observe();
  const same=(a,b)=>JSON.stringify(a)===JSON.stringify(b);
  const stats=v=>['max_hp','max_mp','attack','defense','speed','dexterity','vitality','wisdom'].map(k=>v.character.stats[k]);
  const drag=async(from,to,capture)=>game.act({keys:[],duration_ms:450,drag:{from,to}},capture);
  const pack={x:634,y:428},ring={x:766,y:385};
  for(const [id,boost] of rings){
    let before=await game.observe();
    if(before.character.items[4]!==id){
      const r=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:game.actor,change:{kind:'item',slot:4,item:id}}}),signal:AbortSignal.timeout(10000)});
      const receipt=await r.json();assert(r.ok&&receipt.status==='succeeded',JSON.stringify(receipt));
      await game.act({keys:[],duration_ms:250});before=await game.observe();
    }
    game.check(id.toString(16)+' backpack grants no stats',same(stats(before),base)&&before.character.items[3]===-1);
    // The target buff is not the rolled-back prototype: https://wikiwiki.jp/rmd/test/build123.5.1
    await drag(pack,ring);
    const equipped=await game.observe();
    game.check(id.toString(16)+' native equip commits exact bonuses once',equipped.character.items[3]===id&&equipped.character.items[4]===-1&&equipped.character.inventory_revision===before.character.inventory_revision+1&&same(stats(equipped),base.map((n,i)=>n+boost[i])));
    if(id===0xc31||id===0xc20)await game.act({keys:[],duration_ms:1200},{name:'ring-'+id.toString(16),when:v=>v.game_ms>=equipped.game_ms+700});
    if(id===0xc31){
      await drag(ring,{x:634,y:385});const denied=await game.observe();
      game.check('ring refuses weapon slot atomically',same(denied.character.items,equipped.character.items)&&same(stats(denied),stats(equipped))&&denied.character.inventory_revision===equipped.character.inventory_revision);
    }
    await drag(ring,pack);const removed=await game.observe();
    game.check(id.toString(16)+' native removal reverses bonuses once',removed.character.items[3]===-1&&removed.character.items[4]===id&&removed.character.inventory_revision===equipped.character.inventory_revision+1&&same(stats(removed),base)&&removed.character.mp<=base[1]&&removed.character.hp<=base[0]);
    await appendFile(join(out,'rings.jsonl'),JSON.stringify({id,boost,before,equipped,removed})+'\n');
  }
  await drag(pack,ring);const beforeSwap=await game.observe();
  await drag({x:678,y:428},ring);const beforeExit=await game.observe();
  game.check('direct ring replacement removes old bonuses without stacking',beforeExit.character.items[3]===0xc20&&beforeExit.character.items[5]===0xacb&&beforeExit.character.inventory_revision===beforeSwap.character.inventory_revision+1&&same(stats(beforeExit),base.map((n,i)=>n+(i>=6?5:0))));
  await game.act({keys:['Insert'],duration_ms:1200},{name:'rings-retained',when:v=>v.world==='Nexus'});
  const after=await game.observe();
  game.check('Nexus reconnect preserves ring stats inventory and living identity',after.world==='Nexus'&&after.session.world!==beforeExit.session.world&&after.lifecycle.alive&&after.session.character===initial.session.character&&same(after.character.items,beforeExit.character.items)&&same(stats(after),stats(beforeExit))&&after.character.inventory_revision===beforeExit.character.inventory_revision);
}
