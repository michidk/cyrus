import test from 'node:test';
import assert from 'node:assert/strict';
import {requireCinematicTrace, requireCinematicCancellation} from './cinematic-trace.mjs';
const stamp = name => ({name:'TimeStamp',args:{data:{message:name}}});
const names=['cinematic-playback-armed','source-generation-endpoint','session-generation:128',
  'session-generation:256','source-command:0','source-stop:0','source-retired:0:256'];
const capture={graph:{blocks:[{frame:128},{frame:256}]},sources:[{id:0,stopRequested:1,retiredFrame:256}]};
test('complete creation, cancellation and exact endpoint evidence is admitted',()=>{
 assert.equal(requireCinematicTrace(names.map(stamp),capture,{dataLossOccurred:false}).sampleQuanta,2);
});
test('lost tail, creation, cancellation and retirement evidence each reject',()=>{
 for(const missing of names) assert.throws(()=>requireCinematicTrace(names.filter(n=>n!==missing).map(stamp),capture,{dataLossOccurred:false}),/missing or duplicate/);
});
test('buffer loss, incomplete completion and duplicate clocks reject',()=>{
 for(const completion of [{dataLossOccurred:true},{},undefined])assert.throws(()=>requireCinematicTrace(names.map(stamp),capture,completion),/lost events/);
 assert.throws(()=>requireCinematicTrace([...names,'session-generation:256'].map(stamp),capture,{dataLossOccurred:false}),/duplicate/);
});

const cancellation = () => ({seeks:[{target:100,contextTime:2,endContextTime:2,sourceCount:1,active:[0]}],sources:[
 {id:0,stopRequested:2,retiredFrame:256,blocks:[{frame:128,left:[1],right:[1]}]},
 {id:1,movieStart:100,blocks:[]}]});
const adopted=[{id:1,attributed:true,firstProcessingFrame:384}];
test('queued cancellation requires active ownership, stop, retirement and clean continuation',()=>{
 assert.deepEqual(requireCinematicCancellation(cancellation(),adopted),{seeks:1,cancelled:1});
 for(const mutate of [c=>c.seeks[0].active=[],c=>delete c.sources[0].stopRequested,
  c=>delete c.sources[0].retiredFrame,c=>c.sources[0].retiredFrame=512,
  c=>c.sources[0].blocks.push({frame:384,left:[1],right:[0]})]) {
  const c=cancellation();mutate(c);assert.throws(()=>requireCinematicCancellation(c,adopted));
 }
 assert.throws(()=>requireCinematicCancellation(cancellation(),[]),/first-processing/);
});
test('seek trace requires both dispatch boundaries even when no source was active',()=>{
 const c={...capture,seeks:[{active:[]}]};
 assert.throws(()=>requireCinematicTrace(names.map(stamp),c,{dataLossOccurred:false}),/seek-begin/);
 requireCinematicTrace([...names,'cinematic-seek-begin:0','cinematic-seek-end:0'].map(stamp),c,{dataLossOccurred:false});
});

test('short cancellation phase rejects missing adoption, drain acknowledgement and stale PCM',async()=>{
 const {requireCancellationPhase}=await import('./cinematic-trace.mjs');
 const phase=()=>{
  const c=cancellation();c.sources[0].drainedFrame=384;
  c.phase={state:'retired',retirement:[{id:0,frame:256,end:384}]};return c;
 };
 const b=[{id:0,attributed:true,firstProcessingFrame:128}];
 assert.deepEqual(requireCancellationPhase(phase(),b).cancelled,[0]);
 for(const mutate of [c=>delete c.phase,c=>delete c.sources[0].drainedFrame,
  c=>c.sources[0].blocks.push({frame:256,left:[1],right:[0]})]) {
  const c=phase();mutate(c);assert.throws(()=>requireCancellationPhase(c,b));
 }
 assert.throws(()=>requireCancellationPhase(phase(),[]),/native processing/);
});
