// `pnpm tauri …`, with `dev` taking the first free port from 1420 so several worktrees can
// run side by side. Vite gets it through GITVIBER_DEV_PORT and the window through devUrl;
// letting Vite pick its own port would load another worktree's frontend from 1420.
import { spawn } from "node:child_process";
import { createServer } from "node:net";

const args = process.argv.slice(2);
const env = { ...process.env };

if (args[0] === "dev") {
  const free = (port) =>
    new Promise((resolve) => {
      const server = createServer()
        .once("error", () => resolve(false))
        .once("listening", () => server.close(() => resolve(true)))
        .listen(port, "127.0.0.1");
    });
  let port = 1420;
  while (!(await free(port))) port++;
  env.GITVIBER_DEV_PORT = String(port);
  args.push("--config", JSON.stringify({ build: { devUrl: `http://localhost:${port}` } }));
}

spawn("tauri", args, { stdio: "inherit", env }).on("exit", (code) => process.exit(code ?? 1));
