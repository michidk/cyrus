import assert from 'node:assert/strict';
import {expected,checkPcm,checkLevels} from './test-positional-pcm.mjs';
const stage={phase:0,stop:false,position:[512*256,0,512*256],listener:[0,0,0],yaw:0,sound_distance:64,master:255,bits:16,source_rms:.4};
assert.deepEqual(expected(stage),[41/128,86/128]);
const left=Array(8192).fill(.4*41/128),right=Array(8192).fill(.4*86/128),zero=Array(8192).fill(0);
checkPcm(stage,left,right);
assert.throws(()=>checkPcm(stage,right,left),/gain|pan/);
assert.throws(()=>checkPcm(stage,left.map(x=>x/2),right.map(x=>x/2)),/gain/);
assert.throws(()=>checkPcm(stage,zero,zero),/gain/);
assert.throws(()=>checkPcm({...stage,stop:true},left,right),/stale/);
assert.throws(()=>checkPcm(stage,left.slice(1),right.slice(1)),/insufficient/);
checkPcm({...stage,stop:true},zero,zero);
assert.deepEqual(expected({...stage,yaw:512}),[86/128,43/128]);
console.log('positional PCM: literal diagonal/yaw gains, active/silent controls and five failure controls passed');

// Persisted levels must satisfy the same oracle before a resumed run can skip them.
const accepted=checkPcm(stage,left,right);
assert.deepEqual(checkLevels(stage,accepted.rms,accepted.frames),accepted);
assert.throws(()=>checkLevels(stage,[NaN,.1],8192),/invalid stereo RMS/);
assert.throws(()=>checkLevels(stage,accepted.rms,8191),/insufficient/);
assert.throws(()=>checkLevels(stage,[0,0],8192),/gain/);
console.log('checkpoint level validation: valid replay and three tamper controls passed');
