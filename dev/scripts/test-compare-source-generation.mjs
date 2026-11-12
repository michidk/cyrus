import test from 'node:test';import assert from 'node:assert/strict';
import {compareSourceGeneration} from './compare-source-generation.mjs';
const block=(frame,sequence,value)=>({frame,sequence,left:Array(128).fill(value),right:Array(128).fill(value)});
const recording=()=>({sources:[{observed:true,retiredFrame:null,blocks:[block(128,0,.125),block(256,1,.25)]}],
 graph:{rate:48000,blocks:[block(128,0,.125),block(256,1,.25)]}});
test('actual source outputs admit mixed PCM with no fitted shift',()=>{
 const r=compareSourceGeneration(recording());assert.equal(r.error,0);assert.equal(r.frames,256);
});
test('lost interior source/mix quanta and altered output remain rejected',()=>{
 let r=recording();r.sources[0].blocks.pop();assert.throws(()=>compareSourceGeneration(r),/missing source quantum/);
 r=recording();r.graph.blocks[1].frame=384;assert.throws(()=>compareSourceGeneration(r),/discontinuous/);
 r=recording();r.sources[0].blocks[1].left[5]+=.1;assert.throws(()=>compareSourceGeneration(r),/PCM differs/);
});
test('unobserved initial sources require actual acknowledged retirement',()=>{
 const r=recording();r.sources.push({observed:false,blocks:[],retiredFrame:null});
 assert.throws(()=>compareSourceGeneration(r),/retirement/);
 r.sources[1].retiredFrame=128;assert.equal(compareSourceGeneration(r).error,0);
});

test('a continuous silent tail cannot prove waveform equivalence',()=>{
 const r=recording();for(const b of [...r.graph.blocks,...r.sources[0].blocks]){b.left.fill(0);b.right.fill(0);}
 assert.throws(()=>compareSourceGeneration(r),/no nonzero waveform evidence/);
});

test('unattributed retired source excludes only its acknowledged prefix',()=>{
 const r=recording();
 const boundaries=[{id:1,attributed:false,retiredFrame:256}];
 const paired=compareSourceGeneration(r,boundaries);
 assert.equal(paired.start,256);assert.equal(paired.end,384);assert.equal(paired.energy,16);
 assert.throws(()=>compareSourceGeneration(r,[{id:1,attributed:false}]),/unattributed source/);
 assert.throws(()=>compareSourceGeneration(r,[{id:1,attributed:false,retiredFrame:384}]),/no complete paired/);
});
