// Vite plugin: serve and bundle Mate's sounds. The audio engine fetches /sounds/<kind>/<name>.ogg
// (src/audio/sound-bank.ts); the OGG files live in assets/sounds/{cues,sfx,amb}. Without this they
// were never served by the dev server or copied into dist/, so the app played no sound (owner,
// 2026-11-01). Plain JavaScript: the project has no Node type definitions (vite-sounds.d.mts).
import { cpSync, createReadStream, existsSync, statSync } from "node:fs";
import { dirname, join, normalize, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const SOUND_KINDS = ["cues", "sfx", "amb"];
const SOUNDS_DIR = resolve(dirname(fileURLToPath(import.meta.url)), "../assets/sounds");

/** @returns {import("vite").Plugin} */
export function sounds() {
  let outDir = "dist";
  return {
    name: "metamate-sounds",
    configResolved(config) {
      outDir = resolve(config.root, config.build.outDir);
    },
    configureServer(server) {
      server.middlewares.use("/sounds", (req, res, next) => {
        const path = normalize(decodeURIComponent((req.url ?? "").split("?")[0]));
        const kind = path.split(/[\\/]/).filter(Boolean)[0];
        const file = join(SOUNDS_DIR, path);
        if (!SOUND_KINDS.includes(kind) || !file.startsWith(SOUNDS_DIR + sep) || !file.endsWith(".ogg") || !existsSync(file)) {
          next();
          return;
        }
        res.setHeader("Content-Type", "audio/ogg");
        res.setHeader("Content-Length", statSync(file).size);
        createReadStream(file).pipe(res);
      });
    },
    closeBundle() {
      for (const kind of SOUND_KINDS) {
        cpSync(join(SOUNDS_DIR, kind), join(outDir, "sounds", kind), { recursive: true });
      }
    },
  };
}
