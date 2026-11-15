// Map the selected install and its media companions; bytes stay in File objects.
export function localInstall(entries) {
  const selected = new Map();
  for (const [path, file] of entries) {
    if (!path || path.split('/').some(part => !part || part === '.' || part === '..') || path.includes('\\')) {
      throw new Error('Invalid selected file path: ' + path);
    }
    const key = path.toLowerCase();
    if (selected.has(key)) throw new Error('Duplicate selected file: ' + path);
    selected.set(key, { path, file });
  }
  const roots = [...selected.values()].map(({ path }) => path)
    .filter(path => path.toLowerCase() === 'rgfx.exe' || path.toLowerCase().endsWith('/rgfx.exe'))
    .map(path => path.slice(0, -'rgfx.exe'.length))
    .filter(root => [...selected.keys()].some(path => path.startsWith(root.toLowerCase() + 'fxart/')) &&
      [...selected.keys()].some(path => path.startsWith(root.toLowerCase() + 'maps/')));
  if (roots.length !== 1) throw new Error(roots.length ? 'Choose a folder containing just one Redguard installation.' :
    'No Redguard installation found. Choose a folder containing RGFX.EXE, fxart and maps.');
  const root = roots[0];
  const parent = root.slice(0, root.slice(0, -1).lastIndexOf('/') + 1);
  const files = new Map();
  for (const [key, { path, file }] of selected) {
    if (key.startsWith(root.toLowerCase())) files.set(path.slice(root.length), file);
  }
  function add(name, file) {
    const existing = [...files.keys()].find(path => path.toLowerCase() === name.toLowerCase());
    if (existing && files.get(existing) !== file) throw new Error('Conflicting game file: ' + name);
    if (existing) files.delete(existing);
    files.set(name, file);
  }
  function companion(name, candidates) {
    let found = candidates.map(path => selected.get(path.toLowerCase())).find(Boolean);
    if (!found) {
      const basename = name.split('/').at(-1).toLowerCase();
      const matches = [...selected.values()].filter(({ path }) => path.split('/').at(-1).toLowerCase() === basename);
      if (matches.length > 1) throw new Error('Multiple copies of ' + basename + ' found. Choose a folder containing only the game and its support files.');
      found = matches[0];
    }
    if (!found) return;
    add(name, found.file);
  }
  for (const name of ['game.gog', 'game.ins']) companion('disc/' + name, [root + name, parent + name]);
  companion('splash/3DfxSpl2.dll', [root + '3dfxspl2.dll', parent + 'dosbox/3dfxspl2.dll']);
  companion('splash/glide2x_emu.ovl', [root + 'glide2x_emu.ovl', parent + 'dosbox/glide2x_emu.ovl']);
  for (const { path, file } of selected.values()) {
    if (!path.toLowerCase().startsWith(parent.toLowerCase())) continue;
    const relative = path.slice(parent.length).split('/');
    if (relative.length !== 2) continue;
    const [directory, filename] = relative;
    if (directory.toLowerCase() === 'cdrom') add('disc/' + filename, file);
    if (directory.toLowerCase() === 'soundtrack') {
      const track = /^([0-9]{2}).*\.mp3$/i.exec(filename);
      if (track && Number(track[1]) > 0) {
        add('disc/track' + String(Number(track[1]) + 1).padStart(2, '0') + '.mp3', file);
      }
    }
  }
  return { files };
}
