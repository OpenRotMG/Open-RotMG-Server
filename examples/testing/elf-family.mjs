import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
const spec={class:0x30e,level:20,seed:812,world:1,equipment:[0xa9f,-1,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,75,0,50,75,40,60]};
const root=process.argv[process.argv.indexOf('--run-dir')+1];
let wizard;
export async function prepare(api){
  const report=JSON.parse(await readFile(join(root,'result.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(root,'state/control.token'),'utf8')).trim()};
  for(const world of [1]){
    const info=await api({op:'world_info',world}),[x,y]=info.spawn_points[0];
    const scout=await api({op:'spawn',spec:{...spec,world,guid:'elf-scout-'+world,position:{x,y},controller:'direct',profile:null}});
    const state=await api({op:'inspect',actor:scout.result.actor});
    const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers});assert(response.ok);
    const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
    await writeFile(join(process.env.QA_RESULTS_DIR,'natural-elves-'+world+'.json'),JSON.stringify(map.entities.filter(e=>e.kind===0x609)));
    await writeFile(join(process.env.QA_RESULTS_DIR,'map.json'),JSON.stringify(map));
    const clear=p=>!blocked.has([Math.floor(p.x),Math.floor(p.y)].join(','));
    for(const e of map.entities.filter(e=>e.kind===0x609))for(let dx=-6;dx<=6;dx++)for(let dy=-6;dy<=6;dy++){
      const position={x:e.pos.x+dx,y:e.pos.y+dy};
      if(Math.hypot(dx,dy)<3||Math.hypot(dx,dy)>6||!clear(position))continue;
      if([2.7,3,3.3].every(r=>Array.from({length:64},(_,i)=>i*Math.PI/32).every(a=>clear({x:position.x+r*Math.cos(a),y:position.y+r*Math.sin(a)})))){
        wizard=e;return {...spec,world,position};
      }
    }
  }
  throw Error('Ordinary Realm has no Elf camp with clear player orbit space');
}
export async function run(game){
  let serial=0,protectedUntil=0;
  const save=async name=>{const s=await game.inspect();await writeFile(join(process.env.QA_RESULTS_DIR,name+'.json'),JSON.stringify(s));return s;};
  const family=s=>s.diagnostic.entities.filter(e=>e.camp===wizard.camp&&[0x605,0x606,0x607,0x609].includes(e.kind));
  async function protect(){
    const now=(await game.observe()).game_ms;assert(Number.isSafeInteger(now));if(now<protectedUntil)return;
    const path=join(process.env.QA_RESULTS_DIR,'protection-'+serial+'.json');
    await writeFile(path,JSON.stringify({id:'elf-protection-'+serial++,command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}}));
    execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(root,'native.json'),path]);protectedUntil=now+60000;
  }
  const begin=await save('arrival'),hp=begin.view.character.hp;
  game.check('ordinary LowForest camp has Wizard and 2/4/1 escorts',[[0x609,1],[0x605,2],[0x606,4],[0x607,1]].every(([k,n])=>family(begin).filter(e=>e.kind===k).length===n));
  game.check('selected Wizard has 200 HP',family(begin).some(e=>e.id===wizard.id&&e.hp===200));
  await game.act({keys:[],duration_ms:800},{name:'elf-encounter',when:()=>true});
  for(let n=0;n<12&&(await game.observe()).character.hp>=hp;n++)await game.act({keys:[],duration_ms:500});
  game.check('native character receives encounter damage',(await game.observe()).character.hp<hp);
  await protect();
  const samples=[];
  for(let n=0;n<30;n++){await game.act({keys:[],duration_ms:500});samples.push(await game.inspect());}
  await writeFile(join(process.env.QA_RESULTS_DIR,'movement-and-attacks.json'),JSON.stringify(samples));
  await game.act({keys:[],duration_ms:800},{name:'elf-orbits',when:()=>true});
  game.check('Wizard activates and moves',samples.some(s=>family(s).some(e=>e.id===wizard.id&&e.activated!==null&&Math.hypot(e.pos.x-wizard.pos.x,e.pos.y-wizard.pos.y)>.2)));
  for(const [kind,speed,lifetime] of [[0x609,70,2000],[0x605,65,2000],[0x606,100,400],[0x607,65,1500]]){
    const ids=new Set(family(begin).filter(e=>e.kind===kind).map(e=>e.id));
    game.check(kind+': native client receives selected projectile',samples.some(s=>s.view.projectiles.some(p=>ids.has(p.owner)&&p.speed===speed&&p.lifetime_ms===lifetime)));
  }
  const orbits=family(begin).filter(e=>[0x605,0x607].includes(e.kind)).map(e=>{
    let last=null,angle=0,near=0;
    for(const s of samples.slice(8)){const a=family(s).find(a=>a.id===e.id);assert(a);const dx=a.pos.x-s.view.position.x,dy=a.pos.y-s.view.position.y,t=Math.atan2(dy,dx);if(last!==null)angle+=Math.atan2(Math.sin(t-last),Math.cos(t-last));last=t;if(Math.hypot(dx,dy)>1.5&&Math.hypot(dx,dy)<3.5)near++;}
    return {id:e.id,kind:e.kind,angle,near};
  });
  await writeFile(join(process.env.QA_RESULTS_DIR,'orbits.json'),JSON.stringify(orbits));
  const safe=(s,pos)=>{
    const p=s.view.position,k=family(s).find(e=>e.id===wizard.id);assert(k,'Wizard must remain alive during escort targeting');
    const dx=pos.x-p.x,dy=pos.y-p.y,t=((k.pos.x-p.x)*dx+(k.pos.y-p.y)*dy)/(dx*dx+dy*dy);
    return t<0||Math.hypot(k.pos.x-p.x-t*dx,k.pos.y-p.y-t*dy)>3;
  };
  async function defeat(id){
    const xp=(await game.observe()).character.experience;
    for(let n=0;n<35;n++){
      const s=await game.inspect(),e=s.view.objects.find(e=>e.id===id&&e.hp>0);if(!e)break;
      if(id===wizard.id||safe(s,e.position))await game.act({keys:[],duration_ms:100,target:{entity:id,camera_angle:-Math.PI/4,fire:true}});
      await game.act({keys:[],duration_ms:500});
    }
    await game.act({keys:[],duration_ms:500});const s=await game.inspect();
    game.check(id+': native staff defeats target and earns XP',!s.diagnostic.entities.some(e=>e.id===id)&&s.view.character.experience>xp);
  }
  for(const kind of [0x605,0x606,0x607]){
    await protect();
    let before,victim;
    for(let n=0;n<30;n++){
      before=await game.inspect();
      victim=family(before).find(e=>e.kind===kind&&safe(before,e.pos)&&before.view.objects.some(o=>o.id===e.id&&o.hp>0));if(victim)break;
      await game.act({keys:[],duration_ms:400});
    }
    assert(victim,'No visible escort with safe firing line');
    const ids=new Set(family(before).map(e=>e.id));await defeat(victim.id);
    for(let n=0;n<18&&!family(await game.inspect()).some(e=>e.kind===kind&&!ids.has(e.id));n++)await game.act({keys:[],duration_ms:1000});
    const after=await save('replenished-'+kind);
    game.check(kind+': current reconstruction replenishes escort',family(after).some(e=>e.kind===kind&&!ids.has(e.id)));
    game.check(kind+': current role caps hold',[[0x605,2],[0x606,4],[0x607,1]].every(([k,n])=>family(after).filter(e=>e.kind===k).length<=n));
  }
  await defeat(wizard.id);const dead=await save('wizard-defeated'),remaining=family(dead);
  await game.act({keys:[],duration_ms:10000});await game.act({keys:[],duration_ms:6000});
  const final=await save('after-parent-death');
  game.check('dead Wizard cannot replenish or transform',family(final).every(e=>e.kind!==0x609&&remaining.some(p=>p.id===e.id)));
  game.check('Wizard XP awarded once',final.view.character.experience===dead.view.character.experience);
  game.check('prepared native character survives',final.view.lifecycle.alive&&final.view.session.character===begin.view.session.character);
  game.check('Archers and Mage repeatedly circle the player within the bounded motor footprint',orbits.every(e=>e.angle>3&&e.near>=16));
}
