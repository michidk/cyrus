// Independent preservation sound.md oracle for mono installed PCM output.
import assert from 'node:assert/strict';
export function expected(stage) {
  if (stage.stop) return [0,0];
  const delta=stage.position.map((v,i)=>v-stage.listener[i]);
  const distance=Math.floor(Math.round(Math.hypot(...delta))/256);
  const radius=stage.sound_distance*36, divisor=Math.floor(65536/Math.floor(radius/128));
  const volume=distance>radius?0:Math.max(0,127-Math.min(127,Math.floor(distance*divisor/65536)));
  const master=Math.max(0,Math.min(127,Math.floor(volume*stage.master/256)));
  let [x,z]=[Math.abs(delta[0]),Math.abs(delta[2])];
  while(x>511||z>511){x=Math.floor(x/2);z=Math.floor(z/2);}
  let angle;
  if(!x)angle=delta[2]>=0?0:1024;
  else if(!z)angle=delta[0]>=0?512:1536;
  else {
    const t=Math.floor(Math.trunc(Math.atan(Math.floor(Math.min(x,z)*512/Math.max(x,z))/512)*65536/Math.PI)/64);
    const base=x===z?256:x<z?t:512-t;
    angle=delta[0]<0?(delta[2]<0?base+1024:-base):(delta[2]<0?1024-base:base);
  }
  const u=((angle-stage.yaw)%2048+2048)%2048;
  const offset=u<512?u>>3:u<1024?((1024-u)&511)>>3:u<1536?-((u-1024)>>3):-(((2048-u)&511)>>3);
  const pan=64+offset, table=p=>p<63?2*p:128;
  return [table(127-pan),table(pan)].map(side=>stage.bits===8?(master?master+1:0)*side/16384:Math.floor(side*master/127)/128);
}
export function checkPcm(stage,left,right) {
  assert(left.length>=8192 && left.length===right.length,'insufficient stereo PCM');
  const rms=values=>Math.sqrt(values.reduce((s,x)=>s+x*x,0)/values.length);
  return checkLevels(stage,[rms(left),rms(right)],left.length);
}
export function checkLevels(stage,actual,frames) {
  assert(Number.isInteger(frames) && frames>=8192,'insufficient stereo PCM');
  assert.equal(actual.length,2);
  assert(actual.every(x=>Number.isFinite(x)&&x>=0),'invalid stereo RMS');
  const gain=expected(stage);
  if(gain.every(x=>x===0))assert(actual.every(x=>x<1e-7),`stale positional output phase ${stage.phase}: ${actual}`);
  else {
    actual.forEach((value,i)=>assert(Math.abs(value/stage.source_rms-gain[i])<Math.max(.008,gain[i]*.25),`positional gain phase ${stage.phase} channel ${i}: actual RMS ${value}, expected gain ${gain[i]}`));
    assert(Math.abs(actual[0]/actual[1]-gain[0]/gain[1])<.015,'positional stereo pan mismatch');
  }
  return {frames,rms:actual,expectedGain:gain};
}
