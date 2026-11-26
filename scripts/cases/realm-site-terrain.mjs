import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {inflateSync} from 'node:zlib';

// Linked placement logs carry coordinates, not terrain. The two stamps keep the terrain plane
// (src/world/realm_setpieces.rs); decode the packaged map as src/content.rs::Map does.
export async function realmSiteTerrain(layout){
  const directory=/^classic-world-\d{2}-reconstruction$/.test(layout)?'realms/':'';
  assert(directory||/^realm-fsod-[1-3]$/.test(layout),`Unexpected layout ${layout}`);
  const map=inflateSync(await readFile(new URL(`../../data/${directory}${layout}.wmap`,import.meta.url)),{maxOutputLength:16*1024*1024});
  const count=map.readInt16LE(0),terrain=[];let offset=2;
  const skipString=()=>{
    let size=0;
    for(let shift=0;shift<35;shift+=7){
      const byte=map.readUInt8(offset++);size+=(byte&127)*2**shift;
      if(byte<128){offset+=size;return;}
    }
    assert.fail('Invalid map string length');
  };
  for(let n=0;n<count;n++){
    offset+=2;skipString();skipString();terrain.push(map.readUInt8(offset));offset+=2;
  }
  const width=map.readInt32LE(offset),height=map.readInt32LE(offset+4);offset+=8;
  assert(width>0&&height>0&&map.length===offset+width*height*2);
  return (x,y)=>{
    x=Math.floor(x);y=Math.floor(y);assert(x>=0&&x<width&&y>=0&&y<height);
    const index=map.readInt16LE(offset+2*(width*y+x));assert(index>=0&&index<count);
    return terrain[index];
  };
}
