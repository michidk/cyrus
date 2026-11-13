import assert from 'node:assert/strict';
import {mkdtempSync,readFileSync,rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {test} from 'node:test';
import {writeJsonArray} from './write-json-array.mjs';

test('trace export avoids aggregate encoding and preserves events across chunks',()=>{
  const root=mkdtempSync(join(tmpdir(),'trace-export-'));
  try {
    const records=Array.from({length:4096},(_,i)=>({ts:i,args:{message:'é'.repeat(300)}}));
    records.toJSON=()=>{throw new RangeError('Invalid string length');};
    assert.throws(()=>JSON.stringify(records),RangeError);
    const file=join(root,'trace.json');writeJsonArray(file,records);
    assert.deepEqual(JSON.parse(readFileSync(file,'utf8')),Array.from(records));
    writeJsonArray(file,[]);assert.deepEqual(JSON.parse(readFileSync(file,'utf8')),[]);
  } finally {rmSync(root,{recursive:true,force:true});}
});
