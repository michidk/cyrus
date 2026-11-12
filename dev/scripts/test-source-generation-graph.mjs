import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import vm from 'node:vm';
const script=readFileSync(new URL('./source-generation-graph.js',import.meta.url),'utf8');
function processor(){
 let Type;
 vm.runInNewContext(script.match(/registerProcessor\('source-generation-tap',[\s\S]*?\n    \}\);/)[0],{
  AudioWorkletProcessor:class{constructor(){this.messages=[];this.port={postMessage:data=>this.messages.push(data)};}},
  registerProcessor:(_,type)=>Type=type,console:{timeStamp(){}},Float32Array,currentFrame:128,
 });
 return new Type({processorOptions:{id:7}});
}
test('source branch carries its actual input and exact shared generation identity',()=>{
 const p=processor(),left=Float32Array.from({length:128},(_,i)=>i/256),right=new Float32Array(128);
 const output=[[new Float32Array(128),new Float32Array(128)]];
 p.process([[left,right],[Float32Array.from({length:128},(_,i)=>128+i),new Float32Array(128)]],output);
 assert.equal(p.messages[0].frame,128);assert.equal(p.messages[0].error,false);
 assert.deepEqual(p.messages[0].left,left);
 assert(output[0].every(channel=>channel.every(value=>value===0)),'diagnostic branch must not duplicate production audio');
});
test('unarmed clock does not invent source admission; malformed lane rejects it',()=>{
 const p=processor();p.process([[],[new Float32Array(128),new Float32Array(128)]]);
 assert.equal(p.messages.length,0);
 const clock=Float32Array.from({length:128},(_,i)=>128+i);clock[20]++;
 p.process([[],[clock,new Float32Array(128)]]);
 assert.equal(p.messages[0].error,true);
});
test('observer setup failure preserves activation and native exception',()=>{
 let starts=0;
 class AudioNode{connect(){}}
 class AudioBufferSourceNode extends AudioNode{
  constructor(){super();this.context={audioWorklet:{addModule(){throw Error('module failure');}}};this.buffer={numberOfChannels:2};}
  start(){starts++;throw Error('native failure');}
 }
 const scope={AudioBufferSourceNode,AudioNode,Blob,URL:{createObjectURL:()=>''}};
 vm.runInNewContext(script,scope);
 assert.throws(()=>new AudioBufferSourceNode().start(),/native failure/);
 assert.equal(starts,1);assert.match(scope.__sourceGeneration.error,/module failure/);
});

test('source processor acknowledges the fixed endpoint without exporting a later quantum',()=>{
 const p=processor();p.port.onmessage({data:{endpoint:256}});
 const clock=Float32Array.from({length:128},(_,i)=>256+i);
 assert.equal(p.process([[],[clock,new Float32Array(128)]]),false);
 assert.equal(p.messages[0].endpointAck,true);assert.equal(p.messages[0].frame,256);
 assert.equal(p.messages[0].left,undefined);
});

test('freeze drains delayed port delivery before cloning source PCM',async()=>{
 let tap;
 class AudioNode{connect(){}disconnect(){}}
 const context={audioWorklet:{addModule:async()=>{}}};
 class AudioBufferSourceNode extends AudioNode{
  constructor(){super();this.context=context;this.buffer={numberOfChannels:2,sampleRate:48000,length:256};}
  start(){}addEventListener(){}
 }
 class AudioWorkletNode extends AudioNode{constructor(){super();tap=this;this.port={postMessage:data=>{this.request=data;}};}}
 const scope={AudioNode,AudioBufferSourceNode,AudioWorkletNode,Blob,URL:{createObjectURL:()=>''},
  console:{timeStamp(){}},structuredClone,setTimeout,clearTimeout,
  __sessionSamples:{status:()=>({ready:true}),connectClock(){},retirements:{0:128}}};
 vm.runInNewContext(script,scope);
 new AudioBufferSourceNode().start();await new Promise(resolve=>setImmediate(resolve));
 new AudioBufferSourceNode().start();
 const send=(frame,sequence)=>tap.port.onmessage({data:{frame,sequence,zero:false,
  left:Float32Array.of(frame),right:Float32Array.of(frame)}});
 send(128,0);
 let done=false;const pending=scope.__sourceGeneration.freeze(384).then(()=>{done=true;});
 await Promise.resolve();assert.equal(done,false);assert.equal(tap.request.endpoint,384);
 send(256,1);tap.port.onmessage({data:{endpointAck:true,frame:384,error:false}});
 await pending;
 assert.deepEqual(Array.from(scope.__sourceGeneration.captured.sources[1].blocks,b=>b.frame),[128,256]);
 send(384,2);
 assert.equal(scope.__sourceGeneration.captured.sources[1].blocks.length,2,'later messages cannot extend the frozen interval');
});

