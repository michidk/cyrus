import test from 'node:test';
import assert from 'node:assert/strict';
import {installCinematicSeekGate} from './cinematic-seek-gate.mjs';
async function fixture(run) {
 let resume=0,retire=0,release;
 const node={},source={requested:1,duration:1,blocks:[{connected:true}]};
 const audio={sources:[node],context:{currentTime:1.5,state:'running',
  suspend:()=>new Promise(resolve=>{release=()=>{audio.context.state='suspended';resolve();};}),
  resume:async()=>{resume++;audio.context.state='running';}}};
 globalThis.__cyrusMenu={movie:{frame:10}};
 globalThis.__sourceGeneration={sources:[source],idFor:()=>0,retirement:async ids=>{retire++;return ids.map(id=>({id,frame:128,end:256}));}};
 try {installCinematicSeekGate(10);await run({gate:globalThis.__cinematicSeekGate,audio,release:()=>release(),counts:()=>({resume,retire})});}
 finally {for(const key of ['__cyrusMenu','__sourceGeneration','__cinematicSeekGate','__cinematicClockError','__cinematicPhaseHeld'])delete globalThis[key];}
}
const settle=()=>new Promise(resolve=>setImmediate(resolve));
test('public input waits for an acknowledged clock hold with actual remaining owners',()=>fixture(async({gate,audio,release,counts})=>{
 gate.consider(audio);assert.equal(gate.state,'holding');release();await settle();
 assert.equal(gate.state,'ready');assert.deepEqual(gate.trigger.active,[{id:0,end:2,processed:true}]);
 gate.afterSeek({active:[0]});assert.equal(globalThis.__cinematicPhaseHeld,true);await settle();
 assert.equal(gate.state,'retired');assert.deepEqual(counts(),{resume:1,retire:1});
}));
test('the old delayed-input ownership loss is rejected before public input',()=>fixture(async({gate,audio,release})=>{
 gate.consider(audio);audio.sources=[];release();await settle();
 assert.equal(gate.state,'failed');assert.match(globalThis.__cinematicClockError,/expired/);
}));
test('a bank that expires during input transport cannot be called cancellation acceptance',()=>fixture(async({gate,audio,release})=>{
 gate.consider(audio);release();await settle();
 assert.throws(()=>gate.afterSeek({active:[]}),/ownership lost/);
}));
test('empty and already consumed banks do not arm the public trigger',()=>fixture(async({gate,audio})=>{
 audio.context.currentTime=2;gate.consider(audio);assert.equal(gate.state,'waiting');
 audio.sources=[];gate.consider(audio);assert.equal(gate.state,'waiting');
}));

test('trusted-input audio unlock cannot consume the held owners before seek dispatch',()=>fixture(async({gate,audio,release,counts})=>{
 gate.consider(audio);release();await settle();
 let unlocked=false;const unlock=audio.context.resume().then(()=>{unlocked=true;});
 await settle();assert.equal(unlocked,false);assert.equal(audio.context.state,'suspended');
 assert.equal(counts().resume,0);
 gate.afterSeek({active:[0]});await unlock;await settle();
 assert.equal(unlocked,true);assert.equal(gate.state,'retired');
 assert.equal(counts().resume,2);
}));

test('queued source must join its actual rendering input before holding public dispatch',()=>fixture(async({gate,audio,release})=>{
 const source=globalThis.__sourceGeneration.sources[0];source.blocks=[];
 gate.consider(audio);assert.equal(gate.state,'waiting');
 source.blocks.push({connected:true});gate.generation();assert.equal(gate.state,'holding');
 release();await settle();assert.equal(gate.state,'ready');
}));
