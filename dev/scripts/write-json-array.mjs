import {closeSync, openSync, writeSync} from 'node:fs';

// Trace arrays can exceed V8's single-string limit. Keep encoding bounded while
// preserving every event and its original order and values.
export function writeJsonArray(path, values) {
  const file=openSync(path,'w');
  try {
    writeSync(file,'[');
    let buffer='', separator='';
    for(const value of values) {
      buffer+=separator+JSON.stringify(value);separator=',';
      if(buffer.length>=1024*1024) {writeSync(file,buffer);buffer='';}
    }
    if(buffer)writeSync(file,buffer);
    writeSync(file,']');
  } finally {closeSync(file);}
}
