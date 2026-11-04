import { spawnSync } from "node:child_process";
import { createServer, build } from "vite";
const mode = process.argv[2];
if (mode === "build") {
  const result = spawnSync(process.execPath, ["node_modules/typescript/bin/tsc", "--noEmit"], { stdio: "inherit", shell: false });
  if (result.error) throw result.error;
  if (result.status) process.exit(result.status);
  await build();
} else if (mode === "dev") {
  const server = await createServer();
  await server.listen();
  server.printUrls();
} else {
  throw Error("Expected frontend build or dev");
}
