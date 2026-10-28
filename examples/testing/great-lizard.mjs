import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
const spec={class:0x30e,level:20,seed:801,world:1,equipment:[0xa9f,0xa2e,0xb05,0xac7],backpack:Array(8).fill(-1),stats:[670,385,75,25,50,75,40,60]};
const root=process.argv[process.argv.indexOf('--run-dir')+1];
let lizard;
export async function prepare(api){
  const world=await api({op:'world_info',world:1}),[x,y]=world.spawn_points[0];
  const scout=await api({op:'spawn',spec:{...spec,guid:'lizard-scout',position:{x,y},controller:'direct',profile:null}});
  const state=await api({op:'inspect',actor:scout.result.actor});
  const report=JSON.parse(await readFile(join(root,'result.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(root,'state/control.token'),'utf8')).trim()};
  const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers});assert(response.ok);
  const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
  const natural=map.entities.filter(e=>e.kind===0x952);
  await writeFile(join(process.env.QA_RESULTS_DIR,'natural-lizards.json'),JSON.stringify(natural));
  lizard=natural.find(e=>Array.from({length:9},(_,i)=>i-4).every(dx=>Array.from({length:9},(_,i)=>i-4).every(dy=>!blocked.has([Math.floor(e.pos.x)+dx,Math.floor(e.pos.y)+dy].join(',')))));
  assert(lizard,'No naturally populated Great Lizard with a clear approach');
  return {...spec,position:{x:lizard.pos.x-2,y:lizard.pos.y}};
}
export async function run(game){
  const save=async name=>{const s=await game.inspect();await writeFile(join(process.env.QA_RESULTS_DIR,name+'.json'),JSON.stringify(s));return s;};
  const entity=s=>s.diagnostic.entities.find(e=>e.id===lizard.id);
  const begin=await save('arrival'),hp=begin.view.character.hp;
  game.check('natural Realm Great Lizard has selected 500 HP',entity(begin)?.hp===500);
  await game.act({keys:[],duration_ms:1500},{name:'lizard-arrival',when:()=>true});
  const exposed=await save('exposed');
  game.check('ordinary enemy fire damages the native Wizard',exposed.view.character.hp<hp);
  const path=join(process.env.QA_RESULTS_DIR,'protection.json');
  await writeFile(path,JSON.stringify({id:'lizard-protection',command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}}));
  execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(root,'native.json'),path]);
  const samples=[];
  for(let n=0;n<40;n++){
    await game.act({keys:[],duration_ms:250},n===18?{name:'lizard-combat',when:()=>true}:undefined);
    samples.push(await game.inspect());
  }
  await writeFile(join(process.env.QA_RESULTS_DIR,'attacks.json'),JSON.stringify(samples));
  game.check('natural Lizard activates and moves',samples.some(s=>entity(s)?.activated!==null&&Math.hypot(entity(s).pos.x-lizard.pos.x,entity(s).pos.y-lizard.pos.y)>.2));
  for(const [index,speed,lifetime] of [[0,65,1700],[1,55,530]])game.check(index+': selected native projectile is observed',samples.some(s=>s.view.projectiles.some(p=>p.owner===lizard.id&&p.speed===speed&&p.lifetime_ms===lifetime)));
  // This verifies the Trapped reconstruction, not the conflicting Kithio or period attack counts: https://github.com/trapped/rotmg_svr/blob/593e490803fe0ad168ead408cacd5287d778acfe/wServer/logic/db/BehaviorDb.Midland.cs#L687-L717
  const shots=samples.slice(1).map((s,i)=>({age:s.diagnostic.time_ms-entity(s).activated,count:(entity(s).bullet-entity(samples[i]).bullet+256)%256}));
  await writeFile(join(process.env.QA_RESULTS_DIR,'shot-counts.json'),JSON.stringify(shots));
  game.check('aimed singles or pairs are observed',shots.some(s=>s.count===1||s.count===2));
  game.check('twenty-shot ring volleys are observed',shots.some(s=>s.count>=20&&s.count%20===0));
  game.check('short pause between ring and next cycle is observed',shots.some(s=>s.age%8100>=7350&&s.age%8100<8100&&s.count===0));
  game.check('planewalk reaches the stationary native player',samples.some((s,i)=>i>0&&entity(s).reaction_until>entity(samples[i-1]).reaction_until&&Math.hypot(entity(s).pos.x-s.view.position.x,entity(s).pos.y-s.view.position.y)<.6&&Math.hypot(entity(samples[i-1]).pos.x-s.view.position.x,entity(samples[i-1]).pos.y-s.view.position.y)>1));
  game.check('same Lizard has no dependent actors during combat',samples.every(s=>entity(s)?.kind===0x952&&!s.diagnostic.entities.some(e=>e.parent===lizard.id)));
  const before=await game.observe();
  for(let n=0;n<30&&(await game.inspect()).diagnostic.entities.some(e=>e.id===lizard.id);n++)await game.act({keys:[],duration_ms:300,target:{entity:lizard.id,camera_angle:-Math.PI/4,fire:true}});
  await game.act({keys:[],duration_ms:300},{name:'lizard-defeated',when:()=>true});
  const dead=await save('defeated');
  game.check('ordinary native staff defeats Lizard and awards XP',!entity(dead)&&dead.view.character.experience>before.character.experience);
  await game.act({keys:[],duration_ms:1000});
  const final=await save('final');
  game.check('defeat awards XP only once',final.view.character.experience===dead.view.character.experience);
  game.check('defeat creates no nearby Lizard successor or dependent actor',!final.diagnostic.entities.some(e=>e.parent===lizard.id||e.kind===0x952&&Math.hypot(e.home.x-lizard.pos.x,e.home.y-lizard.pos.y)<.1));
  game.check('same living prepared Wizard and equipment survive',final.view.lifecycle.alive&&final.view.session.character===begin.view.session.character&&JSON.stringify(final.view.character.items)===JSON.stringify(begin.view.character.items));
}
