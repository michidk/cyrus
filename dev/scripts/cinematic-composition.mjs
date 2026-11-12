// Observe the actual installed worker result before the application consumes it.
// This is diagnostic-only and never edits preparation or playback preferences.
export function installCinematicComposition() {
  const NativeWorker = globalThis.Worker;
  globalThis.__cinematicMovieMetadata = {};
  globalThis.Worker = class extends NativeWorker {
    constructor(...args) {
      super(...args);
      this.addEventListener('message', ({data}) => {
        if (data.kind !== 'game-prepare') return;
        for (const [path, bytes] of data.files ?? []) {
          const match = /^movies\/(\d+)\.movie$/.exec(path);
          if (match) globalThis.__cinematicMovieMetadata[match[1]] = JSON.parse(new TextDecoder().decode(bytes));
        }
      });
    }
  };
}
