import { read, write, operateSlots } from '../../crates/cyrus-web/session-storage.js';

// Run in a browser with real IndexedDB; deletes only its unique fixture database.
export async function testSessionStorage() {
  const name = `cyrus-storage-contract-${crypto.randomUUID()}`;
  const originalTransaction = IDBDatabase.prototype.transaction;
  const writes = [];
  let abortWrite = false;
  const assert = (condition, message) => { if (!condition) throw new Error(message); };
  IDBDatabase.prototype.transaction = function (...args) {
    const transaction = originalTransaction.apply(this, args);
    if (this.name === name && args[1] === 'readwrite') {
      writes.push(transaction.durability);
      if (abortWrite) queueMicrotask(() => transaction.abort());
    }
    return transaction;
  };
  try {
    assert(await read(name, 'slot') === undefined, 'new slot must be absent');
    await write(name, 'slot', 'complete accepted document');
    assert(await read(name, 'slot') === 'complete accepted document', 'completed write did not persist');
    assert(writes[0] === 'strict', `write durability was ${writes[0]}`);
    abortWrite = true;
    let rejected = false;
    try { await write(name, 'slot', 'unaccepted replacement'); }
    catch (error) { rejected = String(error).includes(`${name}/slot`); }
    assert(rejected, 'aborted write did not reject with database and slot context');
    assert(await read(name, 'slot') === 'complete accepted document', 'aborted write replaced accepted bytes');
    assert(writes.length === 2 && writes.every(value => value === 'strict'), 'replacement did not use strict durability');
    return { passed: true, durability: 'strict', failedWritePreserved: true };
  } finally {
    IDBDatabase.prototype.transaction = originalTransaction;
    await new Promise((resolve, reject) => {
      const request = indexedDB.deleteDatabase(name);
      request.onsuccess = resolve;
      request.onerror = () => reject(request.error);
      request.onblocked = () => reject(new Error(`deleting ${name}: fixture database still open`));
    });
  }
}

