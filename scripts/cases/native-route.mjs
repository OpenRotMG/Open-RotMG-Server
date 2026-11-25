import assert from 'node:assert/strict';

const distance=(a,b)=>Math.hypot(a.x-b.x,a.y-b.y);
const direction=(dx,dy)=>Math.abs(dx)>Math.abs(dy)?(dx>0?['d','s']:['a','w']):(dy>0?['a','s']:['d','w']);

// Half-tile search over a /dev/map snapshot keeping two-tile wall clearance. `soft` tiles hold destructible OccupySquare
// scenery (not FullOccupy): the walker passes beside them and shoots through one only when no open route exists.
// `hazard` tiles stay walkable but are avoided as route and endpoint whenever an alternative exists.
export function route(m,from,to,{open=[],soft=new Map(),avoid=[],hazard=new Set(),reach=1.6,standoff=false}={}){
  const W=m.width,tile=p=>Math.floor(p.y)*W+Math.floor(p.x),blocked=new Set(m.blocked.map(([x,y])=>y*W+x));
  for(const n of [...open,...soft.keys()])blocked.delete(n);
  for(const n of avoid)blocked.add(n);
  const stride=W*2+1,point=n=>({x:(n%stride)/2,y:Math.floor(n/stride)/2}),costs=new Map();
  const touching=(p,r)=>[-r,0,r].flatMap(dx=>[-r,0,r].map(dy=>({x:p.x+dx,y:p.y+dy})));
  const cost=n=>{if(costs.has(n))return costs.get(n);const p=point(n);let c=1;
    const wall=r=>touching(p,r).some(q=>q.x<0||q.y<0||q.x>=W||q.y>=m.height||blocked.has(tile(q)));
    // Prefer corridor centres: short native presses under- or overshoot, and a hugged wall then blocks the next turn.
    if(wall(.51))c=0;else if(touching(p,.3).some(q=>soft.has(tile(q))))c=41;else if(hazard.has(tile(p)))c=20;else if(wall(1.01))c=3;costs.set(n,c);return c;};
  const choices=[];for(let y=Math.round(from.y*2)-3;y<=Math.round(from.y*2)+3;y++)for(let x=Math.round(from.x*2)-3;x<=Math.round(from.x*2)+3;x++){const n=y*stride+x;if(cost(n)&&cost(n)<41)choices.push([n,distance(from,point(n))]);}
  choices.sort((a,b)=>a[1]-b[1]);assert(choices.length,'No safe endpoint '+JSON.stringify(from));
  const start=choices[0][0],dist=new Map([[start,0]]),parent=new Map([[start,null]]),buckets=[[start]];
  for(let d=0;d<buckets.length;d++)for(const at of buckets[d]??[]){if(dist.get(at)!==d)continue;for(const next of [at-1,at+1,at-stride,at+stride]){const c=cost(next),nd=d+c;if(c&&nd<(dist.get(next)??Infinity)){dist.set(next,nd);parent.set(next,at);(buckets[nd]??=[]).push(next);}}}
  const byGoal=(a,b)=>distance(point(a),to)-distance(point(b),to),byRoute=(a,b)=>dist.get(a)-dist.get(b);
  const goals=[...dist.keys()].filter(n=>distance(point(n),to)<reach).sort((a,b)=>hazard.has(tile(point(a)))-hazard.has(tile(point(b)))||(standoff?byRoute(a,b)||byGoal(a,b):byGoal(a,b)||byRoute(a,b)));
  assert(goals.length,'No ordinary route '+JSON.stringify(to));const path=[];for(let at=goals[0];at!==null;at=parent.get(at))path.push(at);path.reverse();
  const first=path.findIndex(n=>cost(n)>40),barrier=first<0?null:touching(point(path[first]),.3).map(q=>soft.get(tile(q))).find(Boolean);
  const points=(first<0?path:path.slice(0,first)).map(point);
  return {barrier,points:points.filter((p,i)=>i===0||i===points.length-1||(p.x-points[i-1].x)!==(points[i+1].x-p.x)||(p.y-points[i-1].y)!==(points[i+1].y-p.y))};
}

