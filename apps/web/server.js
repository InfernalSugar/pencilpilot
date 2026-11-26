// Production entry (adapter-node + private lobbies + the app's updates). `pnpm build` also
// bundles the lobby socket layer to build/lobby-socket.js and the updates to
// build/updates.js. Run with: node server.js
import { createServer } from 'node:http';
import { handler } from './build/handler.js';
import { attachLobbySockets } from './build/lobby-socket.js';
import { createUpdatesHandler, startSweeping, updatesConfig } from './build/updates.js';

const updates = updatesConfig();
const serveUpdates = createUpdatesHandler(updates);
const server = createServer((req, res) => serveUpdates(req, res) || handler(req, res));
attachLobbySockets(server);
startSweeping(updates);
const port = Number(process.env.PORT ?? 3000);
server.listen(port, process.env.HOST ?? '0.0.0.0', () => console.log(`PencilPilot on :${port}`));
