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
