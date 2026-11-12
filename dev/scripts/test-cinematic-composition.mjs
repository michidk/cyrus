import {test} from 'node:test';
import assert from 'node:assert/strict';
import vm from 'node:vm';
import {installCinematicComposition} from './cinematic-composition.mjs';
test('retain installed movie metadata before consumer without modifying worker payload', () => {
  class Worker {
    listeners=[];
    addEventListener(kind, listener) {assert.equal(kind,'message');this.listeners.push(listener);}
    emit(data) {for(const listener of this.listeners)listener({data});}
  }
  const context=vm.createContext({Worker,TextDecoder});
  vm.runInContext(`(${installCinematicComposition.toString()})()`,context);
  const worker=vm.runInContext('new Worker("asset-worker.js")',context);
  const metadata={definition:{subtitles:[{start:2,stop:4,text:'A',always:true}]},font:{glyphs:[]}};
  const data={kind:'game-prepare',files:[['movies/0.movie',new TextEncoder().encode(JSON.stringify(metadata))]]};
  worker.emit(data);
  assert.deepEqual(JSON.parse(JSON.stringify(context.__cinematicMovieMetadata[0])),metadata);
  assert.equal(data.files[0][0],'movies/0.movie');
  worker.emit({kind:'progress'});
  assert.equal(Object.keys(context.__cinematicMovieMetadata).length,1);
});
