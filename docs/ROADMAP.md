# ROADMAP

Present state and next steps only; history is in git.

## Now: scaffold

- [x] Workspace, fidelity switch, install discovery, startup preload with
      progress bar, scene export through `rgpre` and glTF load, two
      placeholder controllers, TCP control server, MCP server.
- [x] Original game runs in DOSBox-X (Voodoo emulation) behind noVNC.
- [x] Browser build (`cyrus-web`) shows exported scenes from the server.
- [x] WORLD.INI selects map, terrain and palette together; numeric world IDs
      preserve palette variants. The CLI exports the full world list.
- [ ] cyrus renders a scene under the virtual display (docker/dev) and the
      first side-by-side screenshot exists.
- [ ] Preload coverage report: which file types in the install fail to parse.

## Next: the island

1. Verify scene placement and rendering against the original in every world.
2. Player start position from the scene data; the original keyboard scheme
   from `KEYS.INI`, verified against the original; collision.
3. Sky, water and lighting as the Glide build renders them.
4. Actors from RGM, idle animations, SOUP script interpreter stub that logs
   unhandled functions instead of guessing.

## Later

Dialogue (RTX), combat (COMBAT.INI), inventory (ITEM.INI), menus (MENU.INI,
GXA, fonts), save games, audio (SFX, music tracks from the CD image).

## Deferred by decision

Multiplayer, content the original did not have, any decoder written from
third-party GPL sources.
