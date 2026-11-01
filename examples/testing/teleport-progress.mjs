import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {setTimeout as delay} from 'node:timers/promises';
import {connect} from '../../scripts/record-demo.mjs';

export const fixture={class:0x30e,level:20,seed:1247,world:-2,position:{x:76.5,y:72.5},equipment:[-1,-1,-1,-1],backpack:Array(8).fill(-1),stats:null};
export async function run(game) {
  const root=join(process.env.QA_RESULTS_DIR,'native'),config=JSON.parse(await readFile(join(root,'native.json'),'utf8'));
  const peer=await game.join({...fixture,seed:1248,position:{x:78.5,y:72.5}});
  const before=await game.observe(),inspected=await peer.inspect(),targetName=inspected.diagnostic.character.name;
  const browser=config.native.find(n=>n.actor===game.actor);assert(browser);
  const client=await connect({...browser,url:config.origin+'/',helper:config.browserTools});
  let captured=false,failure;
  const action=game.act([
    [{actor:game.actor,input:{keys:[],duration_ms:600}},{actor:peer.actor,input:{keys:['d'],duration_ms:600}}],
    {actor:game.actor,input:{keys:[],duration_ms:3900}},
  ],{name:'teleport-progress-ordinary-native',when:()=>captured}).then(()=>({ok:true}),error=>({ok:false,error}));
  try {
    let target;
    for(let i=0;;i++) {
      target=await peer.observe();
      if(!target.paused&&!target.lifecycle.busy&&target.game_ms>=before.game_ms+1000&&!(await game.observe()).lifecycle.busy)break;
      assert(i<150,'Ordinary native target walk did not settle');await delay(20);
    }
    game.check('ordinary native target walk explores new terrain',Math.hypot(target.position.x-inspected.view.position.x,target.position.y-inspected.view.position.y)>.5);
    const key=async type=>client.call('Input.dispatchKeyEvent',{type,key:'Enter',code:'Enter',windowsVirtualKeyCode:13});
    await key('keyDown');await key('keyUp');
    await client.call('Input.insertText',{text:'/teleport '+targetName});
    await key('keyDown');await key('keyUp');
    let receipt;
    for(let i=0;;i++) {
      const log=await readFile(join(root,'server.log'),'utf8');
      receipt=log.split('\n').slice(0,-1).filter(line=>line.startsWith('{')).map(line=>JSON.parse(line).fields).find(event=>event.message==='player teleported'&&event.player===before.session.player&&event.target===target.session.player);
      if(receipt)break;
      assert(i<100,'Ordinary native teleport did not succeed');await delay(20);
    }
    const during=await game.observe(),targetDuring=await peer.observe();
    game.check('ordinary native teleport lands at the actual target position',Math.hypot(during.position.x-targetDuring.position.x,during.position.y-targetDuring.position.y)<.01&&Math.hypot(receipt.x-targetDuring.position.x,receipt.y-targetDuring.position.y)<.01);
    game.check('both living character identities survive the command',during.lifecycle.alive&&targetDuring.lifecycle.alive&&during.session.character===before.session.character&&targetDuring.session.character===inspected.view.session.character);
    await writeFile(join(root,'teleport-progress-observed.json'),JSON.stringify({scope:'ordinary Human-target native compatibility, not a progress-pending race or autonomous-native proof',before,target,during,targetDuring,receipt},null,2));
    captured=true;
  } catch(error) {failure=error;}
  finally {
    try {
      const result=await action;
      const errors=[...(failure?[failure]:[]),...(!result.ok?[result.error]:[])];
      if(errors.length)throw new AggregateError(errors,'Ordinary native teleport failed: '+errors.map(String).join('; '));
    } finally {client.close();}
  }
  const after=await peer.observe(),sender=await game.observe();
  const saved=JSON.parse(execFileSync('python3',['-c','import sqlite3,json,sys\nc=sqlite3.connect("file:"+sys.argv[1]+"?mode=ro",uri=True);print(json.dumps([json.loads(r[0]) for r in c.execute("SELECT stats FROM characters WHERE (account=? AND id=?) OR (account=? AND id=?)",tuple(map(int,sys.argv[2:]))) ]))',join(config.state,'game.db'),String(sender.session.account),String(sender.session.character),String(after.session.account),String(after.session.character)],{encoding:'utf8'}));
  const savedSender=saved.find(ch=>ch.account===sender.session.account),savedTarget=saved.find(ch=>ch.account===after.session.account);
  assert(savedSender&&savedTarget,'Both exact character rows must exist');
  game.check('ordinary saves durably retain both living identities and teleport/exploration facts',savedSender.alive&&savedTarget.alive&&savedSender.id===sender.session.character&&savedTarget.id===after.session.character&&savedSender.lifetime.teleported&&savedTarget.lifetime.tiles>inspected.diagnostic.character.lifetime.tiles);
  await writeFile(join(root,'teleport-progress-saved.json'),JSON.stringify(saved,null,2));
}
