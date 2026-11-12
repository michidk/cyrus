import assert from 'node:assert/strict';
import test from 'node:test';
import {validateObservation,requireSecureOrigin,activeVoice,validateRetirement} from './browser-authored-speech.mjs';
const cleared=()=>({generation:9,dialogue:{presented:null,records:[{active:false,owner:{generation:8,index:4},sample:null,text:''}]}});
test('cleared records may retain speaker identity but never text or a sample',()=>{validateObservation(cleared(),9,true);});
test('old inactive sample-zero regression rejects replacement',()=>{const row=cleared();row.dialogue.records[0].sample=0;assert.throws(()=>validateObservation(row,9,true),/sample handle/);});
test('retired active owner and stale presentation reject',()=>{let row=cleared();row.dialogue.records[0].active=true;assert.throws(()=>validateObservation(row,9),/retired speaker/);row=cleared();row.dialogue.presented=0;assert.throws(()=>validateObservation(row,9,true),/presentation/);});
test('generation identity cannot relabel an old receipt',()=>{assert.throws(()=>validateObservation(cleared(),10),/different world generation/);});
test('bridge HTTP rejects before expensive installed preparation',()=>{assert.throws(()=>requireSecureOrigin('http://172.17.0.1:19362/'),/secure|HTTPS/);for(const url of ['http://127.0.0.1:19362/','http://localhost:19362/','https://example.test/'])requireSecureOrigin(url);});

test('speech admission rejects a stale subtitle handle and unrelated effect',()=>{const row={dialogue:{presented:0,records:[{active:true,sample:0}]}};const saved={samples:{ownership:{samples:{slots:[{token:7,effect:null}]}},players:[]}};assert.throws(()=>activeVoice(saved,row),/no admitted/);saved.samples.players=[{token:7,entity:10}];assert.equal(activeVoice(saved,row).entity,10);saved.samples.ownership.samples.slots[0].effect=3;assert.throws(()=>activeVoice(saved,row),/effect/);});

test('new-world ambience is valid but retired speech identities are rejected',()=>{const voice={entity:3,transport:{source_identity:'voice'}};const before={players:[voice]};const after={players:[{entity:4,transport:{source_identity:'ambience'}}]};validateRetirement(before,after,voice);after.players[0].entity=3;assert.throws(()=>validateRetirement(before,after,voice),/old playback/);after.players[0].entity=4;after.players[0].transport.source_identity='voice';assert.throws(()=>validateRetirement(before,after,voice),/voice source/);});
