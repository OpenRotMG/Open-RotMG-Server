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
  lizard=natural.find(e=>[-2,-1,0].every(dx=>[-1,0,1].every(dy=>!blocked.has([Math.floor(e.pos.x)+dx,Math.floor(e.pos.y)+dy].join(',')))));
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
  // XML Great Lizard cycles idle/charge/spit/flame_ring/pause (actors.xml:4794-4825); it has no planewalk or fixed 8100ms clock.
  const states=s=>(s.diagnostic.xml[lizard.id]?.states??[]).map(x=>x.name),volleys=(speed,size)=>samples.some(s=>{
    const groups=new Map();for(const p of s.view.projectiles.filter(p=>p.owner===lizard.id&&p.speed===speed)){const k=p.start.x+':'+p.start.y;groups.set(k,(groups.get(k)??0)+1);}
    return [...groups.values()].includes(size);
  });
  // The 0.2s charge is often between samples, so prove a full cycle as flame_ring followed by a later spit.
  const ring=samples.findIndex(s=>states(s).includes('flame_ring'));
  game.check('Lizard cycles spit and flame_ring states',ring>=0&&samples.slice(ring).some(s=>states(s).includes('spit')));
  game.check('spit fires aimed single and paired Fire Missiles',volleys(65,1)&&volleys(65,2));
  game.check('flame_ring fires thirty-shot rings (actors.xml:4816)',volleys(55,30));
  game.check('same Lizard has no dependent actors during combat',samples.every(s=>entity(s)?.kind===0x952&&!s.diagnostic.entities.some(e=>e.parent===lizard.id)));
  const before=await game.observe();
  // The charge and retreat can leave the Wizard stationary 16–18 tiles away (AC e5ad142d, c73c5da7).
  // Staff of Horror only travels 8.55 tiles (objects.xml:6097–6104); close the gap before taking another shot.
  for(let n=0;n<60;n++){
    const s=await game.inspect(),e=entity(s);if(!e)break;
    const dx=e.pos.x-s.view.position.x,dy=e.pos.y-s.view.position.y,x=dx-dy,y=dx+dy;
    const approach=Math.hypot(dx,dy)>4||!s.view.objects.some(o=>o.id===lizard.id&&o.hp>0);
    const keys=approach?[Math.abs(x)>=Math.abs(y)*.4142?(x>0?'d':'a'):null,Math.abs(y)>=Math.abs(x)*.4142?(y>0?'s':'w'):null].filter(Boolean):[];
    const scale=90/Math.max(1,Math.hypot(x,y));
    await game.act({keys,duration_ms:250,...(!approach?{mouse:{x:300+x*scale,y:325+y*scale}}:{})});
  }
  await game.act({keys:[],duration_ms:300},{name:'lizard-defeated',when:()=>true});
  const dead=await save('defeated');
  game.check('ordinary native staff defeats Lizard and awards XP',!entity(dead)&&dead.view.character.experience>before.character.experience);
  await game.act({keys:[],duration_ms:1000});
  const final=await save('final');
  game.check('defeat awards XP only once',final.view.character.experience===dead.view.character.experience);
  game.check('defeat creates no nearby Lizard successor or dependent actor',!final.diagnostic.entities.some(e=>e.parent===lizard.id||e.kind===0x952&&Math.hypot(e.home.x-lizard.pos.x,e.home.y-lizard.pos.y)<.1));
  game.check('same living prepared Wizard and equipment survive',final.view.lifecycle.alive&&final.view.session.character===begin.view.session.character&&JSON.stringify(final.view.character.items)===JSON.stringify(begin.view.character.items));
}
