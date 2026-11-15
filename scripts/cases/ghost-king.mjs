import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
const out=process.env.QA_RESULTS_DIR;
const spec={class:0x31d,level:20,seed:794,world:1,equipment:[0xb0b,0xb29,0xa7b,-1],backpack:Array(8).fill(-1),stats:[770,252,75,25,50,50,75,0]};
let roots;
export async function prepare(api){
  const world=await api({op:'world_info',world:1});
  const events=(await readFile(join(out,'native/server.log'),'utf8')).split('\n').filter(l=>l.startsWith('{')).map(l=>JSON.parse(l).fields);
  const end=events.findLastIndex(f=>f.message==='Realm simulation populated'&&f.layout===world.realm_layout);assert(end>=0,'Completed Realm initialization required');
  const start=events.slice(0,end).findLastIndex(f=>f.message==='Realm simulation populated');
  roots=events.slice(start+1,end).filter(f=>f.message==='finite named Realm quest placed'&&f.kind===0x928&&f.layout===world.realm_layout);
  assert(roots.length>=3,'Three ordinarily placed Ghost Kings required');
  const index=['SmallGroup','Mob','HugeMob'].indexOf(process.env.GHOST_BRANCH);assert(index>=0,'Set GHOST_BRANCH to SmallGroup, Mob or HugeMob');
  await writeFile(join(out,'roots.json'),JSON.stringify({world,roots,branch:process.env.GHOST_BRANCH}));
  return {...spec,...(index===2?{equipment:[0xb0b,0xb29,0xa7b,0xacc],backpack:[0xb10,-1,-1,-1,-1,-1,-1,-1]}:{}),position:{x:roots[index].x,y:roots[index].y}};
}
export async function run(game){
  const config=JSON.parse(await readFile(join(out,'native/native.json'),'utf8')),token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const records=[],protectedUntil=new Map();let cofighter;
  async function cheat(actor,change){
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:actor.actor,change}})});
    const receipt=await response.json();assert(response.ok&&receipt.status==='succeeded',JSON.stringify(receipt));
  }
  async function save(label){
    const state=await game.inspect();records.push({label,state});await writeFile(join(out,'encounters.json'),JSON.stringify(records));return state;
  }
  async function protect(actor){
    const now=(await actor.observe()).game_ms;
    if(now>=(protectedUntil.get(actor.actor)??0)){await cheat(actor,{kind:'protection',condition:'invulnerable',duration_ms:60000});protectedUntil.set(actor.actor,now+60000);}
  }
  async function idle(ms){
    while(ms>0){const actors=[game,cofighter].filter(Boolean);for(const actor of actors)await protect(actor);const now=(await game.observe()).game_ms,step=Math.min(ms,10000,...actors.map(actor=>protectedUntil.get(actor.actor)-now));await game.act({keys:[],duration_ms:step});ms-=step;}
  }
  const births=async quest=>(await readFile(join(out,'native/server.log'),'utf8')).split('\n').filter(l=>l.startsWith('{')).map(l=>JSON.parse(l).fields).filter(f=>f.message==='bounded named quest followers spawned'&&f.quest===quest&&f.kind>=0x929&&f.kind<=0x92c).reduce((counts,f)=>(counts[f.kind-0x929]+=f.count,counts),[0,0,0,0]);
  const family=(state,quest)=>state.diagnostic.entities.filter(e=>e.quest===quest&&e.kind>=0x929&&e.kind<=0x92c);
  const king=(state,id)=>state.diagnostic.entities.find(e=>e.id===id);
  async function approach(actor,point,range=1.5){
    for(let n=0;n<60;n++){
      await protect(actor);
      const view=await actor.observe(),dx=point.x-view.position.x,dy=point.y-view.position.y;if(Math.hypot(dx,dy)<range)return;
      const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2,keys=[];
      if(Math.abs(x)>=Math.abs(y)*.4142)keys.push(x>0?'d':'a');
      if(Math.abs(y)>=Math.abs(x)*.4142)keys.push(y>0?'s':'w');
      const distance=keys.length===2?Math.min(Math.abs(x),Math.abs(y))*Math.SQRT2:Math.max(Math.abs(x),Math.abs(y));
      await actor.act({keys,duration_ms:Math.max(50,Math.min(250,Math.round(distance/(4+5.6*view.character.stats.speed/75)*1000)))});
    }
    throw Error('Native walking did not reach the observed target');
  }
  async function fire(actor,id,ms=250){
    await protect(actor);
    const state=await actor.inspect(),target=king(state,id);assert(target,'Living native target required');
    if(Math.hypot(target.pos.x-state.view.position.x,target.pos.y-state.view.position.y)>2)await approach(actor,target.pos);
    await actor.act({keys:[],duration_ms:ms,target:{entity:id,camera_angle:-Math.PI/4,fire:true}});
  }
  const initial=await game.observe();await protect(game);
  if(process.env.GHOST_BRANCH==='HugeMob'){
    cofighter=await game.join({...spec,seed:795,position:initial.position,equipment:[0xb0b,0xb29,0xa7b,0xacc]});
    await protect(cofighter);
    await cheat(cofighter,{kind:'stat_modifier',index:2,amount:100,duration_ms:60000});
  }
  await game.act({keys:['s'],duration_ms:150});
  game.check('ordinary native movement works at the naturally placed tower',Math.hypot((await game.observe()).position.x-initial.position.x,(await game.observe()).position.y-initial.position.y)>.3);
  await approach(game,initial.position,.3);
  if(process.env.GHOST_BRANCH==='HugeMob'){
    await game.act({keys:['1'],duration_ms:150});
    game.check('native Dexterity effusion and ring supply the HugeMob damage fixture',(await game.observe()).character.stats.dexterity===88);
  }
  for(const [index,label,burst,stage,counts] of [[0,'SmallGroup',700,4,[0,3,3,0]],[1,'Mob',3000,5,[0,3,0,3]],[2,'HugeMob',3000,6,[0,3,0,3]]].filter(s=>s[1]===process.env.GHOST_BRANCH)){
    const root=roots[index];
    await protect(game);
    const before=await save(label+'-idle'),original=king(before,root.entity);assert(original?.kind===0x928&&original.hp===300000,'Untouched ordinary King required');
    await cheat(game,{kind:'stat_modifier',index:2,amount:100,duration_ms:60000});
    for(let n=0;n<20&&king(await game.inspect(),root.entity).stage!==1;n++)await fire(game,root.entity);
    game.check(label+' admits ordinary sword damage',king(await game.inspect(),root.entity).stage===1);
    const step=input=>({actor:game.actor,input}),sequence=[step({keys:[],duration_ms:2000}),step({keys:[' '],duration_ms:250})];
    const interval=index===2?500:250;
    for(let remaining=burst;remaining>0;remaining-=interval){
      const input={keys:[],duration_ms:Math.min(interval,remaining),target:{entity:root.entity,camera_angle:-Math.PI/4,fire:true}};
      sequence.push(cofighter?[step(input),{actor:cofighter.actor,input}]:step(input));
    }
    const evaluated=await game.act(sequence);
    game.check(label+' evaluation uses an ordinary Berserk helm',evaluated.steps[1].character.conditions&(1<<19));
    for(let n=0;n<7&&king(await game.inspect(),root.entity).stage<3;n++)await idle(1000);
    let state=await save(label+'-selected');
    game.check(label+' is selected by measured combat damage',king(state,root.entity).stage===stage);
    await idle(2000);state=await save(label+'-first-wave');
    const quest=original.quest,members=family(state,quest);
    const admitted=await births(quest);await writeFile(join(out,label+'-admission.json'),JSON.stringify({requested:counts,admitted}));
    game.check(label+' first wave matches admitted ghosts within requested populations',counts.every((n,i)=>(n?admitted[i]>0&&admitted[i]<=n:admitted[i]===0)&&members.filter(e=>e.kind===0x929+i).length===admitted[i]));
    await game.act({keys:[],duration_ms:750},{name:label+'-wave',when:()=>true});
    if(index===2){
      await idle(30000);state=await save(label+'-second-wave');
      const second=await births(quest);
      game.check('HugeMob admits a second bounded Small and Large Ghost wave',[1,3].every(i=>second[i]>admitted[i]&&second[i]<=6&&family(state,quest).filter(e=>e.kind===0x929+i).length===second[i]));
    }
    for(let n=0;n<7&&king(await game.inspect(),root.entity).stage!==7;n++)await idle(5000);
    state=await save(label+'-wait');const master=family(state,quest).find(e=>e.kind===0x929);
    game.check(label+' Company produces one owned Master',!!master&&family(state,quest).filter(e=>e.kind===0x929).length===1);
    game.check(label+' Company population matches actual admitted births',family(state,quest).length===(await births(quest)).reduce((a,b)=>a+b,0)&&family(state,quest).length<=(index===2?18:12));
    if(index===2){
      // Decay bypasses combat rewards: https://github.com/Zolmex/kithio-as3-source/blob/d15f321edcce0ef890001890c6991c25a1b0f718/Server%20Source/wServer/logic/behaviors/Decay.cs
      const waitStart=king(state,root.entity).activated,experience=state.view.character.experience;
      while((await game.observe()).game_ms-waitStart<125000)await idle(5000);
      state=await save('timeout-before');
      game.check('King and undamaged Master survive before the 140-second Wait timeout',!!king(state,root.entity)&&king(state,master.id)?.hp===100000);
      await game.act({keys:[],duration_ms:500},{name:'timeout-shielded',when:()=>true});
      for(let n=0;n<5&&king(await game.inspect(),root.entity);n++)await idle(5000);
      await game.act({keys:[],duration_ms:750},{name:'timeout-released',when:view=>view.nearby.some(e=>e.kind===0x92d)});state=await save('timeout-after');
      game.check('timeout releases Actual King and removes nearby owned ghosts without XP',!king(state,root.entity)&&family(state,quest).length===0&&state.diagnostic.entities.some(e=>e.quest===quest&&e.kind===0x92d)&&state.view.character.experience===experience);
    }else{
      for(let n=0;n<60&&king(await game.inspect(),root.entity);n++)await fire(game,master.id);
      state=await save(label+'-released');game.check(label+' Master damage releases the King',!king(state,root.entity)&&state.diagnostic.entities.some(e=>e.kind===0x92d&&e.quest===quest));
    }
    const actual=state.diagnostic.entities.find(e=>e.kind===0x92d&&e.quest===quest);assert(actual);
    if(index<2){
      for(let n=0;n<30&&king(await game.inspect(),actual.id);n++)await fire(game,actual.id);
      game.check(label+' final King is defeated by native sword fire',!king(await game.inspect(),actual.id));
      const xp=(await game.observe()).character.experience;await idle(2000);
      game.check(label+' defeat credit stays stable',(await game.observe()).character.experience===xp);
      continue;
    }
    await approach(game,actual.pos);
    const contributor=cofighter,outsider=await game.join({...spec,seed:796,position:actual.pos});assert(contributor);
    await protect(contributor);await protect(outsider);
    const xp=await Promise.all([game.observe(),contributor.observe(),outsider.observe()]);
    for(const actor of [contributor,game]){
      const beforeDamage=king(await game.inspect(),actual.id).damage[actor.actor]??0;
      for(let n=0;n<8;n++){
        if((king(await game.inspect(),actual.id).damage[actor.actor]??0)>beforeDamage+500)break;
        await fire(actor,actual.id,150);
      }
    }
    let damaged=await save('multiplayer-damage');
    game.check('two native players contribute above the private 500-damage threshold',damaged.diagnostic.entities.find(e=>e.id===actual.id).damage[game.actor]>500&&damaged.diagnostic.entities.find(e=>e.id===actual.id).damage[contributor.actor]>500);
    for(let n=0;n<30&&king(await game.inspect(),actual.id);n++)await fire(game,actual.id);
    await idle(500);
    const views=await Promise.all([game.inspect(),contributor.inspect(),outsider.inspect()]);
    game.check('final defeat awards both contributors XP',views[0].view.character.experience>xp[0].character.experience&&views[1].view.character.experience>xp[1].character.experience);
    const bags=views.flatMap(s=>s.diagnostic.entities).filter(e=>e.extra_stats.some(([stat,account])=>stat===54&&account>=0)&&e.items?.some(i=>i!==-1));
    const bag=bags.find(e=>e.extra_stats.some(([stat,account])=>stat===54&&account===views[1].view.session.account));assert(bag,'This observed roll must contain a contributor bag for the pickup check');
    game.check('noncontributor cannot see either private reward',!views[2].diagnostic.entities.some(e=>bags.some(b=>b.id===e.id)));
    await approach(outsider,bag.pos,.4);
    await outsider.act({keys:[],duration_ms:700},{name:'ineligible-no-bag',when:()=>true});
    await outsider.act({keys:[],duration_ms:350,drag:{from:{x:634,y:528},to:{x:634,y:428}}});
    game.check('noncontributor native drag cannot acquire the private reward',(await outsider.observe()).character.inventory_revision===views[2].view.character.inventory_revision&&(await contributor.inspect()).diagnostic.entities.find(e=>e.id===bag.id)?.items[0]===bag.items[0]);
    await approach(contributor,bag.pos,.4);
    await contributor.act({keys:[],duration_ms:700},{name:'eligible-private-bag',when:()=>true});
    const item=bag.items[0],inventory=(await contributor.observe()).character;assert(item!==-1);
    await contributor.act({keys:[],duration_ms:350,drag:{from:{x:634,y:528},to:{x:634,y:428}}});
    const picked=await contributor.inspect();
    game.check('native drag commits exactly one eligible item',picked.view.character.items[4]===item&&picked.view.character.inventory_revision===inventory.inventory_revision+1&&picked.diagnostic.entities.find(e=>e.id===bag.id)?.items[0]===-1);
    await contributor.act({keys:[],duration_ms:350,drag:{from:{x:634,y:528},to:{x:678,y:428}}});
    game.check('repeating the empty bag drag cannot duplicate loot',(await contributor.observe()).character.inventory_revision===picked.view.character.inventory_revision);
    await contributor.act({keys:['Insert'],duration_ms:250});
    const retained=await contributor.observe();
    game.check('normal Nexus reconnect preserves the acquired item and revision',retained.world==='Nexus'&&retained.lifecycle.alive&&retained.session.character===picked.view.session.character&&retained.character.items[4]===item&&retained.character.inventory_revision===picked.view.character.inventory_revision);
    await writeFile(join(out,'multiplayer.json'),JSON.stringify({xp,views,picked,retained}));
    const earned=(await game.observe()).character.experience;await idle(2000);
    game.check('defeat XP is once only',(await game.observe()).character.experience===earned);
  }
  game.check('retired class-shaped escorts never appear in the sampled native encounter',records.every(({state})=>state.diagnostic.entities.every(e=>e.kind<0x662||e.kind>0x667)));
}
