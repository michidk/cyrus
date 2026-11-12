import test from 'node:test';
import assert from 'node:assert/strict';
import {navigateCinematic} from './cinematic-navigation.mjs';
test('a CDP success containing a network error waits for the recovered server',async()=>{
 let calls=0,waits=0;
 const result=await navigateCinematic({call:async()=>++calls<3?{errorText:'net::ERR_CONNECTION_REFUSED'}:{loaderId:'ready'}},
 'http://localhost/movie',async()=>{waits++;});
 assert.equal(result.loaderId,'ready');assert.equal(calls,3);assert.equal(waits,2);
});
test('permanent navigation failure retains the actual network error',async()=>{
 await assert.rejects(()=>navigateCinematic({call:async()=>({errorText:'net::ERR_EMPTY_RESPONSE'})},'http://localhost/movie',async()=>{},2),/ERR_EMPTY_RESPONSE/);
});

test('endpoint freeze stops later animation without inventing a presented frame',async()=>{
 const {installCinematicEndpoint}=await import('./cinematic-navigation.mjs');
 const previous=globalThis.requestAnimationFrame;let pending,calls=0;
 globalThis.requestAnimationFrame=callback=>{pending=callback;return 7;};
 try{
  installCinematicEndpoint();
  assert.equal(requestAnimationFrame(()=>calls++),7);pending(1);assert.equal(calls,1);
  requestAnimationFrame(()=>calls++);globalThis.__cinematicEndpointStopped=true;pending(2);
  assert.equal(calls,1);
 }finally{globalThis.requestAnimationFrame=previous;delete globalThis.__cinematicEndpointStopped;}
});

test('returned movie clock observation calls playback once and preserves native failures',async()=>{
 const {observeMovieTime}=await import('./cinematic-navigation.mjs');
 let reads=0;const records=[],nativeError=Error('native failure');
 const original=id=>{reads++;if(id===2)throw nativeError;return .25;};
 const observed=observeMovieTime(original,()=>({videoEnd:.5}),records);
 assert.equal(observed(1),.25);assert.equal(reads,1);assert.equal(records[0].value,.25);
 assert.throws(()=>observed(2),error=>error===nativeError);assert.equal(reads,2);assert.equal(records.length,1);
 const broken=observeMovieTime(original,()=>{throw Error('observer failure');},[]);
 assert.equal(broken(1),.25);assert.match(globalThis.__cinematicClockError,/observer failure/);
 delete globalThis.__cinematicClockError;
});

test('progress observation waits for waveform evidence without polling the movie clock',async()=>{
 const {cinematicProgress}=await import('./cinematic-navigation.mjs');
 const originalDocument=globalThis.document;
 globalThis.document={getElementById:()=>null};
 globalThis.__cyrusMenu={movie:{frame:31,loading:false}};
 globalThis.__sessionSamples={blocks:[{left:new Float32Array(128),right:new Float32Array(128)}]};
 try{
  assert.equal(cinematicProgress(30).signalEnergy,0);
  __sessionSamples.blocks[0].left.fill(.125);
  assert.equal(cinematicProgress(30).signalEnergy,2);
  assert.equal(cinematicProgress(32).signalEnergy,0);
 }finally{
  globalThis.document=originalDocument;delete globalThis.__cyrusMenu;delete globalThis.__sessionSamples;
 }
});

test('scheduled packet positions remain separate from actual native processing',async()=>{
 const {observeMovieSchedule}=await import('./cinematic-navigation.mjs');
 const probe=globalThis.__sourceGeneration={sources:[]};const receiver={};
 const original=function(block){assert.equal(this,receiver);probe.sources.push({offset:.125});return 7;};
 try {
  assert.equal(observeMovieSchedule(original).call(receiver,{start:2,end:3}),7);
  assert.equal(probe.sources[0].movieStart,2.125);assert.equal(probe.sources[0].movieEnd,3);
  assert.equal(probe.sources[0].firstProcessingFrame,undefined);
  const error=Error('native failure');assert.throws(()=>observeMovieSchedule(()=>{throw error;})({}),e=>e===error);
 } finally {delete globalThis.__sourceGeneration;}
});

