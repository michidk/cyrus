import test from 'node:test';
import assert from 'node:assert/strict';
import vm from 'node:vm';
import {webcrypto} from 'node:crypto';
import {mkdtempSync,readFileSync,rmSync,existsSync,writeFileSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {inflateSync} from 'node:zlib';
import {exportRetainedFrame,png} from './export-retained-frame.mjs';
function fixture() {
  const dir=mkdtempSync(join(tmpdir(),'retained-')), output=join(dir,'frame.png');
  const pixels=new Uint8Array([1,2,3,255,4,5,6,255]);
  const window={__cyrusFrameCaptureResult:{ok:true,id:7,boundary:{update:30},width:2,height:1,format:'Rgba8UnormSrgb',pixels}};
  const context=vm.createContext({window,Uint8Array,crypto:webcrypto,btoa:x=>Buffer.from(x,'binary').toString('base64')});
  return {dir,output,window,evaluate:async expression=>JSON.parse(JSON.stringify(await vm.runInContext(expression,context)))};
}
test('interrupted export resumes retained pixels while live presentation advances',async()=>{
  const f=fixture(); let calls=0;
  try {
    await assert.rejects(exportRetainedFrame(async e=>{if(++calls===3)throw Error('reset transport');return f.evaluate(e);},f.output,4),/reset transport/);
    assert.equal(readFileSync(f.output+'.rgba.part').length,4);
    assert(!existsSync(f.output));
    f.window.liveUpdate=100;
    const result=await exportRetainedFrame(f.evaluate,f.output,4);
    assert(result.complete); assert.equal(result.capture.boundary.update,30);
    const image=readFileSync(f.output); assert.equal(image.subarray(1,4).toString(),'PNG');
    const length=image.readUInt32BE(33); // IDAT payload follows IHDR.
    assert.deepEqual([...inflateSync(image.subarray(41,41+length))],[0,1,2,3,255,4,5,6,255]);
    assert.deepEqual(await exportRetainedFrame(f.evaluate,f.output,4),result);
  } finally {rmSync(f.dir,{recursive:true});}
});
test('replacement after interruption cannot reuse a different frame',async()=>{
  const f=fixture(); let calls=0;
  try {
    await assert.rejects(exportRetainedFrame(async e=>{if(++calls===3)throw Error('interrupted');return f.evaluate(e);},f.output,4));
    f.window.__cyrusFrameCaptureResult.pixels[0]=99;
    await assert.rejects(exportRetainedFrame(f.evaluate,f.output,4),/capture changed/);
    assert(!existsSync(f.output));
  } finally {rmSync(f.dir,{recursive:true});}
});
test('reset or truncated chunks never publish a complete PNG',async()=>{
  for(const mode of ['reset','truncated']) {
    const f=fixture(); let calls=0;
    try {
      await assert.rejects(exportRetainedFrame(async e=>{
        if(++calls===2){if(mode==='reset') f.window.__cyrusFrameCaptureResult=null;else return '';}
        return f.evaluate(e);
      },f.output,4),/replaced|truncated/);
      assert(!existsSync(f.output));
      assert.equal(JSON.parse(readFileSync(f.output+'.json')).complete,false);
    } finally {rmSync(f.dir,{recursive:true});}
  }
});
test('restart after PNG publication finishes the receipt without overwriting pixels',async()=>{
  const f=fixture();
  try {
    const result=await exportRetainedFrame(f.evaluate,f.output,4);
    writeFileSync(f.output+'.json',JSON.stringify({capture:result.capture,complete:false}));
    writeFileSync(f.output+'.rgba.part',Buffer.from(f.window.__cyrusFrameCaptureResult.pixels));
    assert.deepEqual(await exportRetainedFrame(f.evaluate,f.output,4),result);
  } finally {rmSync(f.dir,{recursive:true});}
});
test('BGRA is converted and unsupported formats refuse',()=>{
  const image=png(1,1,'Bgra8Unorm',Buffer.from([1,2,3,255]));
  assert.deepEqual([...inflateSync(image.subarray(41,41+image.readUInt32BE(33)))],[0,3,2,1,255]);
  assert.throws(()=>png(1,1,'Rgba16Float',Buffer.alloc(4)),/unsupported/);
});
