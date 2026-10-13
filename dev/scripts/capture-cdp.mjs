// Each automated replay owns an isolated browser context and closes it afterward.
export async function connectCdp(endpoint) {
  const version = await (await fetch(`${endpoint}/json/version`)).json();
  const socket = new WebSocket(version.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => { socket.onopen = resolve; socket.onerror = reject; });
  let id = 0, pageSession;
  const pending = new Map(), listeners = new Set(), events = [];
  const call = (method, params = {}, sessionId = pageSession) => new Promise((resolve, reject) => {
    const current = ++id;
    const timer = setTimeout(() => {
      pending.delete(current);
      reject(new Error(`CDP timeout: ${method}`));
    }, ['Runtime.evaluate', 'Debugger.getScriptSource', 'Page.captureScreenshot'].includes(method) ? 300000 : 30000);
    pending.set(current, { resolve, reject, timer });
    socket.send(JSON.stringify({ id: current, method, params, sessionId: sessionId || undefined }));
  });
  socket.onmessage = ({ data }) => {
    const message = JSON.parse(data);
    if (message.method?.startsWith('Runtime.') || message.method === 'Log.entryAdded') {
      events.push(message);
      if (events.length > 256) events.shift();
    }
    for (const listener of listeners) listener(message);
    const request = pending.get(message.id);
    if (!request) return;
    pending.delete(message.id);
    clearTimeout(request.timer);
    if (message.error) request.reject(new Error(JSON.stringify(message.error)));
    else request.resolve(message.result);
  };
  socket.onclose = () => {
    for (const request of pending.values()) {
      clearTimeout(request.timer);
      request.reject(new Error("CDP connection closed"));
    }
    pending.clear();
  };
  let context;
  try {
    context = await call('Target.createBrowserContext', { disposeOnDetach: true });
    const target = await call('Target.createTarget', { url: 'about:blank', browserContextId: context.browserContextId });
    const attached = await call('Target.attachToTarget', { targetId: target.targetId, flatten: true });
    pageSession = attached.sessionId;
  } catch (error) {
    if (context) await call('Target.disposeBrowserContext', context).catch(() => {});
    socket.close();
    throw error;
  }
  return {
    call, events,
    onEvent(listener) { listeners.add(listener); return () => listeners.delete(listener); },
    async close() {
      try { await call('Target.disposeBrowserContext', context, null); }
      finally { socket.close(); }
    },
  };
}
