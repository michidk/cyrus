import test from 'node:test';
import assert from 'node:assert/strict';
import {sourceGenerationBoundaries} from './source-generation-boundaries.mjs';
const handler=(type,id,ts,dur)=>({name:'AudioHandler::ProcessIfNecessary',pid:1,tid:2,ts,dur,args:{'node type':type,this:id}});
const stamp=(id,frame,ts)=>({name:'TimeStamp',pid:1,tid:2,ts,args:{data:{message:`source-generation:${id}:${frame}`}}});
const process=ts=>({name:'AudioBufferSourceHandler::Process',pid:1,tid:2,ts,dur:1});
const source=id=>({id,observed:true,blocks:[{frame:128,sequence:0,left:new Float32Array(128),right:new Float32Array(128)}]});
const trace=[{name:'TimeStamp',pid:1,tid:2,ts:30,args:{data:{message:'session-generation:128'}}},{name:'TimeStamp',pid:1,tid:1,ts:1,args:{data:{message:'source-command:0'}}},{name:'TimeStamp',pid:1,tid:1,ts:2,args:{data:{message:'source-command:1'}}},{name:'RealtimeAudioDestinationHandler::Render',pid:1,tid:2,ts:9,dur:55},handler('AudioWorkletNode','tap0',10,30),handler('AudioBufferSourceNode','source0',11,3),process(12),
 handler('AudioWorkletNode','clock',15,15),handler('AudioBufferSourceNode','source1',16,3),process(17),stamp(0,128,35),
 handler('AudioWorkletNode','tap1',41,20),handler('AudioBufferSourceNode','source1',42,3),stamp(1,128,55)];
test('concurrent sources are identified through their own direct inputs',()=>{
 const result=sourceGenerationBoundaries(trace,[source(0),source(1)]);
 assert.equal(result[0].firstProcessingTraceTime,12);
 assert.equal(result[1].firstProcessingTraceTime,17,'shared mixer processed source before its diagnostic branch');
 assert.equal(result[1].firstInputTraceTime,42);
});
test('published commands alone cannot substitute for native ownership',()=>{
 assert.throws(()=>sourceGenerationBoundaries(trace.filter(e=>e.name!=='AudioHandler::ProcessIfNecessary'),[source(0)]),/native tap owner/);
});
test('missing marker and ambiguous direct input reject evidence',()=>{
 assert.throws(()=>sourceGenerationBoundaries(trace.filter(e=>e.name!=='TimeStamp'),[source(0)]),/generation marker/);
 assert.throws(()=>sourceGenerationBoundaries([...trace,handler('AudioBufferSourceNode','extra',31,2)],[source(0)]),/ambiguous/);
});
test('missing source quantum cannot be replaced by zeros',()=>{
 const s=source(0);s.blocks.push({...s.blocks[0],frame:384,sequence:2});
 assert.throws(()=>sourceGenerationBoundaries(trace,[s]),/discontinuous/);
});

 test('retired zero input remains explicitly unattributed, never assigned a clock',()=>{
  const s=source(0);s.retiredFrame=256;s.blocks[0]={frame:128,sequence:0,zero:true};
  const result=sourceGenerationBoundaries(trace.filter(e=>e.args?.['node type']!=='AudioBufferSourceNode'),[s])[0];
  assert.equal(result.attributed,false);assert.equal(result.firstProcessingFrame,undefined);
  s.retiredFrame=null;
  assert.throws(()=>sourceGenerationBoundaries(trace.filter(e=>e.args?.['node type']!=='AudioBufferSourceNode'),[s]),/source identity/);
 });

test('a later connected input establishes ownership without relabelling the earlier empty quantum',()=>{
 const s=source(0);s.blocks[0].connected=false;
 s.blocks.push({...s.blocks[0],frame:256,sequence:1,connected:true});
 const later=[...trace,{name:'RealtimeAudioDestinationHandler::Render',pid:1,tid:2,ts:79,dur:25},
  handler('AudioWorkletNode','tap0',80,20),handler('AudioBufferSourceNode','source0',81,3),stamp(0,256,95)];
 const b=sourceGenerationBoundaries(later,[s])[0];
 assert.equal(b.attributed,true);assert.equal(b.firstInputFrame,256);
 assert.equal(b.firstProcessingFrame,128);assert.equal(b.firstObservedFrame,128);
});
