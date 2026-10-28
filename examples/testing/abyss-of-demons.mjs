import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join,resolve} from 'node:path';
import {execFileSync} from 'node:child_process';

export const fixture={class:0x30e,level:20,seed:859,world:'abyss-of-demons',worldSeed:Number(process.env.ABYSS_SEED??5),position:'spawn',equipment:[0xb08,0xb24,0xb05,0xacd],backpack:Array(8).fill(-1),stats:[670,385,75,25,50,75,40,60]};
export async function run(game){
  const out=process.env.QA_RESULTS_DIR,root=join(out,'native'),distance=(a,b)=>Math.hypot(a.x-b.x,a.y-b.y);
  const initial=await game.observe(),initialCount=(await game.inspect()).diagnostic.character.abysses,report=JSON.parse(await readFile(join(root,'result.json'),'utf8'));
  const token=(await readFile(join(root,'state/control.token'),'utf8')).trim();
  const snapshots=[],seen=new Set(),renewals=[];let serial=0,protectedUntil=0;
  async function command(command){
    const path=join(out,'request.json');await writeFile(path,JSON.stringify({id:'abyss-'+serial++,command}));
    return JSON.parse(execFileSync(process.execPath,[resolve('scripts/game.mjs'),join(root,'native.json'),path],{encoding:'utf8'}));
  }
  async function map(){const s=await game.inspect(),r=await fetch(report.origin+'/dev/map?player='+s.diagnostic.player,{headers:{Authorization:'Bearer '+token}});assert(r.ok);return r.json();}
  async function save(name){const s=await game.inspect();snapshots.push({name,...s});await writeFile(join(out,'journey.json'),JSON.stringify(snapshots));return s;}
  async function act(input,capture){
    let v=await game.observe();assert(v.lifecycle.alive,'Abyss character died');
    if(protectedUntil&&protectedUntil-v.game_ms<15000){
      const position=v.position,[x,y]=originalSpawn;
      await command({op:'cheat',actor:game.actor,change:{kind:'teleport',position:{x,y}}});
      while(v.game_ms<protectedUntil){await game.act({keys:[],duration_ms:Math.min(10000,protectedUntil-v.game_ms)});v=await game.observe();assert(v.lifecycle.alive,'Safe protection renewal failed');}
      await command({op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invincible',duration_ms:60000}});protectedUntil=v.game_ms+60000;
      await command({op:'cheat',actor:game.actor,change:{kind:'teleport',position}});renewals.push({game_ms:v.game_ms,position});
      await writeFile(join(out,'protection-renewals.json'),JSON.stringify(renewals));
    }
    if(!protectedUntil){await command({op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invincible',duration_ms:60000}});protectedUntil=v.game_ms+60000;}
    const result=await game.act(input,capture);for(const e of (await game.observe()).nearby)seen.add(e.kind);return result;
  }
  const originalSpawn=[initial.position.x,initial.position.y];
  const original=await map(),idols=original.entities.filter(e=>e.kind===0xe1d),boss=original.entities.find(e=>e.kind===0x90a);assert(boss);
  await writeFile(join(out,'generated-map.json'),JSON.stringify(original));
  game.check('generated Abyss has natural Malphas and ordinary demons',boss.hp===10000&&original.entities.some(e=>e.kind>=0x66d&&e.kind<=0x672));
  const blocked=new Set(original.blocked.map(p=>p.join(','))),nodes=new Set();
  for(let y=1;y<original.height;y++)for(let x=1;x<original.width;x++)if([[x,y],[x-1,y],[x,y-1],[x-1,y-1]].every(p=>!blocked.has(p.join(','))))nodes.add(y*original.width+x);
  const point=i=>({x:i%original.width,y:Math.floor(i/original.width)}),nearest=p=>[...nodes].reduce((a,b)=>distance(point(a),p)<distance(point(b),p)?a:b);
  function route(start,goal){
    const from=nearest(start),to=nearest(goal),parents=new Map([[from,null]]),queue=[from];
    for(let n=0;n<queue.length&&!parents.has(to);n++)for(const next of [queue[n]+1,queue[n]-1,queue[n]+original.width,queue[n]-original.width])if(nodes.has(next)&&!parents.has(next)){parents.set(next,queue[n]);queue.push(next);}
    assert(parents.has(to),'Generated Abyss destination unreachable');const path=[];
    for(let i=to;i!==null;i=parents.get(i))path.push(i);path.reverse();
    return path.filter((i,n)=>n===0||n===path.length-1||i-path[n-1]!==path[n+1]-i).map(point);
  }
  async function walk(goal){
    for(const p of route((await game.observe()).position,goal)){
      let reached=false;
      for(let n=0;n<50;n++){
        const v=await game.observe(),dx=p.x-v.position.x,dy=p.y-v.position.y;
        if(Math.max(Math.abs(dx),Math.abs(dy))<.45){reached=true;break;}
        const horizontal=Math.abs(dx)>Math.abs(dy),delta=horizontal?dx:dy;
        await act({keys:horizontal?(delta>0?['d','s']:['a','w']):(delta>0?['a','s']:['d','w']),duration_ms:Math.max(50,Math.min(500,Math.round(Math.abs(delta)/7.5*1000)))});
      }
      assert(reached,'Native walking stalled at '+JSON.stringify(p));
    }
  }
  async function defeat(enemy){
    for(let n=0;n<180;n++){
      const s=await game.inspect(),e=s.diagnostic.entities.find(e=>e.id===enemy.id);if(!e)return;
      if(distance(s.view.position,e.pos)>4)await walk(e.pos);
      await act({keys:s.view.character.mp>=85?[' ']:[],duration_ms:500,target:{entity:enemy.id,camera_angle:-Math.PI/4,fire:true}});
    }
    throw Error('Native combat did not defeat '+enemy.id);
  }
  async function collect(enemy,name){
    const all=await map(),bags=all.entities.filter(e=>e.items.includes(0xa34)&&distance(e.pos,enemy.pos)<12);
    assert(bags.length===1,'Expected one private paired-potion bag');const bag=bags[0];
    game.check(name+' grants private DEF and VIT',bag.items.includes(0xa20)&&bag.extra_stats.some(([stat,owner])=>stat===54&&owner===initial.session.account));
    await walk(bag.pos);await act({keys:[],duration_ms:700},{name:name+'-loot',when:()=>true});
    const before=await game.observe(),empty=before.character.items.findIndex((i,n)=>n>=4&&i===-1),slot=bag.items.indexOf(0xa20);assert(empty>=4);
    await act({keys:[],duration_ms:350,drag:{from:{x:634+44*(slot%4),y:528+44*Math.floor(slot/4)},to:{x:634+44*((empty-4)%4),y:428+44*Math.floor((empty-4)/4)}}});
    const picked=await game.observe();game.check(name+' Defense pickup commits once',picked.character.items[empty]===0xa20&&picked.character.inventory_revision===before.character.inventory_revision+1);
  }
  await act({keys:[],duration_ms:900},{name:'abyss-entry',when:()=>true});
  for(const [index,idol] of idols.entries()){
    await walk({x:idol.pos.x,y:idol.pos.y+13});
    const before=await save('idol-'+index+'-approach');
    await walk({x:idol.pos.x,y:idol.pos.y+3});
    const dry=await save('idol-'+index+'-dry'),entrance=dry.view.tiles.filter(t=>Math.abs(t.x+.5-idol.pos.x)<=1&&t.y+.5>=idol.pos.y+7&&t.y+.5<=idol.pos.y+13);
    game.check('Idol '+index+' entrance is dry before damage',entrance.length>=9&&entrance.every(t=>t.ground===0x3c));
    await act({keys:[],duration_ms:300,target:{entity:idol.id,camera_angle:-Math.PI/4,fire:true}});
    await act({keys:[],duration_ms:1500},{name:'idol-'+index+'-active',when:()=>true});
    const active=await save('idol-'+index+'-active');
    game.check('Idol '+index+' damage activates local lava',entrance.every(t=>active.view.tiles.some(a=>a.x===t.x&&a.y===t.y&&a.ground===0x70)));
    await walk({x:idol.pos.x,y:idol.pos.y+13});await act({keys:[],duration_ms:2500});
    game.check('Idol '+index+' retreat keeps boss and uncleared character',(await map()).entities.some(e=>e.id===boss.id)&&(await game.inspect()).diagnostic.character.abysses===initialCount);
    await walk({x:idol.pos.x,y:idol.pos.y+3});await defeat(idol);await collect(idol,'idol-'+index);
    const dead=await save('idol-'+index+'-defeated');
    game.check('Idol '+index+' cannot clear the main dungeon',dead.diagnostic.character.abysses===initialCount&&(await map()).entities.some(e=>e.id===boss.id)&&!(await map()).entities.some(e=>e.kind===0x704));
    game.check('Idol '+index+' defeat restores the same entrance tiles',entrance.every(t=>dead.view.tiles.some(a=>a.x===t.x&&a.y===t.y&&a.ground===0x3c)));
    await walk({x:idol.pos.x,y:idol.pos.y+13});
    game.check('Idol '+index+' restores access without disconnecting',(await game.observe()).session.character===before.view.session.character);
  }
  await walk({x:boss.pos.x+2,y:boss.pos.y+3});
  // Later comparator timings, not authenticated original numbers: https://github.com/moistosaurus/realm-src/blob/1855c017eab870d13f0b61b1cf0b9e66a01922ff/wServer/logic/db/BehaviorDb.Abyss.cs
  for(let n=0;n<6;n++)await act({keys:[],duration_ms:9000},n===3?{name:'malphas-phases',when:()=>true}:undefined);
  const log=await readFile(join(root,'server.log'),'utf8'),phases=log.split('\n').filter(s=>s.includes('Abyss encounter phase entered')).map(s=>JSON.parse(s).fields.phase);
  game.check('natural Malphas executes all seven phase states',[0,1,2,3,4,5,6].every(p=>phases.includes(p)));
  await defeat(boss);await collect(boss,'malphas');
  const dead=await save('malphas-defeated');
  game.check('Malphas grants exactly one Abyss completion',dead.diagnostic.character.abysses===initialCount+1);
  await act({keys:[],duration_ms:1500});game.check('completion cannot repeat',(await game.inspect()).diagnostic.character.abysses===dead.diagnostic.character.abysses);
  game.check('native traversal encountered ordinary demons',[...seen].some(k=>k>=0x66d&&k<=0x672));
  const portal=(await map()).entities.find(e=>e.kind===0x704);assert(portal);await walk(portal.pos);
  await act({keys:[],duration_ms:800},{name:'abyss-exit',when:()=>true});await act({keys:[],mouse:{x:700,y:575},duration_ms:1700});
  const returned=await save('returned');
  game.check('completion exit preserves living character and rewards',returned.view.map.name==='Realm'&&returned.view.session.character===initial.session.character&&returned.view.lifecycle.alive&&returned.diagnostic.character.abysses===dead.diagnostic.character.abysses&&returned.view.character.items.filter(i=>i===0xa20).length===idols.length+1);
  await writeFile(join(out,'coverage.json'),JSON.stringify({seed:fixture.worldSeed,idols:idols.length,seen:[...seen],phases,renewals,scope:'Prepared generated dungeon spawn, supplied level/gear, timed invincibility and explicit safe-spawn/return placements for nonoverlapping renewal. Native walking/combat/loot/exit between placements, not uninterrupted earned traversal or survival difficulty.'}));
}
