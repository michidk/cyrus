// Bevy owns the context; browser activation must resume it synchronously in the input event.
export function unlockAudio(host = window) {
  const contexts = new Set();
  const NativeAudioContext = host.AudioContext;
  host.AudioContext = class extends NativeAudioContext {
    constructor(...args) {
      super(...args);
      contexts.add(this);
    }
  };
  const resume = () => {
    for (const context of contexts) {
      if (context.state === "closed") contexts.delete(context);
      else if (context.state === "suspended" || context.state === "interrupted") {
        context.resume().catch(error => console.warn("Audio activation failed", error));
      }
    }
  };
  host.addEventListener("keydown", resume, { capture: true });
  host.addEventListener("pointerdown", resume, { capture: true });
}
