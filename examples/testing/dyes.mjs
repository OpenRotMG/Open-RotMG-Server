import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';

export const fixture={class:0x30e,level:20,seed:895,world:'pirate-cave',position:'spawn',equipment:[-1,-1,-1,-1],backpack:[0x1000,0x1101,0xa50,0xa50,0xa50,0xa50,0xa50,0xa50],stats:[670,385,50,25,50,50,0,0]};
export async function run(game){
  const out=process.env.QA_RESULTS_DIR,run=join(out,'native');
  const config=JSON.parse(await readFile(join(run,'native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim(),samples=[];
  async function refill(slot,item){
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:game.actor,change:{kind:'item',slot,item}}})});
    const result=await response.json();assert(response.ok&&result.status==='succeeded',JSON.stringify(result));
  }
  const objects=[...String(await readFile(new URL('../../data/objects.xml',import.meta.url))).matchAll(/<Object type="(0x[0-9a-f]+)" id="([^"]+)">([\s\S]*?)<\/Object>/gi)];
  const palette=objects.filter(m=>m[2].endsWith('Clothing Dye')).map(m=>({kind:Number(m[1]),name:m[2],texture:Number(m[3].match(/<Tex1>(.*?)<\/Tex1>/)[1])}));
  assert.equal(palette.length,141);
  for(const color of palette){
    const pair=objects.find(m=>Number(m[1])===color.kind+0x100);
    assert(pair&&pair[2]===color.name.replace('Clothing','Accessory')&&Number(pair[3].match(/<Tex2>(.*?)<\/Tex2>/)[1])===color.texture);
  }
  async function capture(client,name){const before=await client.observe();await client.act({keys:[],duration_ms:900},{name,when:v=>v.game_ms>=before.game_ms+400});}
  async function save(name,client){
    const sample={name,actor:client.actor,...await client.inspect()};samples.push(sample);
    await writeFile(join(out,'dye-observations.json'),JSON.stringify({preparedRefills:true,samples},null,2));return sample;
  }
  await capture(game,'dyes-before');
  const clearPreview=state=>state.diagnostic.entities.every(e=>!e.extra_stats.some(([id,kind])=>id===34&&kind>=0x1000&&kind<0x1400)||Math.hypot(e.pos.x-state.view.position.x,e.pos.y-state.view.position.y)>=1);
  for(const [name,x,y,stock] of [['clothing',75.5,58.5,0x1007],['accessory',84.5,54.5,0x1185]]){
    const client=await game.join({...fixture,world:-2,position:{x,y:y+1.6},backpack:[0x1071,0x1109,-1,-1,-1,-1,-1,-1]});
    await client.act([{actor:client.actor,input:{keys:['1'],duration_ms:200}},{actor:client.actor,input:{keys:['2'],duration_ms:300}}]);
    const before=await save(name+'-before-preview',client);await capture(client,name+'-before-preview');
    game.check(name+' starts clear of all appearance merchants',clearPreview(before));
    const merchant=before.diagnostic.entities.find(e=>e.extra_stats.some(([id,value])=>id===34&&value===stock));assert(merchant);
    await client.act({keys:['w'],duration_ms:160});
    await client.act({keys:[],duration_ms:400});
    const preview=await save(name+'-preview',client);await capture(client,name+'-preview');
    game.check(name+' preview enters the selected merchant radius',Math.hypot(preview.view.position.x-merchant.pos.x,preview.view.position.y-merchant.pos.y)<1);
    // Selected Player/Merchant render proximity previews without changing Tex1/Tex2: https://github.com/OpenRotMG/Open-RotMG-Client/blob/39acbfbf3bf545c397d6ac94f0a9430d6d660ac2/client.swf
    await client.act({keys:['s'],duration_ms:160});await client.act({keys:[],duration_ms:400});
    const cancelled=await save(name+'-cancelled',client);await capture(client,name+'-cancelled');
    game.check(name+' preview cancels clear of every appearance merchant',clearPreview(cancelled));
    for(const state of [before,preview,cancelled])game.check(name+' preview never consumes or persists the trial color',state.diagnostic.character.tex1===0x01ff0000&&state.diagnostic.character.tex2===0x010000ff&&JSON.stringify(state.view.character.items)===JSON.stringify(before.view.character.items)&&state.view.character.inventory_revision===before.view.character.inventory_revision);
  }
  for(const [i,color] of palette.entries()){
    const accessory=palette[(i+1)%palette.length];
    if(i){await refill(4,color.kind);await refill(5,accessory.kind+0x100);}
    const before=await game.observe();
    const result=await game.act([{actor:game.actor,input:{keys:[],duration_ms:100}},{actor:game.actor,input:{keys:['1'],duration_ms:200}},{actor:game.actor,input:{keys:['2'],duration_ms:200}},{actor:game.actor,input:{keys:[],duration_ms:500}}],{name:'palette-'+i.toString().padStart(3,'0'),when:v=>v.character.items[4]===-1&&v.character.items[5]===-1&&v.game_ms>=before.game_ms+700});
    const after=await save(color.name,game);
    game.check(color.name+' and next accessory consume exactly their slots once',after.view.character.inventory_revision===before.character.inventory_revision+2&&after.view.character.items.every((item,slot)=>slot===4||slot===5?item===-1:item===before.character.items[slot]));
    game.check(color.name+' and next accessory retain distinct texture channels',after.diagnostic.character.tex1===color.texture&&after.diagnostic.character.tex2===accessory.texture);
    game.check(color.name+' first native use preserves the second vial',result.steps[1].character.items[4]===-1&&result.steps[1].character.items[5]===accessory.kind+0x100&&result.steps[1].character.inventory_revision===before.character.inventory_revision+1);
  }
  const retained=await save('before-reconnect',game);
  await game.act({keys:['Insert'],duration_ms:1000});await game.act({keys:[],duration_ms:600});
  const reconnected=await save('reconnected',game);await capture(game,'dyes-reconnected');
  game.check('normal Nexus reconnect preserves living identity, both colors and consumed inventory',reconnected.view.map.name==='Nexus'&&reconnected.view.lifecycle.alive&&reconnected.view.session.character===retained.view.session.character&&reconnected.diagnostic.character.tex1===retained.diagnostic.character.tex1&&reconnected.diagnostic.character.tex2===retained.diagnostic.character.tex2&&JSON.stringify(reconnected.view.character.items)===JSON.stringify(retained.view.character.items)&&reconnected.view.character.inventory_revision===retained.view.character.inventory_revision);
}