// Re-planned native legs (at most eight per action, ending after a long one) keep journeys within the receipt budget and correct drift.
// `ready()` renews any prepared protection and returns the milliseconds the walker may act before it lapses.
export async function walk({client,map,to,options=()=>({}),ready,record=()=>{}}){
  let stalled=0,last;
  for(let n=0;n<80;n++){
    let budget=Math.min(9500,await ready());
    const m=await map(),v=await client.observe(),planned=options(m);await record({from:v.position,to,map:m});
    const {points,barrier}=route(m,v.position,to,planned);
    await record({from:v.position,to,map:m,points,barrier});
    if(!barrier&&distance(points.at(-1),v.position)<.5)return;
    const speed=4+5.6*v.character.stats.speed/75,legs=[];let at=v.position;
    for(const p of points)for(const q of [{x:p.x,y:at.y},p]){const dx=q.x-at.x,dy=q.y-at.y,ms=Math.max(50,Math.round(Math.hypot(dx,dy)/speed*1000));if(Math.hypot(dx,dy)<(planned.clearBlockedApproach ? .075 : .25)||legs.length>=8)continue;const duration_ms=Math.min(ms,budget);if(duration_ms<1)continue;budget-=duration_ms;legs.push({actor:client.actor,input:{keys:direction(dx,dy),duration_ms}});at=q;if(duration_ms>1000)budget=0;}
    // Prepared protection cannot be renewed early; let a nearly spent window lapse before planning more legs.
    if(budget<300&&!legs.length){await client.act({keys:[],duration_ms:Math.max(1,budget+50)});continue;}
    if(legs.length){
      // Sub-100ms presses can leave the walker a fraction of a tile short of its snapped node; inside `reach` that is arrival.
      if(last&&distance(last,v.position)<.1){
        if(++stalled>=4&&distance(to,v.position)<(planned.reach??1.6))return;
        if(stalled<=2&&planned.clearBlockedApproach&&points.length>1){
          // The native half-cell slide can block a straight press at a wall corner even
          // after the gap is open. A short diagonal lets it slide along the other axis.
          const dx=Math.sign(points[1].x-points[0].x),dy=Math.sign(points[1].y-points[0].y),side=stalled===1?-1:1;
          const keys=dx?(dx===side?[dx>0?'s':'w']:[dx>0?'d':'a']):(dy===side?[dy>0?'s':'w']:[dy>0?'a':'d']);
          await client.act({keys,duration_ms:250});continue;
        }
        if(stalled>=2&&stalled<=12&&planned.clearBlockedApproach){
          const wall=[...planned.soft.values()].filter(e=>distance(e.pos,v.position)<3).sort((a,b)=>distance(a.pos,v.position)-distance(b.pos,v.position))[0];
          if(wall){
            const dx=wall.pos.x-v.position.x,dy=wall.pos.y-v.position.y;
            await client.act({keys:[],duration_ms:1800,mouse:{x:300+(dx-dy)*35.355,y:325+(dx+dy)*35.355}});
            continue;
          }
        }
        assert(stalled<4,'Native movement stalled '+JSON.stringify({to,actual:v.position,points,barrier}));
      }else stalled=0;
      last=v.position;await client.act(legs);continue;
    }
    if(!barrier)return;
    assert(distance(barrier.pos,v.position)<3,'Planned obstacle is not beside the walker '+JSON.stringify({barrier:barrier.pos,actual:v.position}));
    const present=async()=>(await map()).entities.some(e=>e.id===barrier.id);
    for(let k=0;k<12&&await present();k++)await client.act({keys:[],duration_ms:1500,target:{entity:barrier.id,camera_angle:-Math.PI/4,fire:true}});
    assert(!await present(),'Ordinary fire did not clear the obstacle on the only route');
  }
  throw Error('Native walk exceeded its bound '+JSON.stringify(to));
}
