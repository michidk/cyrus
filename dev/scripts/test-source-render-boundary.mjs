import assert from 'node:assert/strict';
import test from 'node:test';
import {renderBoundaries} from './test-browser-source-render-boundary.mjs';
const call = {name:'TimeStamp',ts:10,pid:1,tid:1,args:{data:{message:'source:1'}}};
const handler = {name:'AudioHandler::ProcessIfNecessary',ts:20,dur:10,pid:1,tid:2,
  args:{'node type':'AudioBufferSourceNode',this:'source-handler'}};
const process = {name:'AudioBufferSourceHandler::Process',ts:21,dur:2,pid:1,tid:2};
const quantum = {name:'TimeStamp',ts:31,pid:1,tid:2,args:{data:{message:'quantum:22144'}}};
const trial = {id:1,requestedFrame:21760,first:22144,error:0,native:{before:21760/48000,after:21760/48000}};

test('native rendering, not an unchanged command clock, admits the 384-frame onset', () => {
  const [boundary] = renderBoundaries([call,handler,process,quantum],[trial]);
  assert.equal(boundary.firstRenderFrame,22144);
  assert.equal(boundary.commandDelayFrames,384);
  assert.throws(() => renderBoundaries([call,handler,process,quantum],[{...trial,first:21760}]), /PCM onset disagrees/);
});

test('future scheduling keeps its requested onset after early native processing', () => {
  const [boundary] = renderBoundaries([call,handler,process,quantum],[{...trial,requestedFrame:24000,first:24000}]);
  assert.equal(boundary.onset,24000);
  assert.equal(boundary.commandDelayFrames,0);
});

for (const [name, trace, observed, error] of [
  ['missing command',[handler,process,quantum],trial,/native command/],
  ['missing generation clock',[call,handler,process],trial,/generation-clock/],
  ['wrong clock thread',[call,handler,process,{...quantum,tid:3}],trial,/generation-clock/],
  ['missing native processing',[call,handler,quantum],trial,/did not process/],
  ['concurrent source',[call,handler,{...handler,args:{...handler.args,this:'other'}},process,quantum],trial,/ambiguous/],
  ['altered waveform',[call,handler,process,quantum],{...trial,error:1},/waveform differs/],
]) test(`${name} rejects source attribution`, () => assert.throws(() => renderBoundaries(trace,[observed]),error));
