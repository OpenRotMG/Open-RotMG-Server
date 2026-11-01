import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
const output=name=>join(process.env.QA_RESULTS_DIR,name);
export const realmSeed=27;
export const fixture={class:0x30e,level:20,seed:1282,world:1,position:{x:1085.5,y:888.5},equipment:[0xb08,-1,-1,-1],backpack:Array(8).fill(-1),stats:[670,385,75,0,0,75,0,0]};
export async function run(game){
  const samples=[],rootId=252189;
  const root=s=>s.diagnostic.entities.find(e=>e.id===rootId);
  async function act(input,capture){await game.act(input,capture);const s=await game.inspect();samples.push(s);return s;}
  try{
    const arrival=await game.inspect();
    game.check('prepared Realm contains the naturally placed 2000-HP Leviathan',root(arrival)?.kind===0x6d8&&root(arrival).hp===2000);
    const config=JSON.parse(await readFile(output('native/native.json'),'utf8'));
    const token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}})});
    const protection=await response.json();assert(response.ok&&protection.status==='succeeded',JSON.stringify(protection));
    for(let n=0;n<12&&root(await game.inspect());n++)await act({keys:[],duration_ms:1000,target:{entity:rootId,camera_angle:-Math.PI/4,fire:true}});
    const dead=await act({keys:[],duration_ms:800},{name:'god-loot-combat',when:()=>true});
    game.check('ordinary native staff fire defeats the natural god and earns XP',!root(dead)&&dead.view.character.experience>arrival.view.character.experience);
    const events=(await readFile(output('native/server.log'),'utf8')).split('\n').slice(0,-1).filter(l=>l.startsWith('{')).map(l=>JSON.parse(l)).filter(e=>e.fields?.message==='god equipment bag allocated');
    const bag=dead.diagnostic.entities.find(e=>events.some(event=>event.fields.entity===e.id));
    await writeFile(output('ordinary-drop.json'),JSON.stringify({arrival,dead,events,bag}));
    game.check('ordinary combat produces an observed god equipment bag for native pickup',!!bag);
    const visible=dead.view.objects.find(e=>e.id===bag.id);
    game.check('native receives the authoritative god bag position',!!visible&&Math.hypot(visible.position.x-bag.pos.x,visible.position.y-bag.pos.y)<.001);
    for(let n=0;n<60;n++){
      const p=(await game.observe()).position,dx=bag.pos.x-p.x,dy=bag.pos.y-p.y;
      if(Math.hypot(dx,dy)<.6)break;
      const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2;
      await act({keys:[...(Math.abs(x)>.1?[x>0?'d':'a']:[]),...(Math.abs(y)>.1?[y>0?'s':'w']:[])],duration_ms:Math.max(30,Math.min(250,Math.round(Math.hypot(dx,dy)*180)))});
    }
    const near=await game.observe();
    game.check('native walking reaches the earned bag',Math.hypot(near.position.x-bag.pos.x,near.position.y-bag.pos.y)<.85);
    await act({keys:[],duration_ms:600},{name:'god-loot-pickup',when:()=>true});
    const slot=bag.items.findIndex(i=>i>=0),item=bag.items[slot],before=await game.observe();
    await act({keys:[],duration_ms:400,drag:{from:{x:634+(slot%4)*44,y:528+Math.floor(slot/4)*44},to:{x:634,y:428}}});
    const after=await game.observe();
    game.check('native pickup commits the earned god item once',after.character.items[4]===item&&after.character.inventory_revision===before.character.inventory_revision+1);
    game.check('pickup retains the same living prepared character',after.lifecycle.alive&&after.session.character===arrival.view.session.character);
    await writeFile(output('pickup.json'),JSON.stringify({bag,before,after}));
  }finally{await writeFile(output('samples.json'),JSON.stringify(samples));}
}
