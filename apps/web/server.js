// Production entry (adapter-node + private lobbies). `pnpm build` also bundles
// the lobby socket layer to build/lobby-socket.js. Run with: node server.js
import { createServer } from 'node:http';
import { handler } from './build/handler.js';
import { attachLobbySockets } from './build/lobby-socket.js';

const server = createServer(handler);
attachLobbySockets(server);
const port = Number(process.env.PORT ?? 3000);
server.listen(port, process.env.HOST ?? '0.0.0.0', () => console.log(`PencilPilot on :${port}`));
