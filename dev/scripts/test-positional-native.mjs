// Independent non-preview Pulse monitor acceptance. The fixture advances nominal updates explicitly.
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {setTimeout as delay} from 'node:timers/promises';
import {writeFileSync,mkdirSync,readFileSync,existsSync} from 'node:fs';
import {createHash} from 'node:crypto';
import {checkPcm,checkLevels} from './test-positional-pcm.mjs';
const [container,output,count="13",binary]=process.argv.slice(2);
assert(container && output,'usage: node dev/scripts/test-positional-native.mjs CONTAINER OUTPUT_DIR');
const countStages=Number(count);
assert(Number.isInteger(countStages)&&countStages>0&&countStages<=13,"invalid stage count");
mkdirSync(output,{recursive:true});
const executable=binary?createHash('sha256').update(readFileSync(binary)).digest('hex'):null;
const checkpoint=`${output}/result.json`;
const saved=executable&&existsSync(checkpoint)?JSON.parse(readFileSync(checkpoint,'utf8')):null;
if(saved){
  assert.equal(saved.executable,executable,'checkpoint executable provenance differs');
  assert(saved.results.length<=countStages,'invalid checkpoint length');
  for(const [phase,stage] of saved.results.entries()){
    assert.equal(stage.phase,phase);
    assert.deepEqual(stage.pcm,checkLevels(stage,stage.pcm.rms,stage.pcm.frames));
  }
}
const results=saved?.results??[],deadline=performance.now()+55000;
let phase=0;
while(phase<countStages) {
  assert(performance.now()<deadline,'bounded positional acceptance expired');
  const logs=spawnSync('docker',['logs',container],{encoding:'utf8',timeout:5000});
  assert.equal(logs.status,0,logs.stderr);
  const stages=logs.stdout.split('\n').filter(x=>x.startsWith('{')).map(JSON.parse).filter(x=>x.positional_emitters);
  if(stages.length<=phase){
    const state=spawnSync('docker',['inspect','--format','{{.State.Running}}',container],{encoding:'utf8',timeout:5000});
    assert.equal(state.stdout.trim(),'true',`positional example exited before all stages: ${logs.stdout} ${logs.stderr}`);
    await delay(50);continue;
  }
  const stage=stages[phase];
  assert.equal(stage.phase,phase); assert.equal(stages.at(-1).phase,stage.phase,'observer missed nominal stage');
  if(phase<results.length){
    const {pcm,...previous}=results[phase];
    assert.deepEqual(stage,previous,'replayed native fixture differs from checkpoint');
    phase++;continue;
  }
  await delay(250);
  const capture=spawnSync('docker',['exec','-e','PULSE_SERVER=unix:/run/pulse/native',container,'timeout','1','parecord','--raw','--format=s16le','--rate=22050','--channels=2','--latency-msec=50','--device=cyrus.monitor'],{timeout:5000});
  assert([0,124].includes(capture.status),String(capture.stderr));
  const left=[],right=[];
  for(let i=0;i+3<capture.stdout.length;i+=4){left.push(capture.stdout.readInt16LE(i)/32768);right.push(capture.stdout.readInt16LE(i+2)/32768);}
  writeFileSync(`${output}/stage-${stage.phase}.pcm`,capture.stdout);
  writeFileSync(`${output}/stage-${stage.phase}.json`,JSON.stringify(stage));
  // Keep the complete capture, then observe its final stable window after mixed buffers drain.
  results.push({...stage,pcm:checkPcm(stage,left.slice(-8192),right.slice(-8192))});
  writeFileSync(`${output}/result.json`,JSON.stringify({results,executable},null,2));
  phase++;
}
console.log(JSON.stringify({results,executable},null,2));
