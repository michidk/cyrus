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
      const transaction = db.transaction('documents', mode);
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
