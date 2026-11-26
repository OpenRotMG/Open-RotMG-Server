import assert from 'node:assert/strict';

export async function nativeMap(origin,headers,player){
  const response=await fetch(origin+'/dev/map?player='+player,{headers});
  assert(response.ok);
  return response.json();
}