export async function testSessionSlots(operate = operateSlots) {
  const name = `cyrus-slot-contract-${crypto.randomUUID()}`;
  const originalPut = IDBObjectStore.prototype.put;
  const originalDelete = IDBObjectStore.prototype.delete;
  const originalGet = IDBObjectStore.prototype.get;
  const originalTransaction = IDBDatabase.prototype.transaction;
  const originalNow = Date.now;
  const assert = (condition, message) => { if (!condition) throw new Error(message); };
  const operation = async (request, document) => JSON.parse(await operate(name, JSON.stringify(request), document));
  const rejected = async (request, document) => {
    try { await operation(request, document); }
    catch (error) { assert(String(error).includes(name), 'error lacks database context'); return; }
    throw new Error(`accepted invalid operation ${request.type}`);
  };
  let abortPut = false;
  let abortDelete = false;
  let listing = false;
  const durability = [];
  IDBDatabase.prototype.transaction = function (...args) {
    const transaction = originalTransaction.apply(this, args);
    if (this.name === name && args[1] === 'readwrite') durability.push(transaction.durability);
    return transaction;
  };
  IDBObjectStore.prototype.put = function (...args) {
    const pending = originalPut.apply(this, args);
    if (this.transaction.db.name === name && abortPut && args[1]?.[0] === 'payload') {
      pending.addEventListener('success', () => this.transaction.abort());
    }
    return pending;
  };
  IDBObjectStore.prototype.delete = function (...args) {
    const pending = originalDelete.apply(this, args);
    if (this.transaction.db.name === name && abortDelete) {
      pending.addEventListener('success', () => this.transaction.abort());
    }
    return pending;
  };
  IDBObjectStore.prototype.get = function (...args) {
    assert(!(listing && this.transaction.db.name === name && args[0]?.[0] === 'payload'), 'catalog copied a session payload');
    return originalGet.apply(this, args);
  };
  try {
    Date.now = () => 1000;
    await write(name, 'diagnostic', 'accepted named snapshot');
    assert((await operation({ type: 'list' })).slots.length === 0, 'new catalog not empty');
    const writesBeforePlan = durability.length;
    const planned = await operation({ type: 'create', description: 'PLANNED', prepare: true });
    assert(planned.slot.slot === 2 && planned.slot.description === 'PLANNED', 'save admission selected the wrong slot');
    assert((await operation({ type: 'list' })).slots.length === 0, 'save admission wrote a slot');
    assert(durability.length === writesBeforePlan, 'save admission opened a write transaction');
    const created = await Promise.all(Array.from({ length: 5 }, (_, index) =>
      operation({ type: 'create', description: `SAVE ${index}` }, `session ${index}`)));
    const numbers = created.map(value => value.slot.slot).sort((a, b) => a - b);
    assert(JSON.stringify(numbers) === '[2,3,4,5,6]', 'concurrent allocation overwrote a slot');
    await rejected({ type: 'create', description: 'PLANNED', expected_slot: planned.slot.slot }, 'late capture');
    listing = true;
    const tied = (await operation({ type: 'list' })).slots;
    listing = false;
    assert(JSON.stringify(tied.map(value => value.slot)) === '[2,3,4,5,6]', 'timestamp ties not ordered by slot');
    for (const value of created) {
      const loaded = await operation({ type: 'load', ...value.slot });
      assert(loaded.document === `session ${value.slot.description.slice(-1)}`, 'concurrent payload mismatch');
    }
    const accepted = tied[0];
    const payload = (await operation({ type: 'load', ...accepted })).document;
    abortPut = true;
    await rejected({ type: 'overwrite', ...accepted, description: 'FAILED' }, 'unaccepted payload');
    await rejected({ type: 'create', description: 'FAILED NEW' }, 'unaccepted new payload');
    abortPut = false;
    assert((await operation({ type: 'load', ...accepted })).document === payload, 'aborted overwrite replaced payload');
    assert(JSON.stringify((await operation({ type: 'list' })).slots) === JSON.stringify(tied), 'aborted publication changed catalog');
    abortDelete = true;
    await rejected({ type: 'delete', ...accepted });
    abortDelete = false;
    assert((await operation({ type: 'load', ...accepted })).document === payload, 'aborted deletion removed payload');
    Date.now = () => 2000;
    const plannedOverwrite = await operation({ type: 'overwrite', ...accepted, description: 'UPDATED', prepare: true });
    assert(plannedOverwrite.slot.slot === accepted.slot && plannedOverwrite.slot.description === 'UPDATED', 'overwrite admission lost selection');
    assert((await operation({ type: 'load', ...accepted })).slot.description === accepted.description, 'overwrite admission changed saved metadata');
    const replacement = (await operation({ type: 'overwrite', ...accepted, description: 'UPDATED' }, 'replacement')).slot;
    assert(replacement.revision !== accepted.revision, 'overwrite retained stale revision');
    for (const type of ['load', 'delete', 'overwrite']) await rejected({ type, ...accepted }, 'stale');
    assert((await operation({ type: 'load', ...replacement })).document === 'replacement', 'overwrite not retained');
    assert((await operation({ type: 'list' })).slots[0].slot === replacement.slot, 'newest slot not first');
    for (const description of ['', 'x'.repeat(65), 'bad\ntext']) {
      await rejected({ type: 'create', description }, 'bad');
    }
    for (const slot of [-1, 300, 1.5]) await rejected({ type: 'delete', slot, revision: 'x' });
    const originalInput = await operation({ type: 'create', description: 'Mixed\x0bcase\rtext' }, 'original input');
    assert((await operation({ type: 'list' })).slots.some(entry => entry.description === 'Mixed\x0bcase\rtext'), 'original input characters did not round trip');
    await operation({ type: 'delete', ...originalInput.slot });
    const max = await operation({ type: 'create', description: 'x'.repeat(64) }, 'max description');
    assert(max.slot.slot === 7, 'failed new save consumed its slot');
    await operation({ type: 'delete', ...replacement });
    const reused = await operation({ type: 'create', description: 'REUSED' }, 'reused');
    assert(reused.slot.slot === 2, 'first free manual slot not reused');
    const thumb = Array.from({ length: 256 * 192 }, (_, index) => index % 251);
    const thumbSave = await operation({ type: 'create', description: 'THUMBNAIL', thumbnail: thumb }, 'thumbnail payload');
    assert(JSON.stringify((await operation({ type: 'list' })).slots.find(entry => entry.slot === thumbSave.slot).thumbnail) === JSON.stringify(thumb), 'thumbnail metadata did not round trip');
    const quick = await operation({ type: 'quick_save' }, 'quick payload');
    assert(quick.slot.slot === 0 && quick.slot.description === 'Quick Save Game', 'quick-save slot contract failed');
    const auto = await operation({ type: 'auto_save' }, 'auto payload');
    assert(auto.slot.slot === 1 && auto.slot.description === 'Redguard AutoSave', 'autosave slot contract failed');
    for (const [type, slot, description] of [
      ['quick_save', 0, 'Quick Save Game'], ['auto_save', 1, 'Redguard AutoSave'],
    ]) {
      const planned = await operation({ type, prepare: true });
      assert(planned.slot.slot === slot, 'reserved save preparation moved its slot');
      const replacement = await operation({ type, expected_slot: slot, expected_revision: planned.revision }, 'replacement reserved payload');
      await rejected({ type, expected_slot: slot, expected_revision: planned.revision }, 'stale reserved payload');
      await rejected({ type, expected_slot: slot, expected_revision: null }, 'unexpected occupied reserved payload');
      assert(replacement.slot.slot === slot && replacement.slot.description === description,
        'repeated reserved save moved its slot or changed its description');
      assert((await operation({ type: 'load', ...replacement.slot })).document === 'replacement reserved payload',
        'repeated reserved save did not replace its payload');
    }
    await rejected({ type: 'overwrite', slot: 1, revision: auto.slot.revision, description: 'MANUAL' }, 'forbidden');
    await operation({ type: 'discard_auto_save' });
    await operation({ type: 'discard_auto_save' });
    const kept = (await operation({ type: 'list' })).slots.map(entry => entry.slot);
    assert(!kept.includes(1) && kept.includes(0), 'discarding the autosave removed only slot 1');
    assert(await read(name, 'diagnostic') === 'accepted named snapshot', 'slot operations touched named snapshots');
    const db = await new Promise((resolve, reject) => {
      const request = indexedDB.open(name, 1);
      request.onsuccess = () => resolve(request.result);
      request.onerror = () => reject(request.error);
    });
    await new Promise((resolve, reject) => {
      const transaction = db.transaction('documents', 'readwrite', { durability: 'strict' });
      const store = transaction.objectStore('documents');
      for (let slot = 0; slot <= 299; slot++) {
        if (slot >= 2 && slot <= 7) continue;
        store.put({ slot, description: 'RESERVED OR FULL', modified_ms: 0, revision: `seed-${slot}` }, ['metadata', slot]);
        store.put('seed', ['payload', slot]);
      }
      transaction.oncomplete = resolve;
      transaction.onabort = () => reject(transaction.error);
    });
    db.close();
    await rejected({ type: 'create', description: 'FULL' }, 'overflow');
    await rejected({ type: 'overwrite', slot: 0, revision: 'seed-0', description: 'QUICK' }, 'forbidden');
    assert((await operation({ type: 'list' })).slots.length === 300, 'full catalog was changed');
    assert(durability.length > 0 && durability.every(value => value === 'strict'), 'slot write used relaxed durability');
    return { passed: true, concurrentSlots: numbers, abortedPublicationPreserved: true,
      abortedDeletePreserved: true, staleRevisionRejected: true, fullCatalogPreserved: true,
      namedSnapshotPreserved: true, durability: 'strict' };
  } finally {
    Date.now = originalNow;
    IDBObjectStore.prototype.put = originalPut;
    IDBObjectStore.prototype.delete = originalDelete;
    IDBObjectStore.prototype.get = originalGet;
    IDBDatabase.prototype.transaction = originalTransaction;
    await new Promise((resolve, reject) => {
      const request = indexedDB.deleteDatabase(name);
      request.onsuccess = resolve;
      request.onerror = () => reject(request.error);
      request.onblocked = () => reject(new Error(`deleting ${name}: fixture database still open`));
    });
  }
}
