// Septavius wanders while Triforce alternates shield windows; short-range weapons must follow him.
export async function inRange(game,boss,range){
  for(let n=0;n<18;n++){
    const s=await game.inspect(),e=s.diagnostic.entities.find(e=>e.id===boss.id);
    if(!e)throw Error('Living Septavius left the observed range');
    const dx=e.pos.x-s.view.position.x,dy=e.pos.y-s.view.position.y;
    if(Math.hypot(dx,dy)<=range)return s;
    const x=(dx-dy)/Math.SQRT2,y=(dx+dy)/Math.SQRT2;
    const keys=Math.abs(x)>Math.abs(y)?[x>0?'d':'a']:[y>0?'s':'w'];
    await game.act({keys,duration_ms:300});
  }
  throw Error('Native movement did not close on Septavius');
}
