// `pnpm tauri …`, with `dev` taking the first free port from 1420 so several worktrees can
// run side by side (`--port <n>` picks one). Vite gets it through GITVIBER_DEV_PORT and the
// window through devUrl; letting Vite pick its own port would load another worktree's
// frontend from 1420.
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
  const flag = args.indexOf("--port");
  if (flag > 0) {
    port = Number(args[flag + 1]);
    if (!Number.isInteger(port) || port <= 0) {
      console.error("--port needs a number, e.g. pnpm tauri dev --port 1421");
      process.exit(1);
    }
    args.splice(flag, 2);
  } else {
    while (!(await free(port))) port++;
  }
  env.GITVIBER_DEV_PORT = String(port);
  // Right after `dev`: tauri hands everything from the first loose argument on to the app.
  args.splice(1, 0, "--config", JSON.stringify({ build: { devUrl: `http://localhost:${port}` } }));
}

spawn("tauri", args, { stdio: "inherit", env }).on("exit", (code) => process.exit(code ?? 1));
