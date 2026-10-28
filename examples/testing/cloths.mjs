import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
import {setTimeout as delay} from 'node:timers/promises';
import {connect} from '../../scripts/record-demo.mjs';
const xml=await readFile(new URL('../../data/objects.xml',import.meta.url),'utf8');
const cloths=[...xml.matchAll(/<Object type="(0x[\da-f]+)" id="([^"]+ Cloth)">([\s\S]*?)<\/Object>/g)].map(([,kind,name,body])=>{
  const texture=/<Tex([12])>(0x[\da-f]+)<\/Tex[12]>/.exec(body);assert(texture,name);
  return {kind:Number(kind),name,channel:Number(texture[1])-1,texture:Number(texture[2])};
});
assert.equal(cloths.length,142);
export const fixture={class:0x30e,level:20,seed:896,world:1,position:'spawn',equipment:[-1,-1,-1,-1],backpack:cloths.slice(0,8).map(c=>c.kind),stats:null};
export async function run(game){
  const out=process.env.QA_RESULTS_DIR,config=JSON.parse(await readFile(join(out,'native/native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim(),samples=[],captured=new Set();
  const browser=config.native.find(b=>b.actor===game.actor);assert(browser);
  const cdp=await connect({port:browser.port,target:browser.target,url:config.origin+'/',helper:config.browserTools});
  try{
  const rect=await cdp.eval('(()=>{const r=document.querySelector("ruffle-player").getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height}})()');assert(rect.width===800&&rect.height===600);
  async function change(change){
    const r=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:game.actor,change}})});
    const receipt=await r.json();assert(r.ok&&receipt.status==='succeeded',JSON.stringify(receipt));
  }
  await change({kind:'protection',condition:'invulnerable',duration_ms:60000});
  const initial=await game.observe();
  await game.act({keys:[],duration_ms:1500},{name:'cloths-before',when:v=>v.game_ms>initial.game_ms+500});
  await game.act({keys:[],duration_ms:300,drag:{from:{x:634,y:428},to:{x:634,y:385}}});
  game.check('cloth refuses equipment slot without spending',JSON.stringify((await game.observe()).character.items)===JSON.stringify(initial.character.items));
  const expected=[0,0];
  // Large/Small select channels, not atlas size: https://openrotmg.org/restoration/reference-data/client-123.5.1.swf (0x1200/0x1300).
  for(let i=0;i<cloths.length;i++){
    const c=cloths[i],slot=i%8+4;
    if(i===72)await change({kind:'protection',condition:'invulnerable',duration_ms:60000});
    if(i>=8&&i%8===0){
      for(let j=0;j<8;j++)await change({kind:'item',slot:j+4,item:cloths[i+j]?.kind??-1});
      await game.act({keys:[],duration_ms:150});
    }
    const before=await game.observe();assert.equal(before.character.items[slot],c.kind);
    expected[c.channel]=c.texture;
    const key=c.channel+'-'+(c.texture>>>24),capture=!captured.has(key);captured.add(key);
    if(capture){
      let moved=false;const start=before.game_ms;
      const action=game.act({keys:[],duration_ms:1500},{name:'preview-'+key,when:v=>moved&&v.game_ms>=start+700});
      const hover=(async()=>{
        const deadline=Date.now()+5000;
        while((await game.observe()).paused||!(await cdp.eval('document.querySelector("ruffle-player").ruffle().isPlaying'))){assert(Date.now()<deadline,'Preview must resume');await delay(20);}
        await cdp.eval('new Promise(ok=>requestAnimationFrame(()=>requestAnimationFrame(ok)))');
        await cdp.call('Input.dispatchMouseEvent',{type:'mouseMoved',x:rect.x+634+(i%4)*44,y:rect.y+428+Math.floor((slot-4)/4)*44});moved=true;
      })();
      const results=await Promise.allSettled([action,hover]);for(const r of results)if(r.status==='rejected')throw r.reason;
    }
    await game.act({keys:[String(slot-3)],duration_ms:300},capture?{name:'cloth-'+key,when:v=>v.character.items[slot]===-1}:undefined);
    const after=await game.observe(),diagnostic=(await game.inspect()).diagnostic.character;
    game.check(c.name+' consumes once and preserves other slots',after.character.items[slot]===-1&&after.character.inventory_revision===before.character.inventory_revision+1&&after.character.items.every((v,j)=>j===slot||v===before.character.items[j]));
    game.check(c.name+' updates only its texture channel',diagnostic.tex1===expected[0]&&diagnostic.tex2===expected[1]);
    samples.push({item:c,before:before.character,after:after.character,textures:[diagnostic.tex1,diagnostic.tex2]});
  }
  const spent=await game.observe();await game.act({keys:['1'],duration_ms:200});
  game.check('spent cloth cannot be reused',(await game.observe()).character.inventory_revision===spent.character.inventory_revision);
  await game.act({keys:['Insert'],duration_ms:1200},{name:'cloths-reconnected',when:v=>v.world==='Nexus'});
  const reconnected=await game.observe(),persisted=(await game.inspect()).diagnostic.character;
  game.check('normal Nexus reconnect preserves identity inventory and both textures',reconnected.world==='Nexus'&&reconnected.session.character===initial.session.character&&JSON.stringify(reconnected.character.items)===JSON.stringify(spent.character.items)&&persisted.tex1===expected[0]&&persisted.tex2===expected[1]);
  await writeFile(join(out,'cloth-observations.json'),JSON.stringify({preparedInventoryAndProtection:true,samples},null,2));
  }finally{cdp.close();}
}
