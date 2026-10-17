// Atomic session documents; callers own encoding and validation.
export async function open(name) {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(name, 1);
    let blocked = false;
    request.onupgradeneeded = () => request.result.createObjectStore('documents');
    request.onblocked = () => {
      blocked = true;
      reject(new Error(`opening ${name}: database upgrade blocked`));
    };
    request.onerror = () => reject(new Error(`opening ${name}: ${request.error}`));
    request.onsuccess = () => {
      if (blocked) { request.result.close(); return; }
      request.result.onversionchange = () => request.result.close();
      resolve(request.result);
    };
  });
}

async function transact(name, key, mode, operation) {
  const db = await open(name);
  try {
    return await new Promise((resolve, reject) => {
      const transaction = mode === 'readwrite'
        ? db.transaction('documents', mode, { durability: 'strict' })
        : db.transaction('documents', mode);
      let result;
      transaction.oncomplete = () => resolve(result);
      transaction.onabort = () => reject(new Error(`${mode} ${name}/${key}: ${transaction.error ?? 'transaction aborted'}`));
      try {
        const request = operation(transaction.objectStore('documents'));
        request.onsuccess = () => { result = request.result; };
      } catch (error) {
        transaction.abort();
        reject(new Error(`${mode} ${name}/${key}: ${error}`));
      }
    });
  } finally {
    db.close();
  }
}

export const read = (name, key) => transact(name, key, 'readonly', store => store.get(key));
export const write = (name, key, document) => transact(name, key, 'readwrite', store => store.put(document, key));

// Slot metadata and opaque payloads share a transaction, but listing reads metadata only.
export async function operateSlots(name, encodedRequest, document) {
  const request = JSON.parse(encodedRequest);
  const { type, slot, revision, description } = request;
  const context = `${name}/${type}/${slot ?? 'new'}`;
  const check = (condition, message) => { if (!condition) throw new Error(`${context}: ${message}`); };
  check(['list', 'create', 'overwrite', 'load', 'delete'].includes(type), 'unknown slot operation');
  if (type !== 'list' && type !== 'create') {
    check(Number.isInteger(slot) && slot >= 0 && slot <= 299, 'invalid slot');
    check(typeof revision === 'string' && revision.length > 0, 'missing slot revision');
  }
  if (type === 'create' || type === 'overwrite') {
    check(typeof description === 'string' && description.length > 0 && description.length <= 64
      && !/[^\x20-\x7e]/.test(description), 'description must contain 1..64 printable ASCII characters');
    check(typeof document === 'string' && document.length > 0, 'empty session document');
    check(type !== 'overwrite' || slot !== 0, 'quick-save slot cannot be manually overwritten');
  }
  const metadataKey = value => ['metadata', value];
  const payloadKey = value => ['payload', value];
  const range = prefix => IDBKeyRange.bound([prefix, 0], [prefix, 299]);
  const validateMetadata = value => {
    check(value && Number.isInteger(value.slot) && value.slot >= 0 && value.slot <= 299
      && typeof value.description === 'string' && value.description.length > 0
      && value.description.length <= 64 && !/[^\x20-\x7e]/.test(value.description)
      && Number.isSafeInteger(value.modified_ms) && value.modified_ms >= 0
      && typeof value.revision === 'string' && value.revision.length > 0, 'invalid slot metadata');
    return value;
  };
  const db = await open(name);
  try {
    return await new Promise((resolve, reject) => {
      const mode = type === 'list' || type === 'load' ? 'readonly' : 'readwrite';
      const transaction = mode === 'readwrite'
        ? db.transaction('documents', mode, { durability: 'strict' })
        : db.transaction('documents', mode);
      const store = transaction.objectStore('documents');
      let result;
      let failure;
      transaction.oncomplete = () => resolve(JSON.stringify(result));
      transaction.onabort = () => reject(new Error(`${context}: ${failure ?? transaction.error ?? 'transaction aborted'}`));
      const consume = (pending, callback) => {
        pending.onsuccess = () => {
          try { callback(pending.result); }
          catch (error) { failure = error; transaction.abort(); }
        };
      };
      const publish = number => {
        const metadata = { slot: number, description, modified_ms: Date.now(), revision: crypto.randomUUID() };
        store.put(document, payloadKey(number));
        store.put(metadata, metadataKey(number));
        result = { stored: true, slot: metadata };
      };
      if (type === 'list') {
        const entries = [];
        consume(store.openCursor(range('metadata')), function next(cursor) {
          if (cursor) {
            const metadata = validateMetadata(cursor.value);
            check(metadata.slot === cursor.key[1], 'slot metadata key mismatch');
            entries.push(metadata);
            cursor.continue();
          } else {
            entries.sort((a, b) => b.modified_ms - a.modified_ms || a.slot - b.slot);
            result = { slots: entries };
          }
        });
      } else if (type === 'create') {
        // Both scans and publication hold the same write transaction across tabs.
        consume(store.getAllKeys(range('metadata')), metadataKeys => {
          consume(store.getAllKeys(range('payload')), payloadKeys => {
            const occupied = new Set([...metadataKeys, ...payloadKeys].map(key => key[1]));
            let number = 2;
            while (number <= 299 && occupied.has(number)) number++;
            check(number <= 299, 'no free manual save slots');
            publish(number);
          });
        });
      } else {
        consume(store.get(metadataKey(slot)), value => {
          const metadata = validateMetadata(value);
          check(metadata.slot === slot && metadata.revision === revision, 'slot changed; refresh the save list');
          consume(store.getKey(payloadKey(slot)), key => {
            check(key !== undefined, 'slot payload is absent');
            if (type === 'load') {
              consume(store.get(payloadKey(slot)), value => {
                check(typeof value === 'string' && value.length > 0, 'slot payload is not session text');
                result = { document: value };
              });
            } else if (type === 'overwrite') {
              publish(slot);
            } else {
              store.delete(metadataKey(slot));
              store.delete(payloadKey(slot));
              result = { deleted: true, slot };
            }
          });
        });
      }
    });
  } catch (error) {
    throw new Error(`${context}: ${error}`);
  } finally {
    db.close();
  }
}
