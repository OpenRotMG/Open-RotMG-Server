import assert from 'node:assert/strict';
import {readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {pathToFileURL} from 'node:url';
import {randomUUID} from 'node:crypto';
import {setTimeout as wait} from 'node:timers/promises';

export const fixture={class:0x30e,level:20,seed:781,world:-2,position:'spawn',
  equipment:[0xa97,0xa2e,-1,-1],backpack:[0x1071,0x1109,-1,-1,-1,-1,-1,-1],stats:null};
export async function run(game){
  const out=process.argv[process.argv.indexOf('--run-dir')+1];
  const config=JSON.parse(await readFile(join(out,'native.json'),'utf8'));
  const token=(await readFile(join(config.state,'control.token'),'utf8')).trim();
  async function api(command){
    const response=await fetch(config.origin+'/dev/testing',{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':'application/json'},body:JSON.stringify({id:'legends-'+randomUUID(),command})});
    const result=await response.json();assert(response.ok&&result.status!=='failed',JSON.stringify(result));return result;
  }
  async function list(span,account=-1,char=-1){
    const response=await fetch(config.origin+'/legacy-api/fame/list',{method:'POST',body:new URLSearchParams({timespan:span,accountId:String(account),charId:String(char)})});
    assert(response.ok);return await response.text();
  }
  const ids=xml=>[...xml.matchAll(/<FameListElem accountId="(\d+)" charId="(\d+)"/g)].map(m=>Number(m[2]));
  const initial=await list('all');game.check('living native character is absent from Legends',ids(initial).length===0);
  const undyed=await game.inspect();
  await game.act({keys:[],duration_ms:900},{name:'native-undyed',when:()=>true});
  await game.act({keys:['1'],duration_ms:300});
  await game.act({keys:['2'],duration_ms:900},{name:'native-dyed',when:view=>view.character.items[5]===-1});
  const dyed=await game.inspect(),textures=[0x01ff0000,0x010000ff];
  game.check('native dye use consumes both prepared vials once',dyed.view.character.items[4]===-1&&dyed.view.character.items[5]===-1&&dyed.view.character.inventory_revision===undyed.view.character.inventory_revision+2);
  game.check('native dye use commits clothing and accessory colors',dyed.diagnostic.character.tex1===textures[0]&&dyed.diagnostic.character.tex2===textures[1]);
  await writeFile(join(out,'native-dyes.json'),JSON.stringify({undyed,dyed,prepared:true},null,2));
  const world=(await api({op:'world',template:'ocean-trench'})).result.world;
  const info=await api({op:'world_info',world});
  const spawn=info.spawn_points[0],position={x:spawn[0],y:spawn[1]+2},victims=[];
  for(let i=0;i<21;i++){
    const result=await api({op:'spawn',spec:{...fixture,class:i%2?0x300:0x30e,level:1,seed:781+i,world,position,backpack:i===0?fixture.backpack:Array(8).fill(-1),equipment:i%2?[0xa14,-1,-1,-1]:fixture.equipment,guid:'legends-victim-'+i,stats:[100,100,12,0,10,15,12,12],controller:'direct',profile:null}});
    const actor=result.result.actor,account=result.result.account;
    const before=(await api({op:'inspect',actor})).diagnostic;
    assert(before.entities.filter(e=>e.kind===0x731).every(e=>Math.hypot(e.pos.x-position.x,e.pos.y-position.y)>=1),'Victims must be outside oxygen vents');
    victims.push({actor,account,name:before.character.name});
  }
  const dyedVictim=victims[0];
  await Promise.all([game.act({keys:[],duration_ms:1800}), (async()=>{
    const end=Date.now()+10000;
    while((await game.observe()).paused){assert(Date.now()<end,'Native dye action did not resume');await wait(30);}
    for(const slot of [4,5])await api({op:'act',actor:dyedVictim.actor,action:{command:'use',slot,x:position.x,y:position.y},duration_ms:0,pauseAfter:false});
  })()]);
  const applied=(await api({op:'inspect',actor:dyedVictim.actor})).diagnostic;
  game.check('prepared direct death fixture uses the same ordinary dyes',applied.character.tex1===textures[0]&&applied.character.tex2===textures[1]&&applied.character.items[4]===-1&&applied.character.items[5]===-1);
  await writeFile(join(out,'direct-dyes.json'),JSON.stringify(applied,null,2));
  let xml;
  for(let i=0;i<7;i++){
    await game.act({keys:[],duration_ms:9000});xml=await list('all');if(ids(xml).length===20)break;
  }
  const ranked=ids(xml);game.check('twenty committed deaths fill the native leaderboard',ranked.length===20);
  game.check('ordinarily dyed death is visible as the first ranked row',ranked[0]===dyedVictim.actor);
  const absent=victims.filter(v=>!ranked.includes(v.actor));assert.equal(absent.length,1);
  const own=await list('all',absent[0].account,absent[0].actor);
  game.check('eligible own death below the cap is an unranked row',own.includes(`<MyFameListElem accountId="${absent[0].account}" charId="${absent[0].actor}">`));
  const foreign=await list('all',-1,absent[0].actor);
  game.check('foreign requested character does not produce an own row',!foreign.includes('<MyFameListElem'));
  await writeFile(join(out,'legends-fixture.json'),JSON.stringify({prepared:true,victims,ranked,absent},null,2));
  await writeFile(join(out,'legends-own.xml'),own);
  const {withAgentTab}=await import(pathToFileURL(join(process.env.PI_BROWSER_TOOLS,'script-tab.mjs')).href);
  const browser=JSON.parse(await readFile(join(out,'result.json'),'utf8')).browser;
  await withAgentTab(browser.port,browser.owner,async page=>{
    const requests=[],network=[];
    page.on('response',response=>{network.push({url:response.url(),status:response.status()});if(['/legacy-api/fame/list','/legacy-api/char/fame'].includes(new URL(response.url()).pathname))requests.push(response);});
    await page.setViewport({width:1280,height:900});
    const cdp=await page.createCDPSession();await cdp.send('Emulation.setFocusEmulationEnabled',{enabled:true});
    await page.goto(config.origin);
    try{await page.waitForFunction(()=>document.querySelector('ruffle-player')?.ruffle().isPlaying===true);}
    catch(error){await page.screenshot({path:join(out,'legends-loading-failure.png')});await writeFile(join(out,'legends-loading-failure.json'),JSON.stringify({url:page.url(),state:await page.evaluate(()=>({visibility:document.visibilityState,playing:document.querySelector('ruffle-player')?.ruffle().isPlaying}))}));throw error;}
    await page.evaluate(()=>{const m=document.querySelector('ruffle-player').shadowRoot.querySelector('#hardware-acceleration-modal');if(m&&!m.classList.contains('hidden'))m.querySelector('.close-modal').click();});
    const rect=await page.evaluate(()=>{const p=document.querySelector('ruffle-player');p.focus({preventScroll:true});const r=p.getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height,vw:innerWidth,vh:innerHeight};});assert.equal(rect.width,800);assert.equal(rect.height,600);assert(rect.x>=0&&rect.y>=0&&rect.x+800<=rect.vw&&rect.y+600<=rect.vh,'Native menu must fit inside the input viewport');
    const frames=()=>page.evaluate(()=>new Promise(ok=>requestAnimationFrame(()=>requestAnimationFrame(ok))));
    async function click(x,y){const p={x:rect.x+x,y:rect.y+y,button:'left',clickCount:1};await cdp.send('Input.dispatchMouseEvent',{type:'mouseMoved',buttons:0,...p});await frames();try{await cdp.send('Input.dispatchMouseEvent',{type:'mousePressed',buttons:1,...p});}finally{await cdp.send('Input.dispatchMouseEvent',{type:'mouseReleased',buttons:0,...p});}await frames();}
    async function capture(name){await wait(700);await page.screenshot({path:join(out,name+'.png'),clip:{x:rect.x,y:rect.y,width:800,height:600}});}
    async function response(path,count){const end=Date.now()+15000;while(requests.filter(r=>new URL(r.url()).pathname===path).length<count){if(Date.now()>=end){await capture('menu-response-failure');await writeFile(join(out,'menu-response-failure.json'),JSON.stringify({rect,network,state:await page.evaluate(()=>({visibility:document.visibilityState,playing:document.querySelector('ruffle-player').ruffle().isPlaying}))}));assert.fail('Missing native response: '+path);}await wait(100);}const r=requests.filter(r=>new URL(r.url()).pathname===path)[count-1];assert.equal(r.status(),200);return await r.text();}
    await capture('title');await click(400,300);await click(580,545);await capture('menu-click');
    for(const [i,span,x] of [[0,'week',35],[1,'month',125],[2,'all',235]]){
      if(i)await click(x,85);
      const body=await response('/legacy-api/fame/list',i+1);
      game.check('native '+span+' tab loads the actual twenty death records',body.includes(`timespan="${span}"`)&&JSON.stringify(ids(body))===JSON.stringify(ranked));
      // Selected 123.5.1 Legends rows render archived Tex1/Tex2: https://github.com/OpenRotMG/Open-RotMG-Client/blob/39acbfbf3bf545c397d6ac94f0a9430d6d660ac2/client.swf
      const row=body.match(new RegExp(`<FameListElem accountId="${dyedVictim.account}" charId="${dyedVictim.actor}">([\\s\\S]*?)</FameListElem>`));
      game.check('native '+span+' response retains the ordinarily dyed death appearance',row&&textures.every((t,i)=>row[1].includes(`<Tex${i+1}>${t}</Tex${i+1}>`)));
      await writeFile(join(out,'native-'+span+'.xml'),body);await capture('legends-'+span);
    }
    await click(210,135);const detail=await response('/legacy-api/char/fame',1);
    game.check('native row selection opens the matching archived fame details',detail.includes(`<Char id="${ranked[0]}">`)&&detail.includes('<KilledBy>suffocation</KilledBy>')&&textures.every((t,i)=>detail.includes(`<Tex${i+1}>${t}</Tex${i+1}>`)));
    await writeFile(join(out,'native-fame.xml'),detail);await capture('legends-details');
  });
  const retained=await game.inspect();
  game.check('original native character remains alive after read-only Legends checks',retained.view.lifecycle.alive);
  game.check('native dye appearance survives the Legends menu checks',retained.diagnostic.character.tex1===textures[0]&&retained.diagnostic.character.tex2===textures[1]);
}
