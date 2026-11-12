import assert from 'node:assert/strict';
import {digest,storageModuleExpression,restoreStorageExpression,validateSessionCheckpoint} from './ambient-session-checkpoint.mjs';

// Runtime.evaluate evaluates a script, even when awaitPromise is enabled.
assert.throws(()=>new Function('window.__ambientStorage=await import("./store.js");true'),SyntaxError);
new Function(`return ${storageModuleExpression('http://localhost/store.js')}`);
const document='a saved document with "quotes",\nnewlines and ${literal}';
let written;
assert.equal(await new Function('__ambientStorage',`return ${restoreStorageExpression(document)}`)({
  write:async(...args)=>{written=args},
}),true);
assert.deepEqual(written,['cyrus-sessions','ambient-restoration',document]);
const identity={wasm:'executed-build',source_fixture_sha256:'oracle',fixture:{index:3,effect:7}};
const checkpoint={...identity,document_sha256:digest(document),saved_slots:[{token:11,effect:7,owner:{Script:{generation:2,index:3}}}],saved_players:[{token:11}]};
validateSessionCheckpoint(checkpoint,document,identity);
for(const changed of [
  {...identity,wasm:'other-build'},
  {...identity,source_fixture_sha256:'other-oracle'},
  {...identity,fixture:{index:4,effect:7}},
])assert.throws(()=>validateSessionCheckpoint(checkpoint,document,changed));
assert.throws(()=>validateSessionCheckpoint(checkpoint,document+'tamper',identity));
assert.throws(()=>validateSessionCheckpoint({...checkpoint,saved_slots:[]},document,identity));
assert.throws(()=>validateSessionCheckpoint({...checkpoint,saved_players:[]},document,identity));
console.log('session checkpoint: script evaluation, literal storage round-trip and six stale/tamper controls passed');
