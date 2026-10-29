import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
const spec={class:0x30e,level:20,seed:815,world:1,equipment:[0xa97,0xa2e,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,10,0,50,75,40,60]};
const root=process.argv[process.argv.indexOf('--run-dir')+1];
let queen;
export async function prepare(api){
  const world=await api({op:'world_info',world:1}),[x,y]=world.spawn_points[0];
  const scout=await api({op:'spawn',spec:{...spec,guid:'scorpion-scout',position:{x,y},controller:'direct',profile:null}});
  const state=await api({op:'inspect',actor:scout.result.actor});
  const report=JSON.parse(await readFile(join(root,'result.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(root,'state/control.token'),'utf8')).trim()};
  const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers});assert(response.ok);
  const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
  queen=map.entities.find(e=>e.kind===0x604&&[-2,-1,0,1,2].every(dx=>[-2,-1,0,1,2].every(dy=>!blocked.has([Math.floor(e.pos.x)+dx,Math.floor(e.pos.y)+dy].join(',')))));assert(queen,'No clear naturally populated Scorpion Queen');
  await writeFile(join(process.env.QA_RESULTS_DIR,'natural-queens.json'),JSON.stringify({world,queens:map.entities.filter(e=>e.kind===0x604),family:map.entities.filter(e=>e.camp===queen.camp)}));
  return {...spec,position:{x:queen.pos.x-2,y:queen.pos.y}};
}
export async function run(game){
  const save=async name=>{const s=await game.inspect();await writeFile(join(process.env.QA_RESULTS_DIR,name+'.json'),JSON.stringify(s));return s;};
  const members=s=>s.diagnostic.entities.filter(e=>e.camp===queen.camp&&e.kind===0x603);
  const begin=await save('arrival'),initial=members(begin),samples=[];
  game.check('natural Queen has selected HP and five initial defenders',begin.diagnostic.entities.some(e=>e.id===queen.id&&e.hp===100)&&initial.length===5&&initial.every(e=>e.hp===10));
  await game.act({keys:[],duration_ms:4000},{name:'scorpion-arrival',when:()=>true});
  const exposed=await save('exposed');
  game.check('unarmored native Wizard takes enemy damage',exposed.view.character.hp<begin.view.character.hp);
  game.check('ordinary Green Magic causes no status effect',exposed.view.character.conditions===0);
  const path=join(process.env.QA_RESULTS_DIR,'protection.json');
  await writeFile(path,JSON.stringify({id:'scorpion-protection',command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}}));
  execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(root,'native.json'),path]);
  for(let n=0;n<6;n++){await game.act({keys:[],duration_ms:700});samples.push(await game.inspect());}
  await writeFile(join(process.env.QA_RESULTS_DIR,'movement-and-attacks.json'),JSON.stringify(samples));
  game.check('Queen activates and wanders',samples.some(s=>s.diagnostic.entities.some(e=>e.id===queen.id&&e.activated!==null&&Math.hypot(e.pos.x-queen.pos.x,e.pos.y-queen.pos.y)>.1)));
  game.check('owned defenders move',samples.some(s=>members(s).some(e=>initial.some(p=>p.id===e.id&&Math.hypot(e.pos.x-p.pos.x,e.pos.y-p.pos.y)>.2))));
  game.check('native client receives selected Green Magic',samples.some(s=>s.view.projectiles.some(p=>initial.some(e=>e.id===p.owner)&&p.speed===40&&p.lifetime_ms===2000)));
  game.check('Queen has no projectile of her own',samples.every(s=>s.view.projectiles.every(p=>p.owner!==queen.id)));
  // This is a bounded reconstruction check, not target-era count proof: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/logic/db/BehaviorDb.Shore.cs#L55-L65
  const brood=await save('brood');
  game.check('current brood stays within ten owned defenders',members(brood).length<=10);
  const captureStart=(await game.observe()).game_ms;
  await game.act({keys:[],duration_ms:1000},{name:'scorpion-brood',when:v=>v.game_ms>=captureStart+500});
  async function defeat(id){
    const before=await game.observe();
    for(let n=0;n<40;n++){
      const s=await game.inspect(),e=s.diagnostic.entities.find(e=>e.id===id);if(!e)break;
      const dx=e.pos.x-s.view.position.x,dy=e.pos.y-s.view.position.y,x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2;
      if(Math.hypot(dx,dy)>3||!s.view.objects.some(e=>e.id===id&&e.hp>0))await game.act({keys:[Math.abs(x)>.15?(x>0?'d':'a'):null,Math.abs(y)>.15?(y>0?'s':'w'):null].filter(Boolean),duration_ms:200});
      else{
        await game.act({keys:[],duration_ms:100,target:{entity:id,camera_angle:-Math.PI/4,fire:true}});
        await game.act({keys:[],duration_ms:500});
      }
    }
    await game.act({keys:[],duration_ms:350});const after=await game.inspect();
    game.check(id+': native staff defeats target with XP',!after.diagnostic.entities.some(e=>e.id===id)&&after.view.character.experience>before.character.experience);
    return after;
  }
  const victim=members(brood).sort((a,b)=>Math.hypot(a.pos.x-brood.view.position.x,a.pos.y-brood.view.position.y)-Math.hypot(b.pos.x-brood.view.position.x,b.pos.y-brood.view.position.y))[0];
  const killed=await defeat(victim.id),ids=new Set(members(brood).map(e=>e.id));
  game.check('ordinary defender has no loot bag',killed.diagnostic.entities.filter(e=>[0x500,0x503,0x507,0x508,0x509].includes(e.kind)).length===brood.diagnostic.entities.filter(e=>[0x500,0x503,0x507,0x508,0x509].includes(e.kind)).length);
  for(let n=0;n<12&&!members(await game.inspect()).some(e=>!ids.has(e.id));n++)await game.act({keys:[],duration_ms:1000});
  const replenished=await save('replenished');
  game.check('living Queen adds an owned defender after native defeat within cap',members(replenished).some(e=>!ids.has(e.id))&&members(replenished).length<=10);
  const dead=await defeat(queen.id);await save('queen-defeated');
  const surviving=new Set(members(dead).map(e=>e.id));
  const defeatStart=(await game.observe()).game_ms;
  await game.act({keys:[],duration_ms:10000});await game.act({keys:[],duration_ms:1000},{name:'scorpion-defeated',when:v=>v.game_ms>=defeatStart+10500});
  const final=await save('final');
  game.check('dead Queen cannot replenish her camp',members(final).every(e=>surviving.has(e.id))&&!final.diagnostic.entities.some(e=>e.camp===queen.camp&&e.kind===0x604));
  game.check('Queen XP is not credited twice',final.view.character.experience===dead.view.character.experience);
  game.check('same prepared living Wizard completes encounter',final.view.lifecycle.alive&&final.view.session.character===begin.view.session.character);
}
