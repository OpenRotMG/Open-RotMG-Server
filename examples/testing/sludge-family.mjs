import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';
const spec={class:0x30e,level:20,seed:803,world:1,equipment:[0xa9f,0xa2e,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,75,25,50,75,40,60]};
const root=process.argv[process.argv.indexOf('--run-dir')+1];
let sludge;
export async function prepare(api){
  const world=await api({op:'world_info',world:1}),[x,y]=world.spawn_points[0];
  const scout=await api({op:'spawn',spec:{...spec,guid:'sludge-scout',position:{x,y},controller:'direct',profile:null}});
  const state=await api({op:'inspect',actor:scout.result.actor});
  const report=JSON.parse(await readFile(join(root,'result.json'),'utf8'));
  const headers={Authorization:'Bearer '+(await readFile(join(root,'state/control.token'),'utf8')).trim()};
  const response=await fetch(report.origin+'/dev/map?player='+state.diagnostic.player,{headers});assert(response.ok);
  const map=await response.json(),blocked=new Set(map.blocked.map(p=>p.join(',')));
  const clear=(x,y)=>!blocked.has([Math.floor(x),Math.floor(y)].join(','));
  sludge=map.entities.find(e=>e.kind===0x950&&clear(e.pos.x,e.pos.y)&&Array.from({length:5},(_,i)=>i*Math.PI*2/5).every(a=>clear(e.pos.x+4*Math.cos(a),e.pos.y+4*Math.sin(a))));
  assert(sludge,'No naturally populated Sludge with clear initial landing positions');
  await writeFile(join(process.env.QA_RESULTS_DIR,'natural-sludges.json'),JSON.stringify(map.entities.filter(e=>e.kind===0x950)));
  return {...spec,position:{x:sludge.pos.x,y:sludge.pos.y}};
}
export async function run(game){
  const save=async name=>{const s=await game.inspect();await writeFile(join(process.env.QA_RESULTS_DIR,name+'.json'),JSON.stringify(s));return s;};
  const children=s=>s.diagnostic.entities.filter(e=>e.camp===sludge.camp&&e.kind===0x951);
  const parent=s=>s.diagnostic.entities.find(e=>e.id===sludge.id);
  const first=await save('arrival'),hp=first.view.character.hp;
  game.check('natural Realm Sludge starts with selected 1000 HP',parent(first)?.hp===1000);
  await game.act({keys:[],duration_ms:500},{name:'sludge-arrival',when:()=>true});
  const active=await save('activated');
  game.check('ordinary proximity activates the natural Sludge',parent(active)?.activated!==null);
  const path=join(process.env.QA_RESULTS_DIR,'protection.json'),trace=[];
  let protectedAfterHit=false;
  for(let n=0;n<8&&!protectedAfterHit;n++){
    await game.act({keys:[],duration_ms:500});const s=await game.inspect();trace.push(s);
    await writeFile(join(process.env.QA_RESULTS_DIR,'attacks.json'),JSON.stringify(trace));
    if(!protectedAfterHit&&s.view.character.hp<hp&&(s.view.character.conditions&(1<<3))!==0){
      await writeFile(path,JSON.stringify({id:'sludge-protection',command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}}));
      execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(root,'native.json'),path]);protectedAfterHit=true;
    }
  }
  assert(protectedAfterHit,'No unprotected damage and Slowed observed before protection');
  let pending=null,sampleError=null;
  const timer=setInterval(()=>{if(!pending)pending=game.inspect().then(s=>trace.push(s)).catch(e=>{sampleError=e;}).finally(()=>{pending=null;});},100);
  try{await game.act({keys:[],duration_ms:6000},{name:'sludge-attacks',when:()=>true});}
  finally{clearInterval(timer);if(pending)await pending;}
  if(sampleError)throw sampleError;
  await writeFile(join(process.env.QA_RESULTS_DIR,'attacks.json'),JSON.stringify(trace));
  game.check('selected parent spinner speed and lifetime reach the native session',trace.some(s=>s.view.projectiles.some(p=>p.owner===sludge.id&&p.speed===42&&p.lifetime_ms===3200)));
  game.check('native enemy attacks damage the prepared Wizard',trace.some(s=>s.view.character.hp<hp));
  game.check('native spinner applies selected Slowed status',trace.some(s=>(s.view.character.conditions&(1<<3))!==0));
  const wave=trace.find(s=>children(s).length===5);assert(wave,'No complete five-Sludget wave observed');
  game.check('admitted children arrive after the 1500 ms telegraph',children(wave).every(e=>e.activated>=parent(active).activated+1500));
  game.check('Sludgets use selected 250 HP and small appearance',children(wave).every(e=>e.hp===250&&e.size===50));
  game.check('Sludget projectiles reach the native session',trace.some(s=>s.view.projectiles.some(p=>children(s).some(e=>e.id===p.owner)&&p.speed===70&&p.lifetime_ms===510)));
  game.check('activated parent wanders',trace.some(s=>parent(s)&&Math.hypot(parent(s).pos.x-sludge.pos.x,parent(s).pos.y-sludge.pos.y)>.1));
  game.check('Sludgets move after landing',trace.some(s=>children(s).some(e=>children(wave).some(p=>p.id===e.id&&Math.hypot(e.pos.x-p.pos.x,e.pos.y-p.pos.y)>.2))));
  await game.act({keys:[],duration_ms:500},{name:'sludge-wave',when:()=>true});
  const beforeThrow=await game.inspect(),due=parent(beforeThrow).next_aux,wait=due-beforeThrow.view.time_ms;
  assert(wait>0&&wait<8000,'Next timed throw is outside the bounded capture window');
  await game.act({keys:[],duration_ms:wait+700},{name:'sludge-throw',when:view=>view.game_ms>=due+250&&view.game_ms<due+1450});
  const old=new Set(children(wave).map(e=>e.id));
  let replacement;
  for(let n=0;n<14;n++){
    await game.act({keys:[],duration_ms:500});const s=await game.inspect();
    game.check('wave stays inside five owned slots at sample '+n,children(s).length<=5);
    if(children(s).length===5&&children(s).every(e=>!old.has(e.id))){replacement=s;break;}
  }
  assert(replacement,'Full-health parent did not replace its timed wave');
  // This checks the admitted reconstruction, not historical fidelity: https://github.com/trapped/rotmg_svr/blob/593e490803fe0ad168ead408cacd5287d778acfe/wServer/logic/cond/Shambling.cs#L17-L79
  game.check('new wave replaces old children without an HP threshold',parent(replacement).hp===1000&&children(replacement).every(e=>!old.has(e.id)));
  async function defeat(id){
    const before=await game.observe();
    for(let n=0;n<24&&(await game.inspect()).diagnostic.entities.some(e=>e.id===id);n++)await game.act({keys:[],duration_ms:250,target:{entity:id,camera_angle:-Math.PI/4,fire:true}});
    await game.act({keys:[],duration_ms:300});
    const s=await game.inspect();
    game.check(id+': ordinary staff fire defeats target and earns XP',!s.diagnostic.entities.some(e=>e.id===id)&&s.view.character.experience>before.character.experience);
  }
  await defeat(children(replacement)[0].id);
  await defeat(sludge.id);
  await game.act({keys:[],duration_ms:1500});
  const dead=await save('defeated'),survivors=new Set(children(dead).map(e=>e.id));
  await game.act({keys:[],duration_ms:10000});
  await game.act({keys:[],duration_ms:1500},{name:'sludge-defeated',when:()=>true});
  const final=await save('after-parent-death');
  game.check('dead parent produces no later waves',!parent(final)&&children(final).every(e=>survivors.has(e.id)));
  game.check('replacement and cleanup do not award duplicate XP',final.view.character.experience===dead.view.character.experience);
  game.check('surviving ordinary children currently outlive their parent',children(final).length>0);
  game.check('same living prepared Wizard finishes encounter',final.view.lifecycle.alive&&final.view.session.character===first.view.session.character);
}
