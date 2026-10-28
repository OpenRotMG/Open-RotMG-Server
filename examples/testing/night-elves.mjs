import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
const spec={class:0x30e,level:20,seed:828,world:1,equipment:[0xa9f,-1,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,75,0,50,75,40,60]};
const root=process.argv[process.argv.indexOf('--run-dir')+1];
const kinds=[0x650,0x64f,0x64a,0x64b,0x64c,0x64d];
let lizard;
export async function prepare(api){
  const info=await api({op:'world_info',world:1}),[x,y]=info.spawn_points[0];
  const scout=await api({op:'spawn',spec:{...spec,guid:'night-elf-scout',position:{x,y},controller:'direct',profile:null}});
  const state=await api({op:'inspect',actor:scout.result.actor});
  const report=JSON.parse(await readFile(join(root,'result.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(root,'state/control.token'),'utf8')).trim()};
  const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers});assert(response.ok);
  const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
  await writeFile(join(process.env.QA_RESULTS_DIR,'natural-map.json'),JSON.stringify(map));
  const clear=p=>!blocked.has([Math.floor(p.x),Math.floor(p.y)].join(','));
  for(const e of map.entities.filter(e=>e.kind===0x650))for(let dx=-4;dx<=4;dx++)for(let dy=-4;dy<=4;dy++){
    const position={x:e.pos.x+dx,y:e.pos.y+dy};
    if(Math.hypot(dx,dy)<2||Math.hypot(dx,dy)>4)continue;
    if([-1,0,1].every(x=>[-1,0,1].every(y=>clear({x:position.x+x,y:position.y+y})))){
      lizard=e;return {...spec,position};
    }
  }
  throw Error('No naturally populated Lizard God camp with a clear player approach');
}
export async function run(game){
  let serial=0,protectedUntil=0;
  const save=async name=>{const s=await game.inspect();await writeFile(join(process.env.QA_RESULTS_DIR,name+'.json'),JSON.stringify(s));return s;};
  const family=s=>s.diagnostic.entities.filter(e=>e.camp===lizard.camp&&kinds.includes(e.kind));
  async function protect(){
    const now=(await game.observe()).game_ms;assert(Number.isSafeInteger(now));if(now<protectedUntil)return;
    const path=join(process.env.QA_RESULTS_DIR,'protection-'+serial+'.json');
    await writeFile(path,JSON.stringify({id:'night-elf-protection-'+serial++,command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}}));
    execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(root,'native.json'),path]);protectedUntil=now+60000;
  }
  const act=async(input,capture)=>{
    await protect();
    while(protectedUntil-(await game.observe()).game_ms<input.duration_ms+500){await game.act({keys:[],duration_ms:100});await protect();}
    return game.act(input,capture);
  };
  const begin=await save('arrival');
  game.check('natural HighForest camp contains the six selected identities',kinds.every(k=>family(begin).some(e=>e.kind===k)));
  game.check('selected HP survives ordinary admission',[[0x650,900],[0x64f,180],[0x64a,240],[0x64b,280],[0x64c,220],[0x64d,300]].every(([k,hp])=>family(begin).filter(e=>e.kind===k).every(e=>e.hp===hp)));
  await protect();
  const samples=[];
  for(let n=0;n<24;n++){await act({keys:[],duration_ms:500});samples.push(await game.inspect());}
  await writeFile(join(process.env.QA_RESULTS_DIR,'combat.json'),JSON.stringify(samples));
  await act({keys:[],duration_ms:800},{name:'night-elves-combat',when:()=>true});
  for(const [kind,speed,lifetime] of [[0x650,70,1600],[0x64f,70,1200],[0x64a,75,2000],[0x64b,100,800],[0x64c,60,2000],[0x64d,80,2000]]){
    const ids=new Set(samples.flatMap(s=>family(s).filter(e=>e.kind===kind).map(e=>e.id)));
    game.check(kind+': selected projectile reaches native client',samples.some(s=>s.view.projectiles.some(p=>ids.has(p.owner)&&p.speed===speed&&p.lifetime_ms===lifetime)));
    game.check(kind+': activates and moves or holds its chase stop range',samples.some(s=>family(s).some(e=>e.kind===kind&&e.activated!==null&&(Math.hypot(e.pos.x-s.view.position.x,e.pos.y-s.view.position.y)<([0x650,0x64b].includes(kind)?1.5:7.5)||family(begin).some(p=>p.id===e.id&&Math.hypot(e.pos.x-p.pos.x,e.pos.y-p.pos.y)>.2)))));
  }
  const caps=[[0x650,1],[0x64f,1],[0x64a,4],[0x64b,3],[0x64c,2],[0x64d,2]];
  game.check('current camp role caps hold',samples.every(s=>caps.every(([k,n])=>family(s).filter(e=>e.kind===k).length<=n)));
  game.check('current reconstruction adds new owned elves while active',samples.some(s=>family(s).some(e=>!family(begin).some(p=>p.id===e.id))));
  async function defeat(id){
    const xp=(await game.observe()).character.experience;
    for(let n=0;n<60;n++){
      const s=await game.inspect(),e=family(s).find(e=>e.id===id);if(!e)break;
      const dx=e.pos.x-s.view.position.x,dy=e.pos.y-s.view.position.y,x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2;
      if(Math.hypot(dx,dy)>4||!s.view.objects.some(o=>o.id===id&&o.hp>0))await act({keys:[Math.abs(x)>.15?(x>0?'d':'a'):null,Math.abs(y)>.15?(y>0?'s':'w'):null].filter(Boolean),duration_ms:150});
      else await act({keys:[],duration_ms:150,target:{entity:id,camera_angle:-Math.PI/4,fire:true}});
      await act({keys:[],duration_ms:250});
    }
    await act({keys:[],duration_ms:350});const dead=await save('defeated-'+id);
    game.check(id+': native staff defeats target and earns XP',!family(dead).some(e=>e.id===id)&&dead.view.character.experience>xp);
    return dead;
  }
  const rootDead=await defeat(lizard.id);
  game.check('Lizard God defeat leaves living ordinary elves',family(rootDead).some(e=>e.kind!==0x650));
  for(const kind of [0x64f,0x64a,0x64b,0x64c,0x64d]){
    const s=await game.inspect(),victim=family(s).find(e=>e.kind===kind);
    if(victim)await defeat(victim.id);
    else game.check(kind+': native collateral already defeated the observed role',family(begin).filter(e=>e.kind===kind).every(e=>!family(s).some(p=>p.id===e.id))&&s.view.character.experience>begin.view.character.experience);
  }
  await act({keys:[],duration_ms:2500});
  const dead=await save('defeated'),survivors=family(dead),endStart=(await game.observe()).game_ms;
  game.check('living elf orphans remain for post-leader observation',survivors.length>0&&survivors.every(e=>![0x650,0x64f].includes(e.kind)));
  await act({keys:[],duration_ms:11000},{name:'night-elves-defeated',when:v=>v.game_ms>=endStart+500});
  const final=await save('after-root-death');
  game.check('leader deaths preserve surviving ordinary elves',survivors.every(e=>family(final).some(p=>p.id===e.id)));
  game.check('current orphan reconstruction wanders',family(final).some(e=>survivors.some(p=>p.id===e.id&&Math.hypot(p.pos.x-e.pos.x,p.pos.y-e.pos.y)>.2)));
  game.check('dead root cannot produce another King',!family(final).some(e=>e.kind===0x650||e.kind===0x64f));
  game.check('root and King XP are not awarded twice',final.view.character.experience===dead.view.character.experience);
  game.check('same prepared living native Wizard completes the encounter',final.view.lifecycle.alive&&final.view.session.character===begin.view.session.character);
  await writeFile(join(process.env.QA_RESULTS_DIR,'reward-outcome.json'),JSON.stringify(final.view.objects.filter(e=>[0x500,0x503,0x507,0x508,0x509].includes(e.kind))));
}
