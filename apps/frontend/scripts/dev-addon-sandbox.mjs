import { spawn } from "node:child_process";
import { resolve } from "node:path";
import { once } from "node:events";

const forwardedViteArgs = process.argv.slice(2);
const children = new Set();
let stopping = false;

// vite/node_modules/vite/bin/vite.js — invoke directly so we never nest a
// second `pnpm` inside the pnpm-run script (pnpm refuses recursive runs).
const viteBin = resolve(import.meta.dirname, "../node_modules/vite/bin/vite.js");

function spawnVite(args) {
  const child = spawn(process.execPath, [viteBin, ...args], { stdio: "inherit" });
  children.add(child);
  child.once("exit", () => children.delete(child));
  return child;
}

async function stop(code) {
  if (stopping) return;
  stopping = true;
  for (const child of children) {
    child.kill("SIGTERM");
  }
  await Promise.all(Array.from(children, (child) => once(child, "exit").catch(() => undefined)));
  process.exitCode = code;
}

process.once("SIGINT", () => void stop(130));
process.once("SIGTERM", () => void stop(143));

const initialBuild = spawnVite([
  "build",
  "--config",
  "vite.addon-sandbox.config.ts",
]);
const [initialCode] = await once(initialBuild, "exit");
if (initialCode !== 0) {
  process.exitCode = typeof initialCode === "number" ? initialCode : 1;
} else {
  const runtimeWatcher = spawnVite([
    "build",
    "--config",
    "vite.addon-sandbox.config.ts",
    "--watch",
  ]);
  const vite = spawnVite([...forwardedViteArgs]);

  runtimeWatcher.once("exit", (code) => void stop(typeof code === "number" ? code : 1));
  vite.once("exit", (code) => void stop(typeof code === "number" ? code : 1));
}
