import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {randomUUID} from 'node:crypto';
export const fixture={class:0x30e,level:20,seed:893,world:'pirate-cave',position:'spawn',equipment:[-1,-1,-1,-1],backpack:[0x1539,0x1513,-1,-1,-1,-1,-1,-1],stats:null};
export async function run(game){
  const root=join(process.env.QA_RESULTS_DIR,'native'),records=[];
  const config=JSON.parse(await readFile(join(root,'native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  const xml=await readFile(new URL('../../data/objects.xml',import.meta.url),'utf8');
  const objects=[...xml.matchAll(/<Object\b[^>]*type="([^"]+)"[^>]*id="([^"]+)"[^>]*>([\s\S]*?)<\/Object>/g)];
  const roster=objects.filter(o=>/>PermaPet<\/Activate>/.test(o[3])).map(o=>({item:Number(o[1]),name:o[2],kind:Number(objects.find(p=>p[2]===/objectId="([^"]+)"[^>]*>PermaPet/.exec(o[3])[1])[1]),consumed:/<Consumable\s*\/>/.test(o[3])}));
  assert.equal(roster.length,66);
  const kinds=new Set(roster.map(o=>o.kind));
  async function save(label){
    const {view,diagnostic}=await game.inspect();
    const record={label,view,companions:diagnostic.entities.filter(e=>kinds.has(e.kind))};
    records.push(record);await writeFile(join(root,'pet-observations.json'),JSON.stringify(records));return record;
  }
  const visible=r=>r.view.objects.filter(o=>kinds.has(o.kind));
  const wait=()=>game.act({keys:[],duration_ms:10000});
  async function prepare(item){
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:randomUUID(),command:{op:'cheat',actor:game.actor,change:{kind:'item',slot:4,item}}})});
    const receipt=await response.json();assert(response.ok&&receipt.status==='succeeded',JSON.stringify(receipt));
  }
  const initial=await save('initial');
  await game.act({keys:['1'],duration_ms:600});
  const baby=await save('baby');
  game.check('Baby generator commits exactly one visible companion without consuming its item',visible(baby).length===1&&visible(baby)[0].kind===roster.find(o=>o.item===0x1539).kind&&baby.view.character.items[4]===0x1539&&baby.view.character.inventory_revision===initial.view.character.inventory_revision+1);
  await game.act({keys:['2'],duration_ms:300});
  const refused=await save('cooldown-refused');
  game.check('generator cooldown refuses replacement atomically',visible(refused)[0].id===visible(baby)[0].id&&refused.view.character.inventory_revision===baby.view.character.inventory_revision&&JSON.stringify(refused.view.character.items)===JSON.stringify(baby.view.character.items));
  await game.act({keys:['d'],duration_ms:700});
  await game.act({keys:[],duration_ms:1500},{name:'baby-following',when:()=>true});
  const followed=await save('following');
  game.check('Baby follows actual native movement',Math.hypot(followed.view.position.x-baby.view.position.x,followed.view.position.y-baby.view.position.y)>2&&Math.hypot(visible(followed)[0].position.x-followed.view.position.x,visible(followed)[0].position.y-followed.view.position.y)<1.5);
  await game.act({keys:['Insert'],duration_ms:800});
  const nexus=await save('nexus-transfer');
  game.check('ordinary Nexus reconnect preserves selection on the same living character',nexus.view.map.name==='Nexus'&&nexus.view.lifecycle.alive&&nexus.view.session.character===baby.view.session.character&&visible(nexus).length===1&&visible(nexus)[0].kind===visible(baby)[0].kind&&nexus.view.character.inventory_revision===baby.view.character.inventory_revision);
  await wait();await game.act({keys:['1'],duration_ms:350});
  const hidden=await save('toggle-off');
  game.check('reusing permanent generator dismisses without consumption',visible(hidden).length===0&&hidden.view.character.items[4]===0x1539&&hidden.view.character.inventory_revision===nexus.view.character.inventory_revision+1);
  // These are selected definitions, not proof every generator was historically sold: https://www.realmeye.com/wiki/pets-prior-to-release-12-0-0
  for(const pet of roster){
    await wait();await prepare(pet.item);
    await game.act({keys:[],duration_ms:350});
    const before=await save('prepared-'+pet.name);
    await game.act({keys:['1'],duration_ms:350});
    const after=await save('used-'+pet.name),seen=visible(after);
    game.check(pet.name+' publishes one selected companion and commits once',seen.length===1&&seen[0].kind===pet.kind&&after.view.character.items[4]===(pet.consumed?-1:pet.item)&&after.view.character.inventory_revision===before.view.character.inventory_revision+1);
    if(pet.consumed){
      await game.act({keys:['1'],duration_ms:350});const empty=await save('empty-'+pet.name);
      game.check(pet.name+' cannot consume or publish twice from empty slot',empty.view.character.inventory_revision===after.view.character.inventory_revision&&visible(empty)[0].id===seen[0].id);
    }
    if([0x151f,0x1521,0x1539,0x153f,0x1541].includes(pet.item))await game.act({keys:[],duration_ms:500},{name:'pet-'+pet.item.toString(16),when:()=>true});
  }
  const final=await save('final');
  game.check('prepared roster leaves identity base stats and equipment unchanged',final.view.lifecycle.alive&&final.view.session.character===initial.view.session.character&&JSON.stringify(final.view.character.base_stats)===JSON.stringify(initial.view.character.base_stats)&&JSON.stringify(final.view.character.items.slice(0,4))===JSON.stringify(initial.view.character.items.slice(0,4)));
}
