// Runs qpdf (WASM) in a Web Worker, as the real app would.
importScripts('node_modules/@neslinesli93/qpdf-wasm/dist/qpdf.js');

self.onmessage = async ({ data: { id, bytes, password } }) => {
  const log = [];
  // This build ignores the print/printErr hooks and writes to console, so capture console instead.
  for (const k of ['log', 'info', 'warn', 'error']) console[k] = (...a) => log.push(a.join(' '));
  const t0 = performance.now();
  try {
    // A fresh instance per job: Emscripten's callMain is meant to run once per instance.
    const qpdf = await Module({
      locateFile: () => 'node_modules/@neslinesli93/qpdf-wasm/dist/qpdf.wasm',
      print: (s) => log.push(s),
      printErr: (s) => log.push(s),
    });
    const tLoad = performance.now();
    qpdf.FS.writeFile('/in.pdf', new Uint8Array(bytes));

    // 1. Is it encrypted, and does the password open it?
    const showCode = qpdf.callMain([`--password=${password}`, '--show-encryption', '/in.pdf']);
    const showLog = log.splice(0);

    // 2. Write a decrypted copy.
    const code = qpdf.callMain([`--password=${password}`, '--decrypt', '/in.pdf', '/out.pdf']);
    const decryptLog = log.splice(0);
    let out = null;
    try { out = qpdf.FS.readFile('/out.pdf'); } catch { /* no output written */ }
    self.postMessage({
      id, showCode, showLog, code, decryptLog,
      out: out ? out.buffer : null,
      loadMs: Math.round(tLoad - t0), runMs: Math.round(performance.now() - tLoad),
    }, out ? [out.buffer] : []);
  } catch (e) {
    self.postMessage({ id, crash: String(e && e.stack || e), log });
  }
};
