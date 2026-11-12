// Recovery binds public saved bytes to their executed build and selected fixture.
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';

export const digest=bytes=>createHash('sha256').update(bytes).digest('hex');
export const storageModuleExpression=url=>`(async()=>{window.__ambientStorage=await import(${JSON.stringify(url)});return true})()`;
export const restoreStorageExpression=document=>`__ambientStorage.write('cyrus-sessions','ambient-restoration',${JSON.stringify(document)}).then(()=>true)`;

export function validateSessionCheckpoint(checkpoint,document,identity){
  assert.equal(checkpoint.document_sha256,digest(document),'checkpoint document corrupted');
  assert.equal(checkpoint.wasm,identity.wasm,'checkpoint WASM changed');
  assert.equal(checkpoint.source_fixture_sha256,identity.source_fixture_sha256,'checkpoint source oracle changed');
  assert.deepEqual(checkpoint.fixture,identity.fixture,'checkpoint fixture changed');
  const selected=checkpoint.saved_slots.filter(slot=>slot.effect===identity.fixture.effect
    && slot.owner?.Script?.index===identity.fixture.index);
  assert.equal(selected.length,1,'public save lacks exactly one selected ambient owner');
  assert(checkpoint.saved_players.some(player=>player.token===selected[0].token),'saved decoder absent');
}
