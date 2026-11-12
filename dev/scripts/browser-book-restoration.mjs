// Installed book/speech ownership and muted session publication through public input.
import assert from 'node:assert/strict';
import {writeFileSync} from 'node:fs';
import {captureSampleGraph} from './capture-sample-graph.mjs';

export async function runBookRestoration({cdp,evaluate,until,session,key,action,outputWindow,fixture,output,result}) {
  const body=response=>JSON.parse(response.document.split('\n').slice(2).join('\n'));
  async function sample(label,present){
    const window=await evaluate(`__cyrusSoundReset.sample(${JSON.stringify(label)},${present})`);
    return captureSampleGraph(cdp,`${output}/${label}`,window.start,window.end);
  }
  async function select(action){
    for(let attempt=0;attempt<12;attempt++){
      const menu=await evaluate('__cyrusMenu');assert(menu.ready);
      if(menu.entries[menu.selected]?.action===action)return;
      await cdp.call('Input.dispatchKeyEvent',{type:'keyDown',code:'ArrowDown',key:'ArrowDown',windowsVirtualKeyCode:40});
      try{await until('book selection',`__cyrusMenu.selected!==${menu.selected}`,5);}
      finally{await cdp.call('Input.dispatchKeyEvent',{type:'keyUp',code:'ArrowDown',key:'ArrowDown',windowsVirtualKeyCode:40});}
      await until('book selection ready','__cyrusMenu.ready');
    }throw Error(`installed book action unavailable: ${action}`);
  }
  await until('ordinary world readiness','__cyrusCapture.scripts.globals.world_loop_frame>2');
  result.savedMusic=body(await session('capture')).music;
  await session('save',{key:'before-book'});
  if(!fixture.isolate_publication){
  await evaluate(`window.__cyrusAcceptanceRequest=${JSON.stringify(JSON.stringify({id:100000,action:'place_player',...fixture.placement}))};true`);
  const placement=await until('conversation placement','window.__cyrusAcceptanceResult?.id===100000&&__cyrusAcceptanceResult');
  assert(placement.applied,placement.error);
  if(!await evaluate('__cyrusCapture.scripts.globals.dialogue.records.some(r=>r.active)')){
    await cdp.call('Input.dispatchKeyEvent',{type:'keyDown',code:'ControlLeft',key:'Control',windowsVirtualKeyCode:17});
    try{const clock=await evaluate('__cyrusStartup.clock.game_seconds');await until('conversation input',`__cyrusStartup.clock.game_seconds>${clock+.25}`);}
    finally{await cdp.call('Input.dispatchKeyEvent',{type:'keyUp',code:'ControlLeft',key:'Control',windowsVirtualKeyCode:17});}
  }
  await until('authored speech','(()=>{const d=__cyrusCapture.scripts.globals.dialogue;return d.presented!==null&&d.records[d.presented].active&&d.records[d.presented].sample!==null})()');
  result.speech=await sample('before-book-speech',true);
  result.subtitle=await evaluate('structuredClone(__cyrusCapture.scripts.globals.dialogue)');
  writeFileSync(`${output}/speech.png`,Buffer.from((await cdp.call('Page.captureScreenshot',{format:'png'})).data,'base64'));
  await key('Escape',27);await until('paused book','__cyrusStartup.state==="Menu"&&__cyrusMenu.ready');
  result.bookSilence=await sample('open-book-silence',false);
  const inBook=body(await session('capture'));result.bookMusic=inBook.music;
  result.bookSamples=inBook.samples.players.map(player=>player.entity);
  assert.equal(result.bookSamples.length,0,'book retained a playing speech/effect owner');
  const bookSource=await evaluate('__sessionCd.sources.length-1');
  result.bookOutput=await outputWindow(bookSource,true,'book-output');
  await key('Escape',27);await until('world resumed','__cyrusStartup.state==="InGame"');
  const frame=await evaluate('__cyrusCapture.scripts.globals.world_loop_frame');
  await until('resumed ordinary update',`__cyrusCapture.scripts.globals.world_loop_frame>${frame}+2`);
  result.resumedDialogue=await evaluate('structuredClone(__cyrusCapture.scripts.globals.dialogue)');
  const retired=result.subtitle.records[result.subtitle.presented];
  assert(!result.resumedDialogue.records.some(record=>record.active&&
    record.owner?.generation===retired.owner.generation&&record.owner?.index===retired.owner.index&&record.text===retired.text),
    'closing book retained the retired utterance');
  writeFileSync(`${output}/resumed.png`,Buffer.from((await cdp.call('Page.captureScreenshot',{format:'png'})).data,'base64'));
  result.resumedMusic=body(await session('capture')).music;
  assert.equal(result.resumedMusic.track,2,'closing book reselected world music');
  await key('Escape',27);await until('second book','__cyrusStartup.state==="Menu"&&__cyrusMenu.ready');
  }else{await key('Escape',27);await until('muting book','__cyrusStartup.state==="Menu"&&__cyrusMenu.ready');}
  await select(4);await key('Enter',13);await until('Options','__cyrusMenu.page===4&&__cyrusMenu.ready');
  await select(406);await key('Enter',13);await until('Sound','__cyrusMenu.page===6&&__cyrusMenu.ready');
  await select(602);
  for(let step=0;await evaluate('__cyrusMenu.preferences.values[602]>0');step++){
    assert(step<21,'Music slider did not reach zero');
    const before=await evaluate('__cyrusMenu.preferences.values[602]');
    await cdp.call('Input.dispatchKeyEvent',{type:'keyDown',code:'ArrowLeft',key:'ArrowLeft',windowsVirtualKeyCode:37});
    try{await until('mute preference',`__cyrusMenu.preferences.values[602]!==${before}`,5);}
    finally{await cdp.call('Input.dispatchKeyEvent',{type:'keyUp',code:'ArrowLeft',key:'ArrowLeft',windowsVirtualKeyCode:37});}
    await until('mute presentation','__cyrusMenu.ready');
  }
  result.mutedMusic=body(await session('capture')).music;
  assert(result.mutedMusic.playback.paused,'zero Music slider did not pause the CD');
  const count=await evaluate('__sessionCd.sources.length');
  result.muted=await outputWindow(count-1,false,'muted-before-restore');
  if(!fixture.isolate_publication){
    for(const page of [4,0]){await key('Escape',27);await until('book return page',`__cyrusMenu.page===${page}&&__cyrusMenu.ready`);}
    await key('Escape',27);await until('muted world','__cyrusStartup.state==="InGame"');
    const generation=await evaluate('__cyrusCapture.scripts.generation');
    await evaluate(`(()=>{const read=cyrusGameAssets.read.bind(cyrusGameAssets);window.__bookDestination={read,returned:0};cyrusGameAssets.read=async path=>{
      if(path.startsWith(${JSON.stringify(fixture.destination+'.')})){__bookDestination.pending=true;await new Promise(resolve=>(__bookDestination.release??=[]).push(resolve));const bytes=await read(path);__bookDestination.returned++;return bytes;}return read(path);
    };return true})()`);
    await action(fixture.destination);await until('held destination','__bookDestination.pending');
    try{await session('load',{key:'before-book'});assert.fail('pending destination restore unexpectedly published');}
    catch(error){assert(/world preparation is in flight/.test(String(error)),String(error));result.rejectedPendingRestore=String(error);}
    assert.equal(await evaluate('__sessionCd.sources.length'),count,'rejected load submitted music');
    result.rejectedSilence=await outputWindow(count-1,false,'pending-rejection-silence');
    await action(fixture.alias);
    await until('settled replacement world',`__cyrusCapture.ready&&__cyrusCapture.scripts.generation!==${generation}`,600);
    const admitted=await evaluate('__cyrusCapture.scripts.generation');
    await evaluate('cyrusGameAssets.read=__bookDestination.read;__bookDestination.release.forEach(resolve=>resolve());true');
    await until('superseded destination completes','__bookDestination.returned>0');
    const frame=await evaluate('__cyrusCapture.scripts.globals.world_loop_frame');
    await until('settled replacement continues',`__cyrusCapture.scripts.globals.world_loop_frame>${frame}+2`);
    assert.equal(await evaluate('__cyrusCapture.scripts.generation'),admitted,'late destination replaced settled owner');
    assert.equal(await evaluate('__sessionCd.sources.length'),count,'late destination bypassed live mute');
  }
  await session('load',{key:'before-book'});
  await until('restored world entry','__cyrusStartup.state==="InGame"');
  result.restoredMusic=body(await session('capture')).music;
  assert.equal(result.restoredMusic.track,result.savedMusic.track,'muted publication lost the saved selection');
  assert.equal(result.restoredMusic.gain,result.savedMusic.gain,'muted publication changed the saved drive gain');
  assert(result.restoredMusic.playback.paused,'muted publication did not preserve host inhibition');
  result.restoredSilence=await outputWindow(await evaluate('__sessionCd.sources.length-1'),false,'muted-restored');
  result.sources=await evaluate('__sessionCd.status()');
  if(result.sources.sources.length>count){
    const leaked=await evaluate(`(()=>{
      const p=__sessionCd,s=p.sources[${count}],start=Math.ceil((s.frame+128)/128)*128,end=Math.min(s.stopped,start+8192);
      const blocks=p.blocks.filter(b=>b.frame>=start&&b.frame<end);
      if(end<=start||blocks.length!==(end-start)/128||blocks.some((b,i)=>b.frame!==start+i*128))throw Error('incomplete publication PCM window');
      let energy=0;for(const b of blocks)for(const c of [b.left,b.right])for(const v of c)energy+=v*v;
      return {start,end,frames:end-start,energy,pcm:blocks.flatMap(b=>Array.from(b.left).flatMap((v,i)=>[v,b.right[i]]))};
    })()`);
    const bytes=Buffer.alloc(leaked.pcm.length*4);leaked.pcm.forEach((v,i)=>bytes.writeFloatLE(v,i*4));
    writeFileSync(`${output}/publication-leak.f32le`,bytes);delete leaked.pcm;
    result.publicationLeak=leaked;writeFileSync(`${output}/publication-leak.json`,JSON.stringify(leaked,null,2));
  }
  assert.equal(result.sources.sources.length,count,'muted session publication admitted an unpaused CD source before applying live mute');
  if(!fixture.isolate_publication){
    await key('Escape',27);await until('restored paused book','__cyrusStartup.state==="Menu"&&__cyrusMenu.ready');
    await select(4);await key('Enter',13);await until('restored Options','__cyrusMenu.page===4&&__cyrusMenu.ready');
    await select(406);await key('Enter',13);await until('restored Sound','__cyrusMenu.page===6&&__cyrusMenu.ready');
    await select(602);
    result.beforeUnmute=await evaluate('structuredClone(__cyrusMenu)');
    await cdp.call('Input.dispatchKeyEvent',{type:'keyDown',code:'ArrowRight',key:'ArrowRight',windowsVirtualKeyCode:39});
    try{await until('sampled post-restore Music input','__cyrusMenu.preferences.values[602]>0',5);}
    finally{await cdp.call('Input.dispatchKeyEvent',{type:'keyUp',code:'ArrowRight',key:'ArrowRight',windowsVirtualKeyCode:39});}
    result.afterUnmute=await evaluate('structuredClone(__cyrusMenu)');
    await until('post-restore unmute','__sessionCd.sources.length>'+count);
    result.unmuted=await outputWindow(count,true,'post-restore-unmute');
    result.unmutedMusic=body(await session('capture')).music;
    assert.equal(result.unmutedMusic.track,2,'post-restore book unmute selected a stale world track');
  }

}
