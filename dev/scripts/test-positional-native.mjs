// Independent non-preview Pulse monitor acceptance. The fixture advances nominal updates explicitly.
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {setTimeout as delay} from 'node:timers/promises';
import {writeFileSync,mkdirSync} from 'node:fs';
import {checkPcm} from './test-positional-pcm.mjs';
const [container,output]=process.argv.slice(2);
assert(container && output,'usage: node dev/scripts/test-positional-native.mjs CONTAINER OUTPUT_DIR');
mkdirSync(output,{recursive:true});
const results=[],deadline=performance.now()+55000;
while(results.length<13) {
  assert(performance.now()<deadline,'bounded positional acceptance expired');
  const logs=spawnSync('docker',['logs',container],{encoding:'utf8',timeout:5000});
  assert.equal(logs.status,0,logs.stderr);
  const stages=logs.stdout.split('\n').filter(x=>x.startsWith('{')).map(JSON.parse).filter(x=>x.positional_emitters);
  if(stages.length<=results.length){
    const state=spawnSync('docker',['inspect','--format','{{.State.Running}}',container],{encoding:'utf8',timeout:5000});
    assert.equal(state.stdout.trim(),'true',`positional example exited before all stages: ${logs.stdout} ${logs.stderr}`);
    await delay(50);continue;
  }
  const stage=stages[results.length];
  assert.equal(stage.phase,results.length); assert.equal(stages.at(-1).phase,stage.phase,'observer missed nominal stage');
  await delay(250);
  const capture=spawnSync('docker',['exec','-e','PULSE_SERVER=unix:/run/pulse/native',container,'timeout','1','parecord','--raw','--format=s16le','--rate=22050','--channels=2','--latency-msec=50','--device=cyrus.monitor'],{timeout:5000});
  assert([0,124].includes(capture.status),String(capture.stderr));
  const left=[],right=[];
  for(let i=0;i+3<capture.stdout.length;i+=4){left.push(capture.stdout.readInt16LE(i)/32768);right.push(capture.stdout.readInt16LE(i+2)/32768);}
  writeFileSync(`${output}/stage-${stage.phase}.pcm`,capture.stdout);
  writeFileSync(`${output}/stage-${stage.phase}.json`,JSON.stringify(stage));
  // Keep the complete capture, then observe its final stable window after mixed buffers drain.
  results.push({...stage,pcm:checkPcm(stage,left.slice(-8192),right.slice(-8192))});
  writeFileSync(`${output}/result.json`,JSON.stringify({results},null,2));
}
console.log(JSON.stringify({results},null,2));
