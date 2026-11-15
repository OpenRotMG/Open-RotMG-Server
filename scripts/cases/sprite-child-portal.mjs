import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
import {prepare as family} from './sprite-god.mjs';
const output=name=>join(process.env.QA_RESULTS_DIR,name);
export async function prepare(api){return {...await family(api),equipment:[0xa97,-1,-1,-1]};}
export async function run(game){
  const root=JSON.parse(await readFile(output('natural-placement.json'),'utf8')).root;
  const config=JSON.parse(await readFile(output('native/native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim(),samples=[];
  let protectedUntil=0;
  async function act(input,capture){
    const now=(await game.observe()).game_ms;
    if(now>=protectedUntil){
      const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:game.actor,change:{kind:'protection',condition:'invulnerable',duration_ms:60000}}})});
      const receipt=await response.json();assert(response.ok&&receipt.status==='succeeded',JSON.stringify(receipt));protectedUntil=now+60000;
    }
    await game.act(input,capture);const s=await game.inspect();samples.push(s);return s;
  }
  function walk(from,to){
    const dx=to.x-from.x,dy=to.y-from.y,x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2;
    return {keys:[Math.abs(x)>.15?(x>0?'d':'a'):null,Math.abs(y)>.15?(y>0?'s':'w'):null].filter(Boolean),duration_ms:Math.min(250,Math.max(50,Math.round(Math.hypot(dx,dy)*60)))};
  }
  try{
    const begin=await game.inspect();
    game.check('fresh natural family has no preexisting Sprite portal',begin.diagnostic.entities.some(e=>e.id===root.id)&&!begin.diagnostic.entities.some(e=>e.kind===0x70c));
    let portal;
    for(let attempt=0;attempt<12&&!portal;attempt++){
      let s=await game.inspect();const child=s.diagnostic.entities.find(e=>e.kind===0x653&&e.camp===root.camp);
      if(child){
        for(let step=0;step<60;step++){
          s=await game.inspect();const e=s.diagnostic.entities.find(e=>e.id===child.id);if(!e)break;
          const far=Math.hypot(e.pos.x-s.view.position.x,e.pos.y-s.view.position.y)>4.5;
          await act(far?walk(s.view.position,e.pos):{keys:[],duration_ms:250,target:{entity:e.id,camera_angle:-Math.PI/4,fire:true}});
        }
        game.check('native Energy Staff defeats selected child'+child.id,!(await game.inspect()).diagnostic.entities.some(e=>e.id===child.id));
      }else{assert(s.diagnostic.entities.some(e=>e.id===root.id),'No surviving parent remains for another ordinary child birth');await act({keys:[],duration_ms:10000});}
      portal=(await game.inspect()).diagnostic.entities.find(e=>e.kind===0x70c);
    }
    assert(portal,'No Sprite portal observed within12 bounded child attempts');
    for(let step=0;step<80;step++){
      const s=await game.inspect();if(Math.hypot(portal.pos.x-s.view.position.x,portal.pos.y-s.view.position.y)<.7)break;
      await act(walk(s.view.position,portal.pos));
    }
    await act({keys:[],duration_ms:1000},{name:'sprite-child-portal',when:()=>true});
    await act({keys:[],mouse:{x:700,y:575},duration_ms:700});
    for(let step=0;step<10&&(await game.observe()).session.world===1;step++)await act({keys:[],duration_ms:500});
    const end=await game.inspect();
    game.check('naturally dropped child portal admits the same living Wizard to Sprite World',end.view.map.name==='Sprite World'&&end.view.session.world!==1&&end.view.lifecycle.alive&&end.view.session.character===begin.view.session.character);
    game.check('portal admission retains earned child XP and unchanged supplied gear',end.view.character.experience>begin.view.character.experience&&JSON.stringify(end.view.character.items)===JSON.stringify(begin.view.character.items));
    await act({keys:[],duration_ms:1500},{name:'sprite-world-entry',when:()=>true});
    await writeFile(output('portal-outcomes.json'),JSON.stringify({root,portal,begin,end:await game.inspect()}));
  }finally{await writeFile(output('portal-samples.json'),JSON.stringify(samples));}
}
