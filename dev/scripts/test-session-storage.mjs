import { read, write } from '../../crates/cyrus-web/session-storage.js';

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
