export const fixture = {
  class:0x300, level:20, seed:722, world:-2, position:{x:88.5,y:58.5},
  equipment:[-1,-1,-1,-1], backpack:[-1,-1,-1,-1,-1,-1,-1,-1], stats:null,
};
export async function run(game) {
  const before=await game.observe();
  await game.act({keys:[],duration_ms:9000},{name:'merchant-offer',when:view=>view.game_ms>=before.game_ms+6500});
  const offer=(await game.inspect()).diagnostic.entities.find(e=>e.pos.x===88.5&&e.pos.y===58.5);
  game.check('observed offer has a 25% discount and costs 90 Gold',offer.extra_stats.some(([id,v])=>id===44&&v===25)&&offer.extra_stats.some(([id,v])=>id===36&&v===90));
  await game.act({keys:[],duration_ms:9000},{name:'merchant-discount',when:view=>view.game_ms>=before.game_ms+11500});
  for(let i=0;i<17;i++) {
    await game.act({keys:[],duration_ms:10000},i===11?{name:'merchant-going-soon',when:view=>view.game_ms>=125000}:undefined);
  }
  game.check('expired offer is no longer visible',(await game.inspect()).diagnostic.entities.every(e=>e.id!==offer.id));
  const after=await game.observe();
  game.check('viewing offers and expiry do not change inventory or revision',JSON.stringify(after.character.items)===JSON.stringify(before.character.items)&&after.character.inventory_revision===before.character.inventory_revision);
  game.check('same living character',after.lifecycle.alive&&after.session.character===before.session.character);
}
