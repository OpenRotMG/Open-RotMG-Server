import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';

export const fixture={class:0x30e,level:20,seed:1051,world:'spider-den',position:'spawn',equipment:[0xb08,-1,0xb05,0xac5],backpack:Array(8).fill(-1),stats:[670,385,75,25,50,75,40,60]};
export async function run(game){
  const start=await game.inspect();
  const report=JSON.parse(await readFile(join(process.env.QA_RESULTS_DIR,'native/result.json'),'utf8')),token=(await readFile(join(process.env.QA_RESULTS_DIR,'native/state/control.token'),'utf8')).trim();
  const m=await (await fetch(report.origin+'/dev/map?player='+start.view.session.player,{headers:{Authorization:'Bearer '+token}})).json(),w=m.width,blocked=new Set(m.blocked.map(([x,y])=>y*w+x));
  const from=Math.floor(start.view.position.y)*w+Math.floor(start.view.position.x),seen=new Set([from]),queue=[from];let spot=null;
  for(let i=0;i<queue.length&&!spot;i++){const c={x:queue[i]%w+.5,y:Math.floor(queue[i]/w)+.5};if(Math.hypot(c.x-start.view.position.x,c.y-start.view.position.y)>=4)spot=c;for(const n of [queue[i]-1,queue[i]+1,queue[i]-w,queue[i]+w])if(!seen.has(n)&&!blocked.has(n)){seen.add(n);queue.push(n);}}
  const peer=await game.join({...fixture,world:start.view.session.world,position:spot});
  const target=await peer.inspect();
  game.check('two living players begin apart in Spider Den',start.view.map.name==='Spider Den'&&target.view.session.world===start.view.session.world&&target.view.lifecycle.alive&&Math.hypot(start.view.position.x-target.view.position.x,start.view.position.y-target.view.position.y)>3);
  await game.act({keys:[],duration_ms:800});
  await game.act({keys:[],mouse:{x:650,y:520},duration_ms:800},{name:'spider-teleport-menu',when:()=>true});
  const before=await game.inspect();
  await writeFile(join(process.env.QA_RESULTS_DIR,'teleport-before.json'),JSON.stringify({start,target,before}));
  game.check('Spider Den MAPINFO enables native player teleport',before.view.map.allow_teleport);
  await game.act({keys:[],mouse:{x:550,y:400},duration_ms:1000},{name:'spider-teleport-arrival',when:view=>Math.hypot(view.position.x-target.view.position.x,view.position.y-target.view.position.y)<.2});
  const arrived=await game.inspect();
  game.check('native menu teleport reaches the same living Spider Den peer',arrived.view.session.world===start.view.session.world&&arrived.view.session.character===start.view.session.character&&arrived.view.lifecycle.alive&&Math.hypot(arrived.view.position.x-target.view.position.x,arrived.view.position.y-target.view.position.y)<.2);
  game.check('successful native teleport records lifetime use',arrived.diagnostic.character.lifetime.teleported);
  await game.act({keys:['a'],duration_ms:200});
  const moved=await game.observe();
  game.check('native movement resumes after GOTO acknowledgement',Math.hypot(moved.position.x-arrived.view.position.x,moved.position.y-arrived.view.position.y)>.3);
  await game.act({keys:[],mouse:{x:650,y:520},duration_ms:400});
  await game.act({keys:[],mouse:{x:550,y:400},duration_ms:400});
  const limited=await game.inspect();
  await writeFile(join(process.env.QA_RESULTS_DIR,'teleport-after.json'),JSON.stringify({arrived,moved,limited,peer:await peer.inspect()}));
  game.check('native cooldown refuses an immediate repeat without movement',limited.view.time_ms-arrived.view.time_ms<10000&&Math.hypot(limited.view.position.x-moved.position.x,limited.view.position.y-moved.position.y)<.2);
  game.check('teleport preserves living identity and inventory',limited.view.lifecycle.alive&&limited.view.session.character===start.view.session.character&&JSON.stringify(limited.view.character.items)===JSON.stringify(start.view.character.items));
}