test('retirement closes only after an exact rendering-thread acknowledgement and drains before freeze',async()=>{
 let tap,ended,acknowledge;
 class AudioNode{connect(){}disconnect(){this.disconnected=true;}}
 const context={currentTime:1,audioWorklet:{addModule:async()=>{}}};
 class AudioBufferSourceNode extends AudioNode{
  constructor(){super();this.context=context;this.buffer={numberOfChannels:2,sampleRate:48000,length:256};}
  start(){}stop(){}addEventListener(_,callback){ended=callback;}
 }
 class AudioWorkletNode extends AudioNode{constructor(){super();tap=this;this.port={postMessage:data=>{this.request=data;}};}}
 const scope={AudioNode,AudioBufferSourceNode,AudioWorkletNode,Blob,URL:{createObjectURL:()=>''},
  console:{timeStamp(){}},structuredClone,setTimeout,clearTimeout,__cinematicRetireTaps:true,
  __sessionSamples:{status:()=>({ready:true}),connectClock(){},retirements:{},
   retireSource:()=>new Promise(resolve=>{acknowledge=resolve;})}};
 vm.runInNewContext(script,scope);await scope.__sourceGeneration.arm(context);
 new AudioBufferSourceNode().start();ended();
 assert.equal(tap.request,undefined,'an ended callback is not a generation boundary');
 scope.__sessionSamples.retirements[0]=128;acknowledge(128);await new Promise(resolve=>setImmediate(resolve));
 assert.equal(tap.request.endpoint,256);
 tap.port.onmessage({data:{frame:128,sequence:0,zero:true}});
 tap.port.onmessage({data:{endpointAck:true,frame:256,error:false}});
 assert.equal(tap.disconnected,true);
 await scope.__sourceGeneration.freeze(384);
 assert.equal(tap.request.endpoint,256,'freeze must not reactivate a retired diagnostic branch');
 assert.equal(scope.__sourceGeneration.captured.sources[0].retiredFrame,128);
 assert.equal(scope.__sourceGeneration.captured.sources[0].blocks.length,1);
 new AudioBufferSourceNode().start();ended();
 const frozen=scope.__sourceGeneration.freeze(384);
 scope.__sessionSamples.retirements[1]=512;acknowledge(512);
 await new Promise(resolve=>setImmediate(resolve));
 assert.equal(tap.request.endpoint,384,'late retirement must not extend a frozen endpoint');
 tap.port.onmessage({data:{frame:256,sequence:0,zero:true}});
 tap.port.onmessage({data:{endpointAck:true,frame:384,error:false}});
 await frozen;
 assert.equal(scope.__sourceGeneration.error,null);
});

test('stop records the native source identity and preserves arguments and exceptions',()=>{
 const stamps=[],calls=[];
 class AudioNode{connect(){}}
 const context={currentTime:2,audioWorklet:{addModule:async()=>{}}};
 class AudioBufferSourceNode extends AudioNode{
  constructor(){super();this.context=context;this.buffer={numberOfChannels:2,sampleRate:22050,length:128};}
  start(){}addEventListener(){}
  stop(...args){calls.push(args);if(args[0]===-1)throw Error('native stop failure');return 7;}
 }
 const scope={AudioNode,AudioBufferSourceNode,Blob,URL:{createObjectURL:()=>''},
  console:{timeStamp:name=>stamps.push(name)},__sessionSamples:{status:()=>({ready:false})}};
 vm.runInNewContext(script,scope);
 const source=new AudioBufferSourceNode();source.start();
 assert.equal(scope.__sourceGeneration.idFor(source),0);
 assert.equal(source.stop(3),7);assert.deepEqual(calls,[[3]]);
 assert.equal(scope.__sourceGeneration.sources[0].stopRequested,2);
 assert(stamps.includes('source-stop:0'));
 assert.throws(()=>source.stop(-1),/native stop failure/);
 assert.deepEqual(calls,[[3],[-1]]);
});

test('render endpoint acknowledges once even if Chromium pulls a retired worklet again',()=>{
 const p=processor();p.port.onmessage({data:{endpoint:128}});
 const clock=frame=>[Float32Array.from({length:128},(_,i)=>frame+i),new Float32Array(128)];
 p.process([[],clock(128)]);p.process([[],clock(256)]);
 assert.equal(p.messages.length,1);assert.equal(p.messages[0].frame,128);
});