test('a clamped movie clock cannot admit audio consumed beyond decoded video',async()=>{
 const {requireMovieHorizon}=await import('./cinematic-navigation.mjs');
 assert.throws(()=>requireMovieHorizon([{value:.26664,videoEnd:.26664,packetPosition:.38022675736961453}],44100),/beyond decoded video/);
 requireMovieHorizon([{value:.26664,videoEnd:.26664,packetPosition:.26664}],44100);
});

 test('a failed schedule observer cannot suppress native source scheduling',async()=>{
  const {observeMovieSchedule}=await import('./cinematic-navigation.mjs');
  globalThis.__sourceGeneration={get sources(){throw Error('observer failed');}};
  let calls=0;
  try {
   assert.equal(observeMovieSchedule(()=>{calls++;return 7;})({}),7);
   assert.equal(calls,1);assert.match(globalThis.__cinematicClockError,/observer failed/);
  } finally {delete globalThis.__sourceGeneration;delete globalThis.__cinematicClockError;}
 });

test('production playback starts only after an independently armed generation quantum',async()=>{
 const {armCinematicPlayback}=await import('./cinematic-navigation.mjs');
 const order=[],p=globalThis.__sessionSamples={blocks:[],arm:async()=>order.push('clock')};
 globalThis.__sourceGeneration={arm:async()=>order.push('source')};
 const receiver={context:{resume:async()=>{order.push('resume');p.blocks.push({frame:128});}}};
 try {
  assert.equal(await armCinematicPlayback(function(){assert.equal(this,receiver);order.push('play');return 7;}).call(receiver),7);
  assert.deepEqual(order,['clock','source','resume','play']);
 } finally {delete globalThis.__sessionSamples;delete globalThis.__sourceGeneration;}
});

test('arming failure cannot suppress or retry production playback',async()=>{
 const {armCinematicPlayback}=await import('./cinematic-navigation.mjs');let calls=0;
 globalThis.__sessionSamples={arm(){throw Error('arm failed');}};
 globalThis.__sourceGeneration={arm:async()=>{}};
 try {
  assert.equal(await armCinematicPlayback(()=>{calls++;return 7;}).call({context:{}}),7);
  assert.equal(calls,1);assert.match(globalThis.__cinematicClockError,/arm failed/);
 } finally {delete globalThis.__sessionSamples;delete globalThis.__sourceGeneration;delete globalThis.__cinematicClockError;}
});

test('seek snapshots actual owners before native cancellation without changing dispatch',async()=>{
 const {observeMovieSeek}=await import('./cinematic-navigation.mjs');
 const source={},audio={context:{currentTime:2},sources:[source]},records=[];
 globalThis.__sourceGeneration={sources:Array(8),idFor:s=>{assert.equal(s,source);return 7;}};
 const receiver={};
 try {
  const wrapped=observeMovieSeek(function(id,target){assert.equal(this,receiver);assert.equal(id,3);assert.equal(target,100);audio.sources=[];return 9;},()=>({audio}),records);
  assert.equal(wrapped.call(receiver,3,100),9);
  assert.deepEqual(records,[{target:100,contextTime:2,endContextTime:2,sourceCount:8,active:[7]}]);
  const error=Error('native seek failure');assert.throws(()=>observeMovieSeek(()=>{throw error;},()=>({audio}),[])(3,100),e=>e===error);
 } finally {delete globalThis.__sourceGeneration;}
});

test('a cancellation phase retains the pending animation until explicit release',async()=>{
 const {installCinematicEndpoint}=await import('./cinematic-navigation.mjs');
 const native=globalThis.requestAnimationFrame,pending=[];let calls=0;
 globalThis.requestAnimationFrame=callback=>pending.push(callback);
 try {
  installCinematicEndpoint();globalThis.__cinematicPhaseHeld=true;
  requestAnimationFrame(()=>calls++);pending.shift()(10);
  assert.equal(calls,0);assert.equal(pending.length,0);
  __releaseCinematicPhase();assert.equal(pending.length,1);pending.shift()(20);
  assert.equal(calls,1);
 } finally {
  globalThis.requestAnimationFrame=native;
  delete globalThis.__cinematicPhaseHeld;delete globalThis.__releaseCinematicPhase;
 }
});
