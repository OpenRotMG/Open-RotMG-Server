import assert from 'node:assert/strict';
import {execFile,spawn} from 'node:child_process';
import {promisify} from 'node:util';
import {open,readFile,writeFile} from 'node:fs/promises';
import {join} from 'node:path';
import {createServer} from 'node:net';
import {randomUUID} from 'node:crypto';
import {setTimeout as delay} from 'node:timers/promises';

const [native,client]=process.argv.slice(2),out=process.env.QA_RESULTS_DIR;
assert(native&&client&&out,'Run through qa.py exec with a closed native run and its exact client directory');
const receipt=JSON.parse(await readFile(join(native,'result.json'),'utf8'));
assert(receipt.status==='passed'&&receipt.cleanupErrors.length===0,'Native lifecycle must have passed and closed');
const config=JSON.parse(await readFile(join(native,'native.json'),'utf8')),state=config.state;
const exec=promisify(execFile);
const query=async sql=>JSON.parse((await exec('python3',['-c','import sqlite3,json,sys; c=sqlite3.connect(sys.argv[1]); rows=c.execute(sys.argv[2]).fetchall(); c.commit(); print(json.dumps(rows))',join(state,'game.db'),sql])).stdout);
const accounts=await query('SELECT a.guid,m.rank FROM accounts a JOIN guild_members m ON m.account=a.id ORDER BY m.rank DESC');
assert.deepEqual(accounts.map(a=>a[1]),[40,0]);
const founder={guid:accounts[0][0],password:''},member={guid:accounts[1][0],password:''};
const saved=(await query('SELECT board FROM guilds'))[0][0];
const listener=createServer();await new Promise(ok=>listener.listen(0,'127.0.0.1',ok));const port=listener.address().port;await new Promise(ok=>listener.close(ok));
const origin='http://127.0.0.1:'+port,log=await open(join(out,'api-server.log'),'a',0o600);
const child=spawn(join(state,'server-build/openrotmg'),['--state',state,'--dev-control','--no-shore-bots','--client',client,'--port',String(port)],{stdio:['ignore',log.fd,log.fd]});
const closed=new Promise((ok,bad)=>{child.once('error',bad);child.once('close',(code,signal)=>ok({code,signal}));});
const checks=[],check=(name,value)=>{assert(value,name);checks.push(name);};
try{
  const deadline=Date.now()+30000;
  while(true){
    assert(child.exitCode===null,'API server exited during startup');
    try{if((await fetch(origin+'/',{signal:AbortSignal.timeout(1000)})).ok)break;}catch(error){assert(Date.now()<deadline,'API server did not start: '+error);}
    assert(Date.now()<deadline,'API server did not become ready');await delay(100);
  }
  const request=async(credentials,op,board)=>{
    await delay(4000);
    const r=await fetch(origin+'/legacy-api/guild/'+op,{method:'POST',body:new URLSearchParams({...credentials,...(board===undefined?{}:{board})}),signal:AbortSignal.timeout(10000)});
    return {body:await r.text(),type:r.headers.get('content-type'),cache:r.headers.get('cache-control')};
  };
  check('cold server reopen retains native-saved shared text',(await request(member,'getBoard')).body===saved);
  check('lower rank write is refused atomically',(await request(member,'setBoard','forged')).body.includes('Only Officers')&&(await request(founder,'getBoard')).body===saved);
  const outsider={guid:'board-outsider-'+randomUUID(),password:''};
  const enrollment=await fetch(origin+'/legacy-api/char/list',{method:'POST',body:new URLSearchParams(outsider)});
  assert((await enrollment.text()).includes('<Chars'),'Ordinary outsider guest enrollment failed');
  check('authenticated nonmember cannot read or write',(await request(outsider,'getBoard')).body.includes('not in a guild')&&(await request(outsider,'setBoard','forged')).body.includes('not in a guild'));
  check('wrong credentials cannot read or write',(await request({...founder,password:'invalid'},'getBoard')).body.includes('credentials not valid')&&(await request({...founder,password:'invalid'},'setBoard','forged')).body.includes('credentials not valid'));
  assert.equal((await query('UPDATE guild_members SET rank=20 WHERE rank=0 RETURNING account')).length,1);
  check('prepared Officer rank can save and return shared text',(await request(member,'setBoard','Officer update')).body==='Officer update'&&(await request(founder,'getBoard')).body==='Officer update');
  assert.equal((await query('UPDATE guild_members SET rank=0 WHERE rank=20 RETURNING account')).length,1);
  const literal='  e\u0301 + %2B & <tag>\r\n日本語\t🧙  ',reply=await request(founder,'setBoard',literal);
  check('verbatim Unicode whitespace CRLF and form characters with plain no-store response',reply.body===literal&&reply.type==='text/plain; charset=utf-8'&&reply.cache==='no-store'&&(await request(member,'getBoard')).body===literal);
  for(const [label,board] of [['ASCII','x'.repeat(4096)],['surrogate pairs','🧙'.repeat(2048)]])check('4096 UTF16-unit boundary '+label,(await request(founder,'setBoard',board)).body===board);
  for(const [label,board] of [['ASCII','x'.repeat(4097)],['surrogate pairs','🧙'.repeat(2048)+'x'],['control character','bad\u0000control']])check('atomic refusal '+label,(await request(founder,'setBoard',board)).body.includes('<Error>')&&(await request(member,'getBoard')).body==='🧙'.repeat(2048));
  const stale=(await request(founder,'getBoard')).body;
  await request(founder,'setBoard','first save');await request(founder,'setBoard','stale editor replaces '+stale.slice(0,2));
  check('unconditional later save replaces earlier save',(await request(member,'getBoard')).body==='stale editor replaces 🧙');
  check('ordinary empty board save is supported',(await request(founder,'setBoard','')).body===''&&(await request(member,'getBoard')).body==='');
  check('native-saved board restored exactly',(await request(founder,'setBoard',saved)).body===saved);
  await writeFile(join(out,'board-api.json'),JSON.stringify({status:'passed',native,revision:receipt.source.head,checks},null,2));
}finally{
  child.kill('SIGINT');const timer=setTimeout(()=>child.kill('SIGKILL'),10000);
  try{await closed;}finally{clearTimeout(timer);await log.close();}
}
